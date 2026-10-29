//! Orbital layout: a pure function from a world to unit positions.
//!
//! The core sits at the origin. Each depth is a ring around it; children sit near their parent's
//! angle so a parent's cluster reads as one constellation. A ring grows with its population so
//! a thousand hosts never sit on top of each other. Links are quadratic curves lifted off the plane.

use crate::model::World;
use std::collections::HashMap;
use std::f32::consts::TAU;

pub type Vec3 = [f32; 3];

/// Minimum distance between rings.
pub const RING_STEP: f32 = 3.6;
/// Vertical rise per depth so rings read as layers, not one flat disc.
pub const RING_RISE: f32 = 0.55;
/// Arc length each unit needs on its ring before the ring grows.
pub const UNIT_SPACING: f32 = 0.9;

#[derive(Debug, Clone, PartialEq)]
pub struct UnitPos {
    pub unit: String,
    pub pos: Vec3,
    pub depth: u32,
    /// Orbit angle in radians (0 for the core).
    pub angle: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub units: Vec<UnitPos>,
    /// Ring radius by depth; index 0 is the core (0.0).
    pub radii: Vec<f32>,
    index: HashMap<String, usize>,
}

impl Layout {
    pub fn get(&self, unit: &str) -> Option<&UnitPos> {
        self.index.get(unit).map(|&i| &self.units[i])
    }

    pub fn max_depth(&self) -> u32 {
        self.radii.len().saturating_sub(1) as u32
    }

    pub fn radius(&self, depth: u32) -> f32 {
        self.radii.get(depth as usize).copied().unwrap_or(0.0)
    }

    /// Position on ring `depth` at `angle` (used to animate the drift).
    pub fn orbit_point(&self, depth: u32, angle: f32) -> Vec3 {
        let r = self.radius(depth);
        [r * angle.cos(), ring_height(depth), r * angle.sin()]
    }

    /// The outermost ring, for framing the camera.
    pub fn outer_radius(&self) -> f32 {
        self.radii.last().copied().unwrap_or(0.0)
    }
}

pub fn ring_height(depth: u32) -> f32 {
    RING_RISE * depth as f32
}

/// Radius per depth: at least one step out from the previous ring, and wide enough for its units.
pub fn radii(counts: &[usize]) -> Vec<f32> {
    let mut out = Vec::with_capacity(counts.len());
    let mut prev = 0.0_f32;
    for (d, &n) in counts.iter().enumerate() {
        if d == 0 {
            out.push(0.0);
            continue;
        }
        let need = n as f32 * UNIT_SPACING / TAU;
        let r = (prev + RING_STEP).max(need);
        out.push(r);
        prev = r;
    }
    out
}

pub fn layout(world: &World) -> Layout {
    let max_depth = world.max_depth();
    let mut counts = vec![0usize; max_depth as usize + 1];
    for u in &world.units {
        counts[u.depth as usize] += 1;
    }
    let radii = radii(&counts);
    let mut out = Layout {
        units: Vec::with_capacity(world.units.len()),
        radii,
        index: HashMap::new(),
    };
    let mut angle_of: HashMap<&str, f32> = HashMap::new();
    for u in world.units.iter().filter(|u| u.depth == 0) {
        out.units.push(UnitPos {
            unit: u.id.clone(),
            pos: [0.0, 0.0, 0.0],
            depth: 0,
            angle: 0.0,
        });
        angle_of.insert(&u.id, 0.0);
    }
    for depth in 1..=max_depth {
        let mut ring: Vec<&crate::model::Unit> = world.units.iter().filter(|u| u.depth == depth).collect();
        if ring.is_empty() {
            continue;
        }
        let parent_angle = |u: &crate::model::Unit, angle_of: &HashMap<&str, f32>| {
            u.parent
                .as_deref()
                .and_then(|p| angle_of.get(p))
                .copied()
                .unwrap_or(0.0)
        };
        // By parent angle, then arrival, so siblings sit together under their parent.
        ring.sort_by(|a, b| {
            parent_angle(a, &angle_of)
                .partial_cmp(&parent_angle(b, &angle_of))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.order.cmp(&b.order))
        });
        let n = ring.len() as f32;
        let step = TAU / n;
        let first_parent = parent_angle(ring[0], &angle_of);
        let first_group = ring.iter().take_while(|u| u.parent == ring[0].parent).count() as f32;
        let start = first_parent - step * (first_group - 1.0) / 2.0;
        let r = out.radius(depth);
        for (i, u) in ring.iter().enumerate() {
            let angle = start + step * i as f32;
            angle_of.insert(&u.id, angle);
            out.units.push(UnitPos {
                unit: u.id.clone(),
                pos: [r * angle.cos(), ring_height(depth), r * angle.sin()],
                depth,
                angle,
            });
        }
    }
    out.index = out.units.iter().enumerate().map(|(i, p)| (p.unit.clone(), i)).collect();
    out
}

/// Points along a quadratic curve from `a` to `b`, lifted by `lift` at the middle.
pub fn curve(a: Vec3, b: Vec3, lift: f32, segments: usize) -> Vec<Vec3> {
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5 + lift, (a[2] + b[2]) * 0.5];
    (0..=segments)
        .map(|s| {
            let t = s as f32 / segments as f32;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * mid[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * mid[1] + t * t * b[1],
                u * u * a[2] + 2.0 * u * t * mid[2] + t * t * b[2],
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CORE, Event};
    use std::collections::BTreeMap;

    fn up(w: &mut World, id: &str, parent: Option<&str>) {
        w.apply(Event::UnitUp {
            unit: id.into(),
            parent: parent.map(String::from),
            kind: String::new(),
            label: String::new(),
            meta: BTreeMap::new(),
            at: 1,
        });
    }

    #[test]
    fn core_at_origin_children_on_rings_near_parent() {
        let mut w = World::new("c");
        for i in 0..4 {
            up(&mut w, &format!("a{i}"), None);
        }
        up(&mut w, "b0", Some("a1"));
        let l = layout(&w);
        assert_eq!(l.get(CORE).unwrap().pos, [0.0, 0.0, 0.0]);
        assert_eq!(l.max_depth(), 2);
        for p in l.units.iter().filter(|p| p.depth == 1) {
            let r = (p.pos[0].powi(2) + p.pos[2].powi(2)).sqrt();
            assert!((r - RING_STEP).abs() < 1e-3);
        }
        let d = (l.get("b0").unwrap().angle - l.get("a1").unwrap().angle).rem_euclid(TAU);
        assert!(d < 1e-3 || TAU - d < 1e-3);
    }

    #[test]
    fn crowded_rings_grow_and_stay_ordered() {
        let r = radii(&[1, 4, 2_000, 3]);
        assert_eq!(r[1], RING_STEP);
        assert!(r[2] * TAU / 2_000.0 >= UNIT_SPACING - 1e-3);
        assert!(r[3] >= r[2] + RING_STEP - 1e-3);
    }

    #[test]
    fn curve_hits_both_endpoints_and_lifts_middle() {
        let pts = curve([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], 1.0, 8);
        assert_eq!(pts.len(), 9);
        assert_eq!(pts[8], [4.0, 0.0, 0.0]);
        assert!((pts[4][1] - 0.5).abs() < 1e-5);
    }
}
