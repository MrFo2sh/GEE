//! HDR orbit camera (MSAA, bloom, distance fog) rendering into the area the UI leaves free, a
//! separate camera for the UI layer, fly-to-selection, idle cinematic orbit, R to reset, F for
//! cinematic mode, Esc to step back. Disabled while the pointer is over a panel.

use super::scene::{SceneLayout, UnitWorldPos};
use super::settings::Settings;
use super::store::{SpecRes, Store};
use super::theme;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr, Viewport};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiGlobalSettings, PrimaryEguiContext};
use bevy_panorbit_camera::PanOrbitCamera;
use gae_core::model::CORE;

pub const CAMERA_HOME: Vec3 = Vec3::new(0.0, 9.0, 20.0);
const FOCUS_RADIUS: f32 = 6.5;
const FOCUS_RADIUS_CORE: f32 = 11.0;

#[derive(Resource, Default)]
pub struct CameraState {
    last_input_s: f32,
    last_selected: Option<String>,
}

#[derive(Component)]
pub struct KeyLight;

pub fn setup(
    mut commands: Commands,
    settings: Res<Settings>,
    spec: Res<SpecRes>,
    mut egui_settings: ResMut<EguiGlobalSettings>,
) {
    // The UI gets its own camera so clipping the 3D camera to the free area never clips the UI.
    egui_settings.auto_create_primary_context = false;
    let p = &spec.0.palette;

    commands.spawn((
        Camera3d::default(),
        Hdr,
        Msaa::Sample4,
        Bloom {
            intensity: settings.bloom,
            ..Bloom::NATURAL
        },
        DistanceFog {
            color: theme::color(p.background),
            falloff: FogFalloff::Linear {
                start: 45.0,
                end: 170.0,
            },
            ..default()
        },
        Transform::from_translation(CAMERA_HOME).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera {
            focus: Vec3::ZERO,
            zoom_lower_limit: 2.5,
            zoom_upper_limit: Some(220.0),
            orbit_smoothness: 0.12,
            zoom_smoothness: 0.2,
            pan_smoothness: 0.15,
            ..default()
        },
    ));

    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));

    commands.spawn((
        KeyLight,
        DirectionalLight {
            color: Color::srgb(1.0, 0.97, 0.92),
            illuminance: settings.key_light,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.6, 0.75, 1.0),
            illuminance: settings.fill_light,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-10.0, 6.0, -8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.70, 0.78, 0.90),
        brightness: settings.ambient_light,
        ..default()
    });
}

/// Light settings reach the scene the frame they change.
pub fn apply_lights(
    settings: Res<Settings>,
    mut lights: Query<(&mut DirectionalLight, Has<KeyLight>)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut fog: Query<&mut DistanceFog>,
) {
    if !settings.is_changed() {
        return;
    }
    for (mut l, is_key) in &mut lights {
        l.illuminance = if is_key {
            settings.key_light
        } else {
            settings.fill_light
        };
    }
    ambient.brightness = settings.ambient_light;
    let k = settings.fog.max(0.05);
    for mut f in &mut fog {
        f.falloff = FogFalloff::Linear {
            start: 45.0 / k,
            end: 170.0 / k,
        };
    }
}

fn go_home(cam: &mut PanOrbitCamera, outer: f32) {
    cam.target_focus = Vec3::ZERO;
    cam.target_radius = CAMERA_HOME.length().max(outer * 1.9);
    cam.target_yaw = 0.0;
    cam.target_pitch = (CAMERA_HOME.y / CAMERA_HOME.length()).asin();
    cam.force_update = true;
}

#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    world_pos: Res<UnitWorldPos>,
    layout: Res<SceneLayout>,
    mut state: ResMut<CameraState>,
    mut cams: Query<(&mut PanOrbitCamera, &mut Bloom)>,
) {
    let now = time.elapsed_secs();
    if buttons.get_pressed().next().is_some() || scroll.delta != Vec2::ZERO || keys.get_just_pressed().next().is_some()
    {
        state.last_input_s = now;
    }
    let Ok((mut cam, mut bloom)) = cams.single_mut() else {
        return;
    };
    cam.orbit_sensitivity = settings.orbit_sensitivity;
    cam.zoom_sensitivity = settings.zoom_sensitivity;
    cam.pan_sensitivity = settings.pan_sensitivity;
    if (bloom.intensity - settings.bloom).abs() > 1e-4 {
        bloom.intensity = settings.bloom;
    }
    cam.enabled = !store.pointer_over_ui;
    let outer = layout.layout.outer_radius();

    if (keys.just_pressed(KeyCode::KeyR) && !store.typing) || store.reset_view {
        store.reset_view = false;
        go_home(&mut cam, outer);
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) && !store.typing {
        store.hide_ui = !store.hide_ui;
    }
    if keys.just_pressed(KeyCode::Escape) && !store.typing {
        if store.show_trophies {
            store.show_trophies = false;
        } else if store.show_settings {
            store.show_settings = false;
        } else if store.hide_ui {
            store.hide_ui = false;
        } else {
            store.selected = None;
        }
    }

    if store.selected != state.last_selected {
        state.last_selected = store.selected.clone();
        match store
            .selected
            .as_deref()
            .and_then(|id| world_pos.0.get(id).map(|p| (id, *p)))
        {
            Some((id, p)) => {
                cam.target_focus = p;
                cam.target_radius = if id == CORE { FOCUS_RADIUS_CORE } else { FOCUS_RADIUS };
                cam.force_update = true;
            }
            None => go_home(&mut cam, outer),
        }
        return;
    }

    let idle = now - state.last_input_s > theme::IDLE_ORBIT_AFTER_S;
    if settings.idle_orbit && (idle || store.hide_ui) && store.selected.is_none() && !store.pointer_over_ui {
        cam.target_yaw += time.delta_secs() * 0.07;
        cam.force_update = true;
    }
}

/// Render the 3D scene only into the area between the panels, so the camera's centre (and every
/// fly-to target) lands in the visible middle rather than behind a panel.
pub fn apply_viewport(
    store: Res<Store>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<&mut Camera, With<PanOrbitCamera>>,
) {
    let (Some((left, top, right, bottom)), Ok(window)) = (store.scene_insets, windows.single()) else {
        return;
    };
    let scale = window.scale_factor();
    let (pw, ph) = (window.physical_width(), window.physical_height());
    if pw == 0 || ph == 0 {
        return;
    }
    let l = ((left * scale) as u32).min(pw - 1);
    let t = ((top * scale) as u32).min(ph - 1);
    let r = (right * scale) as u32;
    let b = (bottom * scale) as u32;
    let w = pw.saturating_sub(l + r).max(1);
    let h = ph.saturating_sub(t + b).max(1);
    for mut cam in &mut cams {
        let wanted = Viewport {
            physical_position: UVec2::new(l, t),
            physical_size: UVec2::new(w, h),
            ..default()
        };
        let same = cam.viewport.as_ref().is_some_and(|v| {
            v.physical_position == wanted.physical_position && v.physical_size == wanted.physical_size
        });
        if !same {
            cam.viewport = Some(wanted);
        }
    }
}
