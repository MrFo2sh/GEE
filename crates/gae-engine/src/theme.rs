//! Colors for both worlds: Bevy colors for the scene and egui colors for the UI, all derived
//! from the spec's `Palette` so an environment re-themes everything by changing one struct.

use bevy::prelude::*;
use bevy_egui::egui;
use gae_core::Severity;
use gae_core::spec::{Palette, Rgb};

pub fn color(c: Rgb) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

pub fn color32(c: Rgb) -> egui::Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(b(c[0]), b(c[1]), b(c[2]))
}

/// The UI half of the palette, ready for egui.
#[derive(Resource, Clone)]
pub struct UiTheme {
    pub accent: egui::Color32,
    pub text: egui::Color32,
    pub dim: egui::Color32,
    pub warn: egui::Color32,
    pub alert: egui::Color32,
    pub good: egui::Color32,
    pub panel: egui::Color32,
    pub card: egui::Color32,
    pub ink: egui::Color32,
}

impl UiTheme {
    pub fn from_palette(p: &Palette) -> UiTheme {
        let panel = color32(p.panel);
        let card = color32([p.panel[0] * 0.7, p.panel[1] * 0.7, p.panel[2] * 0.7]);
        UiTheme {
            accent: color32(p.accent),
            text: color32(p.text),
            dim: color32(p.dim),
            warn: color32(p.warn),
            alert: color32(p.alert),
            good: color32(p.active),
            panel: egui::Color32::from_rgba_unmultiplied(panel.r(), panel.g(), panel.b(), 232),
            card,
            ink: color32(p.background),
        }
    }

    pub fn severity(&self, s: Severity) -> egui::Color32 {
        match s {
            Severity::Critical => self.alert,
            Severity::High => egui::Color32::from_rgb(255, 120, 70),
            Severity::Medium => self.warn,
            Severity::Low => self.accent,
            Severity::Info => self.dim,
        }
    }
}

/// Idle seconds before the camera starts its slow cinematic orbit.
pub const IDLE_ORBIT_AFTER_S: f32 = 8.0;
pub const STAR_COUNT: usize = 900;
pub const DUST_COUNT: usize = 500;
/// How many queue rows and tree rows the sidebar draws before "show more".
pub const PAGE: usize = 100;
