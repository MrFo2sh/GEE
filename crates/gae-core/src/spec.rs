//! How a domain describes itself to the engine. Plain data (colors are sRGB triples) so a spec
//! is testable without a renderer. An environment is a `Spec` plus one or more `Source`s.

use crate::game::{Achievement, XpRules, default_achievements};

pub type Rgb = [f32; 3];

/// What a unit looks like in the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Sphere,
    Cube,
    Capsule,
    Cylinder,
    /// A cube on its corner.
    Diamond,
}

#[derive(Clone, Debug)]
pub struct KindStyle {
    /// Matches `Event::UnitUp.kind`.
    pub kind: &'static str,
    /// Plural, for the legend and the tree ("hosts").
    pub label: &'static str,
    pub color: Rgb,
    pub shape: Shape,
    /// Scale of the mesh; 1.0 is a standard unit.
    pub size: f32,
}

impl KindStyle {
    pub const fn new(kind: &'static str, label: &'static str, color: Rgb, shape: Shape) -> KindStyle {
        KindStyle {
            kind,
            label,
            color,
            shape,
            size: 1.0,
        }
    }

    pub const fn size(mut self, size: f32) -> KindStyle {
        self.size = size;
        self
    }
}

/// Every color the engine uses. Defaults: a cool night-ops look, distinct from agent-globe's army green.
#[derive(Clone, Debug)]
pub struct Palette {
    pub background: Rgb,
    pub panel: Rgb,
    pub accent: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    pub core: Rgb,
    pub active: Rgb,
    pub idle: Rgb,
    pub done: Rgb,
    pub warn: Rgb,
    pub alert: Rgb,
    pub selected: Rgb,
    pub ring: Rgb,
    pub beam: Rgb,
    pub pulse: Rgb,
    pub dust: Rgb,
    pub star: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            background: [0.012, 0.020, 0.040],
            panel: [0.030, 0.050, 0.085],
            accent: [0.35, 0.85, 1.0],
            text: [0.86, 0.91, 0.96],
            dim: [0.45, 0.55, 0.66],
            core: [0.55, 0.80, 1.0],
            active: [0.45, 1.0, 0.85],
            idle: [0.30, 0.50, 0.70],
            done: [0.18, 0.24, 0.32],
            warn: [1.0, 0.70, 0.20],
            alert: [1.0, 0.28, 0.30],
            selected: [0.95, 0.98, 1.0],
            ring: [0.35, 0.60, 0.85],
            beam: [0.40, 0.70, 1.0],
            pulse: [1.6, 2.6, 3.0],
            dust: [0.45, 0.65, 0.95],
            star: [0.80, 0.86, 0.95],
        }
    }
}

/// The words the UI uses, so a SOC says "hosts" and "alerts" and a dev desk says "repos" and "failures".
#[derive(Clone, Debug)]
pub struct Nouns {
    pub core: &'static str,
    pub unit: &'static str,
    pub units: &'static str,
    pub signal: &'static str,
    pub signals: &'static str,
    pub action: &'static str,
    pub actions: &'static str,
    /// The verbs on the two queue buttons.
    pub resolve: &'static str,
    pub dismiss: &'static str,
}

impl Default for Nouns {
    fn default() -> Self {
        Nouns {
            core: "core",
            unit: "unit",
            units: "units",
            signal: "signal",
            signals: "signals",
            action: "action",
            actions: "actions",
            resolve: "resolve",
            dismiss: "dismiss",
        }
    }
}

/// Moments the radio has a cue for. A spec can replace any clip with its own bytes, and the user
/// can replace any of them with `<config dir>/gae/<slug>/sounds/<name>.wav`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cue {
    UnitAdded,
    Signal,
    Critical,
    Resolved,
    Dismissed,
    SourceCleared,
    Failure,
    Stale,
    LevelUp,
    Badge,
    Select,
    Blip,
}

impl Cue {
    pub const ALL: [Cue; 12] = [
        Cue::UnitAdded,
        Cue::Signal,
        Cue::Critical,
        Cue::Resolved,
        Cue::Dismissed,
        Cue::SourceCleared,
        Cue::Failure,
        Cue::Stale,
        Cue::LevelUp,
        Cue::Badge,
        Cue::Select,
        Cue::Blip,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cue::UnitAdded => "unit",
            Cue::Signal => "signal",
            Cue::Critical => "critical",
            Cue::Resolved => "resolved",
            Cue::Dismissed => "dismissed",
            Cue::SourceCleared => "cleared",
            Cue::Failure => "failure",
            Cue::Stale => "stale",
            Cue::LevelUp => "levelup",
            Cue::Badge => "badge",
            Cue::Select => "select",
            Cue::Blip => "blip",
        }
    }

    pub fn when(self) -> &'static str {
        match self {
            Cue::UnitAdded => "a new unit appeared",
            Cue::Signal => "a medium or high signal arrived",
            Cue::Critical => "a critical signal arrived",
            Cue::Resolved => "you resolved a signal",
            Cue::Dismissed => "you dismissed a signal",
            Cue::SourceCleared => "the source cleared a signal",
            Cue::Failure => "an action failed",
            Cue::Stale => "a signal went stale; the streak broke",
            Cue::LevelUp => "you levelled up",
            Cue::Badge => "you earned an achievement",
            Cue::Select => "you clicked the core",
            Cue::Blip => "every action (optional)",
        }
    }

    /// Minimum seconds between two plays of the same cue.
    pub fn cooldown(self) -> f32 {
        match self {
            Cue::Blip => 0.15,
            Cue::UnitAdded => 6.0,
            Cue::Signal => 3.0,
            Cue::Critical => 2.0,
            Cue::Failure => 3.0,
            Cue::SourceCleared => 4.0,
            Cue::Select => 0.8,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    /// Shown in the HUD and the window title ("SOC Desk").
    pub name: &'static str,
    /// Config folder and the HTTP banner ("soc-desk"). Lowercase, dashes.
    pub slug: &'static str,
    pub tagline: &'static str,
    pub nouns: Nouns,
    pub kinds: Vec<KindStyle>,
    pub palette: Palette,
    pub xp: XpRules,
    pub achievements: Vec<Achievement>,
    /// Embedded clips that replace the engine's defaults.
    pub sounds: Vec<(Cue, &'static [u8])>,
    /// Default ingest port on 127.0.0.1.
    pub port: u16,
    /// A unit counts as active if it did something this recently.
    pub active_window_ms: i64,
}

static FALLBACK: KindStyle = KindStyle {
    kind: "",
    label: "other",
    color: [0.55, 0.65, 0.80],
    shape: Shape::Sphere,
    size: 1.0,
};

impl Spec {
    pub fn new(name: &'static str, slug: &'static str) -> Spec {
        Spec {
            name,
            slug,
            tagline: "",
            nouns: Nouns::default(),
            kinds: Vec::new(),
            palette: Palette::default(),
            xp: XpRules::default(),
            achievements: default_achievements(),
            sounds: Vec::new(),
            port: 4790,
            active_window_ms: 20_000,
        }
    }

    pub fn kind(&self, kind: &str) -> &KindStyle {
        self.kinds.iter().find(|k| k.kind == kind).unwrap_or(&FALLBACK)
    }

    /// Problems an environment author should fix before shipping. Empty means fine.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("name is empty".into());
        }
        if self.slug.is_empty()
            || !self
                .slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            out.push(format!(
                "slug '{}' must be lowercase letters, digits and dashes",
                self.slug
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for k in &self.kinds {
            if !seen.insert(k.kind) {
                out.push(format!("kind '{}' is declared twice", k.kind));
            }
        }
        let mut ids = std::collections::HashSet::new();
        for a in &self.achievements {
            if !ids.insert(a.id) {
                out.push(format!("achievement '{}' is declared twice", a.id));
            }
        }
        if self.port < 1024 {
            out.push(format!("port {} is privileged; use 1024 or above", self.port));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_kinds_fall_back_and_validation_catches_mistakes() {
        let mut s = Spec::new("Desk", "desk");
        s.kinds
            .push(KindStyle::new("host", "hosts", [1.0, 0.0, 0.0], Shape::Cube));
        assert_eq!(s.kind("host").shape, Shape::Cube);
        assert_eq!(s.kind("nope").label, "other");
        assert!(s.validate().is_empty());
        s.kinds
            .push(KindStyle::new("host", "hosts", [1.0, 0.0, 0.0], Shape::Cube));
        s.slug = "Bad Slug";
        s.port = 80;
        assert_eq!(s.validate().len(), 3);
    }

    #[test]
    fn cue_names_are_unique() {
        let mut names: Vec<_> = Cue::ALL.iter().map(|c| c.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Cue::ALL.len());
    }
}
