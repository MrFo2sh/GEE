//! The one `Store` resource: the world, the game, what is selected, what the UI asked for.
//! `drain_bridge` is the only system that applies events; every user decision goes through
//! `Store::close_signal`, so the game sees each change exactly once.

use super::settings;
use bevy::prelude::*;
use gae_core::bridge::{Bridge, ToApp};
use gae_core::game::{Game, Reward};
use gae_core::model::{Change, How, Severity, World, now_millis};
use gae_core::spec::{Cue, Spec};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

/// Most bridge messages applied per frame: a flood drains over several frames, never one long one.
const DRAIN_BUDGET: usize = 64;
const MAX_TOASTS: usize = 6;
const MAX_ERRORS: usize = 50;

#[derive(Resource)]
pub struct SpecRes(pub Spec);

#[derive(Resource)]
pub struct BridgeRes(pub Bridge);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Xp,
    Level,
    Badge,
    Warn,
    Error,
    Info,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
}

/// A signal badge over a unit, in window logical coordinates.
#[derive(Clone, Debug)]
pub struct Badge {
    pub unit: String,
    pub severity: Severity,
    pub count: usize,
    pub x: f32,
    pub y: f32,
}

/// A card's floating position (None: its default), whether its title is being dragged, its zoom.
#[derive(Debug, Clone)]
pub struct CardState {
    pub pos: Option<[f32; 2]>,
    pub dragging: bool,
    pub zoom: f32,
}

impl Default for CardState {
    fn default() -> Self {
        CardState {
            pos: None,
            dragging: false,
            zoom: 1.0,
        }
    }
}

#[derive(Resource)]
pub struct Store {
    pub world: World,
    pub game: Game,
    game_path: PathBuf,
    game_dirty: bool,
    last_game_save: Option<Instant>,
    pub selected: Option<String>,
    pub hovered: Option<String>,
    pub toasts: VecDeque<Toast>,
    pub cues: Vec<Cue>,
    /// Latest status per source, and recent errors (newest last).
    pub status: HashMap<String, String>,
    pub errors: VecDeque<(i64, String)>,
    pub events_seen: u64,
    pub pointer_over_ui: bool,
    pub typing: bool,
    /// Cinematic mode: the scene only.
    pub hide_ui: bool,
    pub reset_view: bool,
    /// Pixels the panels take on each side (left, top, right, bottom): the camera renders between.
    pub scene_insets: Option<(f32, f32, f32, f32)>,
    pub badges: Vec<Badge>,
    /// Screen positions of the selected and hovered units, for their labels.
    pub labels: Vec<(String, f32, f32)>,
    pub cards: HashMap<String, CardState>,
    pub hot_card: Option<String>,
    pub zoom_request: Option<(String, i32)>,
    pub monitor_seams: Vec<f32>,
    pub show_settings: bool,
    pub show_trophies: bool,
    pub tree_filter: String,
    pub timeline_query: String,
    pub queue_shown: usize,
    pub tree_shown: usize,
    pub timeline_shown: usize,
    pub level_flash: Option<Instant>,
}

fn now_label(text: &str) -> (String, String) {
    match text.split_once(": ") {
        Some((k, v)) => (k.to_string(), v.to_string()),
        None => ("bridge".to_string(), text.to_string()),
    }
}

impl Store {
    pub fn new(spec: &Spec) -> Store {
        let dir = settings::config_dir(spec.slug);
        let game_path = dir.join("game.json");
        let game = std::fs::read_to_string(&game_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store {
            world: World::new(spec.nouns.core),
            game,
            game_path,
            game_dirty: false,
            last_game_save: None,
            selected: None,
            hovered: None,
            toasts: VecDeque::new(),
            cues: Vec::new(),
            status: HashMap::new(),
            errors: VecDeque::new(),
            events_seen: 0,
            pointer_over_ui: false,
            typing: false,
            hide_ui: false,
            reset_view: false,
            scene_insets: None,
            badges: Vec::new(),
            labels: Vec::new(),
            cards: HashMap::new(),
            hot_card: None,
            zoom_request: None,
            monitor_seams: Vec::new(),
            show_settings: false,
            show_trophies: false,
            tree_filter: String::new(),
            timeline_query: String::new(),
            queue_shown: super::theme::PAGE,
            tree_shown: super::theme::PAGE,
            timeline_shown: 50,
            level_flash: None,
        }
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push_back(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
        while self.toasts.len() > MAX_TOASTS {
            self.toasts.pop_front();
        }
    }

    pub fn play(&mut self, cue: Cue) {
        self.cues.push(cue);
    }

    /// The human resolved or dismissed a signal in the UI.
    pub fn close_signal(&mut self, spec: &Spec, id: &str, how: How) {
        if let Some(change) = self.world.close(id, how, now_millis(), String::new()) {
            self.play(if how == How::Dismissed {
                Cue::Dismissed
            } else {
                Cue::Resolved
            });
            self.react(spec, &[change]);
        }
    }

    /// Cues for what happened, then the game's rewards for it.
    fn react(&mut self, spec: &Spec, changes: &[Change]) {
        let now = now_millis();
        for c in changes {
            match c {
                Change::UnitAdded { .. } => self.play(Cue::UnitAdded),
                Change::ActionStarted { .. } => self.play(Cue::Blip),
                Change::ActionFailed { .. } => self.play(Cue::Failure),
                Change::SignalRaised { severity, .. } => match severity {
                    Severity::Critical => self.play(Cue::Critical),
                    Severity::High | Severity::Medium => self.play(Cue::Signal),
                    _ => {}
                },
                Change::SignalClosed { how: How::Source, .. } => self.play(Cue::SourceCleared),
                Change::SignalStale { .. } => self.play(Cue::Stale),
                _ => {}
            }
            let open = self.world.open.len();
            let rewards = self.game.score(c, open, &spec.xp, &spec.achievements, now);
            if !rewards.is_empty() {
                self.game_dirty = true;
            }
            for r in rewards {
                match r {
                    Reward::Xp { amount, reason } => self.toast(ToastKind::Xp, format!("+{amount} XP  {reason}")),
                    Reward::LevelUp { level } => {
                        self.level_flash = Some(Instant::now());
                        self.play(Cue::LevelUp);
                        self.toast(ToastKind::Level, format!("LEVEL {level}"));
                    }
                    Reward::Badge { title, .. } => {
                        self.play(Cue::Badge);
                        self.toast(ToastKind::Badge, format!("🏆 {title}"));
                    }
                    Reward::StreakBroken { was } => self.toast(
                        ToastKind::Warn,
                        format!("streak of {was} broken: a {} went stale", spec.nouns.signal),
                    ),
                }
            }
        }
    }

    fn save_game(&mut self, force: bool) {
        if !self.game_dirty {
            return;
        }
        if !force && self.last_game_save.is_some_and(|t| t.elapsed().as_secs() < 5) {
            return;
        }
        self.game_dirty = false;
        self.last_game_save = Some(Instant::now());
        if let Some(dir) = self.game_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.game) {
            let _ = std::fs::write(&self.game_path, json);
        }
    }

    /// The JSON `GET /state` answers: what the user sees, for scripts and agents.
    pub fn state_json(&self, spec: &Spec) -> String {
        let queue: Vec<_> = self
            .world
            .queue()
            .iter()
            .take(20)
            .map(|s| serde_json::json!({"id": s.id, "unit": s.unit, "severity": s.severity, "title": s.title, "count": s.count}))
            .collect();
        serde_json::json!({
            "env": spec.slug,
            "units": self.world.units.len(),
            "open": self.world.open.len(),
            "queue": queue,
            "xp": self.game.xp,
            "level": self.game.level(),
            "streak": self.game.streak,
            "badges": self.game.badges.iter().map(|(b, _)| b.clone()).collect::<Vec<_>>(),
            "events": self.events_seen,
            "selected": self.selected,
        })
        .to_string()
    }
}

pub fn drain_bridge(
    mut store: ResMut<Store>,
    spec: Res<SpecRes>,
    bridge: Res<BridgeRes>,
    mut next_tick: Local<Option<Instant>>,
) {
    let spec = &spec.0;
    for _ in 0..DRAIN_BUDGET {
        let Ok(msg) = bridge.0.rx.try_recv() else { break };
        match msg {
            ToApp::Events(events) => {
                store.events_seen += events.len() as u64;
                let mut changes = Vec::new();
                for ev in events {
                    changes.extend(store.world.apply(ev));
                }
                store.react(spec, &changes);
            }
            ToApp::Status(s) => {
                let (k, v) = now_label(&s);
                store.status.insert(k, v);
            }
            ToApp::Error(e) => {
                warn!("{e}");
                store.errors.push_back((now_millis(), e.clone()));
                while store.errors.len() > MAX_ERRORS {
                    store.errors.pop_front();
                }
                store.toast(ToastKind::Error, e);
            }
        }
    }
    // Once a second: stale signals, the published state, the game file.
    let due = next_tick.is_none_or(|t| Instant::now() >= t);
    if due {
        *next_tick = Some(Instant::now() + std::time::Duration::from_secs(1));
        let stale = store.world.mark_stale(now_millis(), spec.xp.stale_ms);
        if !stale.is_empty() {
            store.react(spec, &stale);
        }
        bridge.0.publish_state(store.state_json(spec));
        store.save_game(false);
    }
    // Selection of a unit that was evicted goes away with it.
    if let Some(sel) = store.selected.clone()
        && store.world.unit(&sel).is_none()
    {
        store.selected = None;
    }
}

/// On exit, write the game file whatever the throttle says.
pub fn save_on_exit(mut exit: MessageReader<AppExit>, mut store: ResMut<Store>) {
    if exit.read().next().is_some() {
        store.save_game(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gae_core::model::Event;

    fn spec() -> Spec {
        Spec::new("Test", "gae-test-store")
    }

    #[test]
    fn resolving_from_the_ui_scores_and_cues() {
        let spec = spec();
        let mut store = Store::new(&spec);
        store.game = Game::default();
        let changes = store.world.apply(Event::Signal {
            id: "s".into(),
            unit: "u".into(),
            severity: Severity::High,
            title: "t".into(),
            detail: String::new(),
            kind: String::new(),
            at: now_millis(),
        });
        store.react(&spec, &changes);
        assert!(store.cues.contains(&Cue::Signal));
        store.close_signal(&spec, "s", How::Resolved);
        assert!(store.world.open.is_empty());
        assert_eq!(store.game.xp, 50, "high, fast: 25 x 2");
        assert!(store.cues.contains(&Cue::Resolved));
        assert!(store.game.has("first-blood"));
        assert!(store.toasts.iter().any(|t| t.kind == ToastKind::Badge));
        let v: serde_json::Value = serde_json::from_str(&store.state_json(&spec)).unwrap();
        assert_eq!(v["xp"], 50);
        assert_eq!(v["open"], 0);
    }
}
