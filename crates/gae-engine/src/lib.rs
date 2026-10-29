//! GAE engine: turns a `Spec` and some `Source`s into a running, gamified 3D environment.
//!
//! ```no_run
//! use gae_engine::{EngineConfig, run};
//! use gae_engine::core::{Spec, source::Replay};
//! let spec = Spec::new("My Desk", "my-desk");
//! run(spec, vec![Box::new(Replay::new("demo", Vec::new(), true))], EngineConfig::default());
//! ```
//!
//! One file per concern; none of them parse files (that is the sources' job, in gae-core).

mod camera;
pub mod cards;
mod scene;
pub mod settings;
mod sounds;
pub mod store;
pub mod theme;
mod ui;

pub use gae_core as core;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use bevy_panorbit_camera::PanOrbitCameraPlugin;
use gae_core::bridge::{self, BridgeConfig};
use gae_core::{Source, Spec};

#[derive(Default)]
pub struct EngineConfig {
    /// Ingest port on 127.0.0.1; `Some(None)` turns HTTP off. Default (`None`): the spec's port.
    pub http_port: Option<Option<u16>>,
}

impl EngineConfig {
    pub fn port(mut self, port: Option<u16>) -> EngineConfig {
        self.http_port = Some(port);
        self
    }
}

/// Start the bridge threads and run the app until the window closes.
pub fn run(spec: Spec, sources: Vec<Box<dyn Source>>, cfg: EngineConfig) {
    for problem in spec.validate() {
        eprintln!("gae: spec problem: {problem}");
    }
    let port = cfg.http_port.unwrap_or(Some(spec.port));
    let bridge = bridge::start(
        BridgeConfig {
            http_port: port,
            banner: spec.slug.to_string(),
        },
        sources,
    );
    let settings = settings::load(spec.slug);
    let ui_theme = theme::UiTheme::from_palette(&spec.palette);
    let store = store::Store::new(&spec);
    let title = spec.name.to_string();
    let background = theme::color(spec.palette.background);

    App::new()
        .insert_resource(ClearColor(background))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title,
                resolution: (1400, 860).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            MeshPickingPlugin,
            EguiPlugin::default(),
            PanOrbitCameraPlugin,
            FrameTimeDiagnosticsPlugin::default(),
        ))
        .insert_resource(settings)
        .insert_resource(ui_theme)
        .insert_resource(store)
        .insert_resource(store::SpecRes(spec))
        .insert_resource(store::BridgeRes(bridge))
        .init_resource::<scene::UnitEntities>()
        .init_resource::<scene::RingEntities>()
        .init_resource::<scene::SceneLayout>()
        .init_resource::<scene::UnitWorldPos>()
        .init_resource::<camera::CameraState>()
        .add_systems(Startup, (camera::setup, scene::setup, sounds::setup))
        .add_systems(
            Update,
            // Bevy system tuples hold 20 at most: keep groups small and chained.
            (
                (
                    store::drain_bridge,
                    scene::sync_units,
                    scene::animate_units,
                    scene::update_halos,
                    scene::animate_dust,
                    scene::draw_links,
                )
                    .chain(),
                (
                    camera::drive,
                    camera::apply_viewport,
                    camera::apply_lights,
                    scene::project_badges,
                    sounds::play,
                    settings::persist,
                    start_window,
                    toggle_fullscreen,
                    zoom_keys,
                    self_shot,
                )
                    .chain(),
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui::draw)
        .add_systems(Last, store::save_on_exit)
        .run();
}

/// Maximized (or full screen) on the first frames; a fixed size can hang off small displays.
fn start_window(
    settings: Res<settings::Settings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut done: Local<u32>,
) {
    if *done >= 3 {
        return;
    }
    *done += 1;
    for mut w in &mut windows {
        w.set_maximized(true);
        if settings.fullscreen {
            w.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        }
    }
}

/// F11 toggles borderless full screen on the monitor the window is on.
fn toggle_fullscreen(keys: Res<ButtonInput<KeyCode>>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    for mut w in &mut windows {
        w.mode = match w.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
}

/// Ctrl and + / - / 0: the card under the pointer zooms its text; elsewhere the whole UI scales.
fn zoom_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<settings::Settings>,
    mut store: ResMut<store::Store>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let step: i32 = if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        1
    } else if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        -1
    } else if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0) {
        0
    } else {
        return;
    };
    if let Some(id) = store.hot_card.clone() {
        store.zoom_request = Some((id, step));
        return;
    }
    if store.typing {
        return;
    }
    let next = if step == 0 {
        1.0
    } else {
        ((settings.ui_scale + 0.1 * step as f32) * 10.0).round() / 10.0
    };
    settings.ui_scale = next.clamp(0.6, 1.8);
}

/// GAE_SHOT=<png> photographs the window itself after GAE_SHOT_AT seconds (default 10): the
/// scene as rendered, whatever covers the window. How agents verify a change without a mouse.
fn self_shot(mut commands: Commands, time: Res<Time>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(path) = std::env::var("GAE_SHOT") else {
        *done = true;
        return;
    };
    let at: f32 = std::env::var("GAE_SHOT_AT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0);
    if time.elapsed_secs() < at {
        return;
    }
    *done = true;
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}
