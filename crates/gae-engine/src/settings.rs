//! User settings: camera feel, look, sound, docked cards. JSON in `<config dir>/gae/<slug>/`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Resource, Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub orbit_sensitivity: f32,
    pub zoom_sensitivity: f32,
    pub pan_sensitivity: f32,
    pub idle_orbit: bool,
    pub drift_speed: f32,
    pub bloom: f32,
    /// Key and fill light in lux, ambient in nits, fog as a multiplier.
    pub key_light: f32,
    pub fill_light: f32,
    pub ambient_light: f32,
    pub fog: f32,
    pub ui_scale: f32,
    pub sound: bool,
    /// 0.0 .. 1.0
    pub volume: f32,
    /// A short blip on every action (busy environments make this chatty).
    pub blips: bool,
    /// Output device name; empty = system default.
    pub audio_device: String,
    /// Lines from each unit to its parent, with tracers while an action runs.
    pub links: bool,
    /// Full screen at start (F11 toggles either way).
    pub fullscreen: bool,
    /// Dock grid rows; columns follow the monitors.
    pub grid_rows: u8,
    /// Where each card is docked, by card id. Floating cards are absent.
    pub panes: HashMap<String, Pane>,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// A docked card's rectangle as fractions of the window, so it survives any window size.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            orbit_sensitivity: 0.45,
            zoom_sensitivity: 0.35,
            pan_sensitivity: 0.6,
            idle_orbit: true,
            drift_speed: 1.0,
            bloom: 0.15,
            key_light: 2_400.0,
            fill_light: 700.0,
            ambient_light: 180.0,
            fog: 1.0,
            ui_scale: 1.0,
            sound: true,
            volume: 0.7,
            blips: false,
            audio_device: String::new(),
            links: true,
            fullscreen: false,
            grid_rows: 2,
            panes: HashMap::new(),
            dir: PathBuf::new(),
        }
    }
}

/// `<OS config dir>/gae/<slug>`: settings, game progress, user sound overrides.
pub fn config_dir(slug: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("gae")
        .join(slug)
}

pub fn load(slug: &str) -> Settings {
    let dir = config_dir(slug);
    let mut s: Settings = std::fs::read_to_string(dir.join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    s.dir = dir;
    s
}

impl Settings {
    pub fn save(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(self.dir.join("settings.json"), json);
        }
    }
}

/// Write the file whenever a value changed since the last frame.
pub fn persist(settings: Res<Settings>, mut last: Local<Option<Settings>>) {
    if last.as_ref() != Some(&*settings) {
        if last.is_some() {
            settings.save();
        }
        *last = Some(settings.clone());
    }
}
