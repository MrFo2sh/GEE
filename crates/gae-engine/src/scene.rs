//! The constellation: the core in the middle, units on orbit rings shaped and colored by kind,
//! halos for state, beams to parents with tracers while an action runs, dust, a night sky.
//! Units are keyed by id; `sync_units` keeps entities in step with the world each frame.

use super::settings::Settings;
use super::store::{Badge, SpecRes, Store};
use super::theme;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use gae_core::layout::{self, Layout};
use gae_core::model::{CORE, Severity, Status, now_millis};
use gae_core::spec::{Cue, Shape, Spec};
use std::collections::HashMap;
use std::f32::consts::TAU;

#[derive(Component)]
pub struct UnitNode {
    pub id: String,
}

/// Orbit parameters of a unit; the animated angle is derived from these each frame.
#[derive(Component)]
pub struct Orbit {
    pub depth: u32,
    pub base_angle: f32,
    pub size: f32,
    pub tilt: Quat,
}

/// A rotating ring around a unit. Unit halos show state; core halos are always on.
#[derive(Component)]
pub struct Halo {
    pub unit: String,
    pub tilt: f32,
    pub speed: f32,
    pub is_core: bool,
}

/// An orbit ring; `RingEntities` maps depth to it.
#[derive(Component)]
pub struct OrbitRing;

#[derive(Component)]
pub struct Dust {
    radius: f32,
    height: f32,
    phase: f32,
    speed: f32,
}

#[derive(Resource, Default)]
pub struct UnitEntities(pub HashMap<String, Entity>);

#[derive(Resource, Default)]
pub struct RingEntities(pub HashMap<u32, Entity>);

/// The layout last built and for which world version.
#[derive(Resource, Default)]
pub struct SceneLayout {
    pub layout: Layout,
    version: u64,
}

/// World position of every unit this frame.
#[derive(Resource, Default)]
pub struct UnitWorldPos(pub HashMap<String, Vec3>);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum MatKey {
    Core,
    Selected,
    Hover,
    Alert,
    Warn,
    Failed,
    Done,
    /// A kind's color by its index in the spec (`usize::MAX`: the fallback), lit or quiet.
    Kind(usize, bool),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum HaloKey {
    Selected,
    Hover,
    Active,
    Alert,
    Warn,
    Core,
}

#[derive(Resource)]
pub struct SceneAssets {
    shapes: HashMap<Shape, Handle<Mesh>>,
    core_mesh: Handle<Mesh>,
    halo_mesh: Handle<Mesh>,
    core_halo_mesh: Handle<Mesh>,
    materials: HashMap<MatKey, Handle<StandardMaterial>>,
    halos: HashMap<HaloKey, Handle<StandardMaterial>>,
    ring_material: Handle<StandardMaterial>,
}

/// Glossy and lit, with an emissive push for the glowing states.
fn plastic(base: Color, glow: f32) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * glow, lin.green * glow, lin.blue * glow, 1.0),
        perceptual_roughness: 0.35,
        metallic: 0.1,
        reflectance: 0.5,
        ..default()
    }
}

/// Pure light: unlit, emissive only. Bloom turns it into a glow. Strengths are nits: 40-220.
fn light(base: Color, strength: f32) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * strength, lin.green * strength, lin.blue * strength, 1.0),
        unlit: true,
        ..default()
    }
}

/// Tiny deterministic PRNG so the sky is identical every launch without a rand dependency.
struct XorShift(u32);
impl XorShift {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32) / (u32::MAX as f32)
    }
}

pub fn setup(
    mut commands: Commands,
    spec: Res<SpecRes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_store: ResMut<GizmoConfigStore>,
) {
    let spec = &spec.0;
    let p = &spec.palette;
    let (gizmo_cfg, _) = gizmo_store.config_mut::<DefaultGizmoConfigGroup>();
    gizmo_cfg.line.width = 2.0;

    let mut shapes = HashMap::new();
    shapes.insert(Shape::Sphere, meshes.add(Sphere::new(0.22).mesh().ico(3).unwrap()));
    shapes.insert(Shape::Cube, meshes.add(Cuboid::new(0.36, 0.36, 0.36)));
    shapes.insert(
        Shape::Capsule,
        meshes.add(Capsule3d {
            radius: 0.14,
            half_length: 0.15,
        }),
    );
    shapes.insert(
        Shape::Cylinder,
        meshes.add(Cylinder {
            radius: 0.2,
            half_height: 0.18,
        }),
    );
    shapes.insert(Shape::Diamond, meshes.add(Cuboid::new(0.32, 0.32, 0.32)));
    let core_mesh = meshes.add(Sphere::new(0.6).mesh().ico(4).unwrap());
    let halo_mesh = meshes.add(Torus {
        minor_radius: 0.022,
        major_radius: 0.46,
    });
    let core_halo_mesh = meshes.add(Torus {
        minor_radius: 0.03,
        major_radius: 1.15,
    });

    let mut mats = HashMap::new();
    mats.insert(MatKey::Core, materials.add(plastic(theme::color(p.core), 8.0)));
    mats.insert(MatKey::Selected, materials.add(plastic(theme::color(p.selected), 80.0)));
    mats.insert(MatKey::Hover, materials.add(plastic(theme::color(p.selected), 30.0)));
    mats.insert(MatKey::Alert, materials.add(plastic(theme::color(p.alert), 90.0)));
    mats.insert(MatKey::Warn, materials.add(plastic(theme::color(p.warn), 60.0)));
    mats.insert(MatKey::Failed, materials.add(plastic(theme::color(p.alert), 12.0)));
    mats.insert(MatKey::Done, materials.add(plastic(theme::color(p.done), 0.0)));
    for (i, k) in spec.kinds.iter().enumerate() {
        mats.insert(
            MatKey::Kind(i, true),
            materials.add(plastic(theme::color(k.color), 40.0)),
        );
        mats.insert(
            MatKey::Kind(i, false),
            materials.add(plastic(theme::color(k.color), 2.0)),
        );
    }
    let fallback = spec.kind("").color;
    mats.insert(
        MatKey::Kind(usize::MAX, true),
        materials.add(plastic(theme::color(fallback), 40.0)),
    );
    mats.insert(
        MatKey::Kind(usize::MAX, false),
        materials.add(plastic(theme::color(fallback), 2.0)),
    );

    let mut halos = HashMap::new();
    halos.insert(HaloKey::Selected, materials.add(light(theme::color(p.selected), 140.0)));
    halos.insert(HaloKey::Hover, materials.add(light(theme::color(p.selected), 90.0)));
    halos.insert(HaloKey::Active, materials.add(light(theme::color(p.active), 110.0)));
    halos.insert(HaloKey::Alert, materials.add(light(theme::color(p.alert), 160.0)));
    halos.insert(HaloKey::Warn, materials.add(light(theme::color(p.warn), 130.0)));
    halos.insert(HaloKey::Core, materials.add(light(theme::color(p.core), 110.0)));
    let ring_material = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Blend,
        ..light(Color::srgba(p.ring[0], p.ring[1], p.ring[2], 0.5), 25.0)
    });

    let star_mesh = meshes.add(Cuboid::new(0.14, 0.14, 0.14));
    let star_mat = materials.add(light(theme::color(p.star), 28.0));
    let mut rng = XorShift(0x9E37_79B9);
    for _ in 0..theme::STAR_COUNT {
        let u = rng.next_f32() * 2.0 - 1.0;
        let phi = rng.next_f32() * TAU;
        let r = 90.0 + rng.next_f32() * 60.0;
        let s = (1.0 - u * u).sqrt();
        let pos = Vec3::new(r * s * phi.cos(), r * u, r * s * phi.sin());
        let size = 0.6 + rng.next_f32() * 1.6;
        commands.spawn((
            Mesh3d(star_mesh.clone()),
            MeshMaterial3d(star_mat.clone()),
            Transform::from_translation(pos).with_scale(Vec3::splat(size)),
            Pickable::IGNORE,
        ));
    }
    let dust_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.05));
    let dust_mat = materials.add(light(theme::color(p.dust), 14.0));
    for _ in 0..theme::DUST_COUNT {
        let radius = 1.5 + rng.next_f32() * 14.0;
        let height = (rng.next_f32() - 0.5) * 2.0;
        let phase = rng.next_f32() * TAU;
        let speed = 0.012 + rng.next_f32() * 0.04;
        commands.spawn((
            Mesh3d(dust_mesh.clone()),
            MeshMaterial3d(dust_mat.clone()),
            Transform::from_translation(Vec3::new(radius * phase.cos(), height, radius * phase.sin())),
            Dust {
                radius,
                height,
                phase,
                speed,
            },
            Pickable::IGNORE,
        ));
    }

    commands.insert_resource(SceneAssets {
        shapes,
        core_mesh,
        halo_mesh,
        core_halo_mesh,
        materials: mats,
        halos,
        ring_material,
    });
}

fn kind_index(spec: &Spec, kind: &str) -> usize {
    spec.kinds.iter().position(|k| k.kind == kind).unwrap_or(usize::MAX)
}

/// The worst open severity per unit, once per frame (not once per unit per frame).
fn alerts(store: &Store) -> HashMap<&str, (Severity, usize)> {
    let mut out: HashMap<&str, (Severity, usize)> = HashMap::new();
    for s in &store.world.open {
        let e = out.entry(s.unit.as_str()).or_insert((s.severity, 0));
        e.0 = e.0.max(s.severity);
        e.1 += 1;
    }
    out
}

fn material_for(store: &Store, spec: &Spec, unit: &gae_core::model::Unit, alert: Option<Severity>, now: i64) -> MatKey {
    if store.selected.as_deref() == Some(unit.id.as_str()) {
        return MatKey::Selected;
    }
    if store.hovered.as_deref() == Some(unit.id.as_str()) {
        return MatKey::Hover;
    }
    if unit.id == CORE {
        return MatKey::Core;
    }
    match alert {
        Some(Severity::Critical | Severity::High) => return MatKey::Alert,
        Some(Severity::Medium) => return MatKey::Warn,
        _ => {}
    }
    if unit.last_failed() && unit.ended.is_none() {
        return MatKey::Failed;
    }
    let k = kind_index(spec, &unit.kind);
    match unit.status(now, spec.active_window_ms) {
        Status::Active => MatKey::Kind(k, true),
        Status::Idle => MatKey::Kind(k, false),
        Status::Done => MatKey::Done,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn sync_units(
    mut commands: Commands,
    store: Res<Store>,
    spec: Res<SpecRes>,
    assets: Res<SceneAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut entities: ResMut<UnitEntities>,
    mut rings: ResMut<RingEntities>,
    mut scene: ResMut<SceneLayout>,
    mut units: Query<(&UnitNode, &mut Orbit, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let spec = &spec.0;
    let now = now_millis();
    let dirty = scene.version != store.world.version;
    if dirty {
        scene.layout = layout::layout(&store.world);
        scene.version = store.world.version;

        // Units that left the world take their entities with them.
        entities.0.retain(|id, e| {
            let keep = store.world.unit(id).is_some();
            if !keep {
                commands.entity(*e).despawn();
            }
            keep
        });
        // One ring per depth, rebuilt when its radius changes (crowded rings grow).
        let max_depth = scene.layout.max_depth();
        rings.0.retain(|depth, e| {
            let keep = *depth <= max_depth;
            if !keep {
                commands.entity(*e).despawn();
            }
            keep
        });
        for depth in 1..=max_depth {
            let radius = scene.layout.radius(depth);
            if let Some(e) = rings.0.get(&depth).copied() {
                commands.entity(e).despawn();
            }
            let mesh = meshes.add(Torus {
                minor_radius: 0.012,
                major_radius: radius,
            });
            let e = commands
                .spawn((
                    OrbitRing,
                    Mesh3d(mesh),
                    MeshMaterial3d(assets.ring_material.clone()),
                    Transform::from_translation(Vec3::Y * layout::ring_height(depth)),
                    Pickable::IGNORE,
                ))
                .id();
            rings.0.insert(depth, e);
        }

        for n in &scene.layout.units {
            if entities.0.contains_key(&n.unit) {
                continue;
            }
            let Some(unit) = store.world.unit(&n.unit) else {
                continue;
            };
            let is_core = n.depth == 0;
            let style = spec.kind(&unit.kind);
            let (mesh, size, tilt) = if is_core {
                (assets.core_mesh.clone(), 1.0, Quat::IDENTITY)
            } else {
                let tilt = if style.shape == Shape::Diamond {
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_4)
                        * Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)
                } else {
                    Quat::IDENTITY
                };
                (assets.shapes[&style.shape].clone(), style.size.max(0.2), tilt)
            };
            // New units deploy from the core and travel out to their ring.
            let mut ec = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(assets.materials[&MatKey::Kind(kind_index(spec, &unit.kind), false)].clone()),
                Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(0.01)),
                UnitNode { id: n.unit.clone() },
                Orbit {
                    depth: n.depth,
                    base_angle: n.angle,
                    size,
                    tilt,
                },
            ));
            ec.observe(on_click).observe(on_over).observe(on_out);
            let id = n.unit.clone();
            ec.with_children(|p| {
                if is_core {
                    for (tilt, speed) in [(0.55_f32, 0.45_f32), (-0.55, -0.35)] {
                        p.spawn((
                            Mesh3d(assets.core_halo_mesh.clone()),
                            MeshMaterial3d(assets.halos[&HaloKey::Core].clone()),
                            Transform::from_rotation(Quat::from_rotation_x(tilt)),
                            Halo {
                                unit: id.clone(),
                                tilt,
                                speed,
                                is_core: true,
                            },
                            Pickable::IGNORE,
                        ));
                    }
                } else {
                    p.spawn((
                        Mesh3d(assets.halo_mesh.clone()),
                        MeshMaterial3d(assets.halos[&HaloKey::Active].clone()),
                        Transform::from_rotation(Quat::from_rotation_x(1.2)),
                        Visibility::Hidden,
                        Halo {
                            unit: id.clone(),
                            tilt: 1.2,
                            speed: 1.6,
                            is_core: false,
                        },
                        Pickable::IGNORE,
                    ));
                }
            });
            entities.0.insert(n.unit.clone(), ec.id());
        }
    }

    let alerts = alerts(&store);
    for (node, mut orbit, mut mat) in units.iter_mut() {
        if dirty && let Some(n) = scene.layout.get(&node.id) {
            orbit.depth = n.depth;
            orbit.base_angle = n.angle;
        }
        if let Some(unit) = store.world.unit(&node.id) {
            let alert = alerts.get(unit.id.as_str()).map(|a| a.0);
            let handle = &assets.materials[&material_for(&store, spec, unit, alert, now)];
            if mat.0 != *handle {
                mat.0 = handle.clone();
            }
        }
    }
}

fn orbit_angle(base: f32, depth: u32, t: f32, speed: f32) -> f32 {
    if depth == 0 {
        return 0.0;
    }
    base + t * 0.06 * speed / depth as f32
}

/// Move units toward their orbit position, bob the busy ones, publish world positions.
pub fn animate_units(
    time: Res<Time>,
    store: Res<Store>,
    spec: Res<SpecRes>,
    settings: Res<Settings>,
    scene: Res<SceneLayout>,
    mut world_pos: ResMut<UnitWorldPos>,
    mut units: Query<(&UnitNode, &Orbit, &mut Transform)>,
) {
    let now = now_millis();
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    world_pos.0.clear();
    for (node, orbit, mut tf) in units.iter_mut() {
        let angle = orbit_angle(orbit.base_angle, orbit.depth, t, settings.drift_speed);
        let mut target = Vec3::from(scene.layout.orbit_point(orbit.depth, angle));
        let busy = store
            .world
            .unit(&node.id)
            .map(|u| u.status(now, spec.0.active_window_ms) == Status::Active && u.has_open_action())
            .unwrap_or(false);
        if busy {
            target.y += 0.12 * (t * 7.0 + orbit.base_angle).sin().abs();
        }
        tf.translation = tf.translation.lerp(target, (dt * 4.0).min(1.0));
        let s = tf.scale.x + (orbit.size - tf.scale.x) * (dt * 5.0).min(1.0);
        tf.scale = Vec3::splat(s);
        let facing = if orbit.depth == 0 {
            t * 0.2
        } else {
            -angle + std::f32::consts::FRAC_PI_2
        };
        tf.rotation = Quat::from_rotation_y(facing) * orbit.tilt;
        world_pos.0.insert(node.id.clone(), tf.translation);
    }
}

pub fn update_halos(
    time: Res<Time>,
    store: Res<Store>,
    spec: Res<SpecRes>,
    assets: Res<SceneAssets>,
    parents: Query<&Transform, Without<Halo>>,
    mut halos: Query<(
        &Halo,
        &ChildOf,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let t = time.elapsed_secs();
    let now = now_millis();
    let alerts = alerts(&store);
    for (halo, child_of, mut tf, mut vis, mut mat) in halos.iter_mut() {
        let parent_rot = parents
            .get(child_of.parent())
            .map(|p| p.rotation)
            .unwrap_or(Quat::IDENTITY);
        tf.rotation = parent_rot.inverse() * Quat::from_rotation_y(t * halo.speed) * Quat::from_rotation_x(halo.tilt);
        if halo.is_core {
            continue;
        }
        let unit = store.world.unit(&halo.unit);
        let alert = alerts.get(halo.unit.as_str()).map(|a| a.0);
        let key = if store.selected.as_deref() == Some(halo.unit.as_str()) {
            Some(HaloKey::Selected)
        } else if store.hovered.as_deref() == Some(halo.unit.as_str()) {
            Some(HaloKey::Hover)
        } else if matches!(alert, Some(Severity::Critical | Severity::High)) {
            Some(HaloKey::Alert)
        } else if alert == Some(Severity::Medium) {
            Some(HaloKey::Warn)
        } else if unit.is_some_and(|u| u.status(now, spec.0.active_window_ms) == Status::Active && u.has_open_action())
        {
            Some(HaloKey::Active)
        } else {
            None
        };
        // Alerts breathe so they read at a glance.
        let pulse = if matches!(key, Some(HaloKey::Alert | HaloKey::Warn)) {
            1.0 + 0.25 * (t * 5.0).sin().abs()
        } else {
            1.0
        };
        tf.scale = Vec3::splat(pulse);
        match key {
            Some(k) => {
                let handle = &assets.halos[&k];
                if mat.0 != *handle {
                    mat.0 = handle.clone();
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

pub fn animate_dust(time: Res<Time>, settings: Res<Settings>, mut dust: Query<(&Dust, &mut Transform)>) {
    let t = time.elapsed_secs() * settings.drift_speed.max(0.2);
    for (d, mut tf) in dust.iter_mut() {
        let a = d.phase + t * d.speed;
        tf.translation = Vec3::new(
            d.radius * a.cos(),
            d.height + 0.15 * (t * 0.7 + d.phase).sin(),
            d.radius * a.sin(),
        );
    }
}

/// Beams from each unit to its parent, and tracer rounds down the beams of busy units.
pub fn draw_links(
    mut gizmos: Gizmos,
    time: Res<Time>,
    store: Res<Store>,
    spec: Res<SpecRes>,
    settings: Res<Settings>,
    world_pos: Res<UnitWorldPos>,
) {
    if !settings.links {
        return;
    }
    let p = &spec.0.palette;
    let beam = Color::srgba(p.beam[0], p.beam[1], p.beam[2], 0.30);
    let beam_active = Color::srgba(p.active[0], p.active[1], p.active[2], 0.9);
    let beam_done = Color::srgba(p.done[0], p.done[1], p.done[2], 0.25);
    let pulse = Color::srgb(p.pulse[0], p.pulse[1], p.pulse[2]);
    let t = time.elapsed_secs();
    let now = now_millis();
    for unit in &store.world.units {
        let Some(parent) = unit.parent.as_deref() else { continue };
        let (Some(&a), Some(&b)) = (world_pos.0.get(parent), world_pos.0.get(&unit.id)) else {
            continue;
        };
        let status = unit.status(now, spec.0.active_window_ms);
        let color = match status {
            Status::Active => beam_active,
            Status::Idle => beam,
            Status::Done => beam_done,
        };
        let v: Vec<Vec3> = layout::curve(a.into(), b.into(), 1.0, 16)
            .into_iter()
            .map(Vec3::from)
            .collect();
        gizmos.linestrip(v.clone(), color);
        if status == Status::Active && unit.has_open_action() {
            let n = v.len();
            for k in 0..2 {
                let phase = (t * 0.9 + unit.order as f32 * 0.37 + k as f32 * 0.5).fract();
                let head = ((phase * (n - 1) as f32) as usize).min(n - 1);
                let tail = head.saturating_sub(3);
                if head > tail {
                    gizmos.linestrip(v[tail..=head].to_vec(), pulse);
                    gizmos.sphere(Isometry3d::from_translation(v[head]), 0.07, pulse);
                }
            }
        }
    }
}

/// Signal badges over units and labels for the selected and hovered ones, in window coordinates.
pub fn project_badges(
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    cams: Query<(&Camera, &GlobalTransform), With<bevy_panorbit_camera::PanOrbitCamera>>,
    units: Query<(&UnitNode, &GlobalTransform)>,
) {
    let Ok((cam, cam_tf)) = cams.single() else { return };
    let zoom = settings.ui_scale.max(0.1);
    let mut badges = Vec::new();
    let mut labels = Vec::new();
    {
        let alerts = alerts(&store);
        for (node, tf) in &units {
            let scale = tf.scale().x.max(0.05);
            let head = tf.translation() + Vec3::Y * scale * if node.id == CORE { 1.0 } else { 0.5 };
            let Ok(p) = cam.world_to_viewport(cam_tf, head) else {
                continue;
            };
            if let Some((severity, count)) = alerts.get(node.id.as_str()) {
                badges.push(Badge {
                    unit: node.id.clone(),
                    severity: *severity,
                    count: *count,
                    x: p.x / zoom,
                    y: p.y / zoom,
                });
            }
            let named = node.id == CORE
                || store.selected.as_deref() == Some(node.id.as_str())
                || store.hovered.as_deref() == Some(node.id.as_str());
            if named && let Some(u) = store.world.unit(&node.id) {
                labels.push((u.title().to_string(), p.x / zoom, p.y / zoom));
            }
        }
    }
    store.badges = badges;
    store.labels = labels;
}

fn on_click(ev: On<PointerClick>, units: Query<&UnitNode>, mut store: ResMut<Store>) {
    // Cinematic mode is for watching: nothing in the scene reacts to a click.
    if store.hide_ui {
        return;
    }
    if let Ok(n) = units.get(ev.entity) {
        if n.id == CORE && store.selected.as_deref() != Some(CORE) {
            store.play(Cue::Select);
        }
        store.selected = Some(n.id.clone());
        store.timeline_shown = 50;
    }
}

fn on_over(ev: On<PointerOver>, units: Query<&UnitNode>, mut store: ResMut<Store>) {
    if let Ok(n) = units.get(ev.entity) {
        store.hovered = Some(n.id.clone());
    }
}

fn on_out(ev: On<PointerOut>, units: Query<&UnitNode>, mut store: ResMut<Store>) {
    if let Ok(n) = units.get(ev.entity)
        && store.hovered.as_deref() == Some(n.id.as_str())
    {
        store.hovered = None;
    }
}
