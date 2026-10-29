//! The radio: cues played through our own rodio sink on a user-chosen output device (Bevy's
//! audio only uses the system default, which on multi-device machines is often a monitor nobody
//! listens to). Clip precedence: `<config dir>/gae/<slug>/sounds/<cue>.wav|ogg`, then the
//! spec's embedded clips, then the engine's defaults.

// cpal's `name()` is deprecated in favour of `description()`, but it is still the stable display string.
#![allow(deprecated)]

use super::settings::{self, Settings};
use super::store::{SpecRes, Store};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use gae_core::spec::Cue;
use rodio::Source;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

fn default_clip(cue: Cue) -> &'static [u8] {
    match cue {
        Cue::UnitAdded => include_bytes!("../assets/sounds/spawn.wav"),
        Cue::Signal => include_bytes!("../assets/sounds/question.wav"),
        Cue::Critical => include_bytes!("../assets/sounds/alarm.wav"),
        Cue::Resolved => include_bytes!("../assets/sounds/allclear.wav"),
        Cue::Dismissed => include_bytes!("../assets/sounds/blip.wav"),
        Cue::SourceCleared => include_bytes!("../assets/sounds/done.wav"),
        Cue::Failure => include_bytes!("../assets/sounds/error.wav"),
        Cue::Stale => include_bytes!("../assets/sounds/stall.wav"),
        Cue::LevelUp | Cue::Badge => include_bytes!("../assets/sounds/fanfare.wav"),
        Cue::Select => include_bytes!("../assets/sounds/yes.wav"),
        Cue::Blip => include_bytes!("../assets/sounds/blip.wav"),
    }
}

enum RadioCmd {
    Play { bytes: Arc<[u8]>, volume: f32 },
    SetDevice(String),
}

#[derive(Resource)]
pub struct Sounds {
    clips: HashMap<Cue, Arc<[u8]>>,
    /// Cue names replaced by files in the config folder.
    pub overrides: Vec<String>,
    pub dir: std::path::PathBuf,
    tx: Sender<RadioCmd>,
    devices: Arc<Mutex<Vec<String>>>,
    status: Arc<Mutex<String>>,
}

fn list_devices() -> Vec<String> {
    let host = rodio::cpal::default_host();
    let mut names: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.dedup();
    names
}

fn open_sink(device_name: &str) -> Result<(rodio::MixerDeviceSink, String), String> {
    let host = rodio::cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_output_device()
    } else {
        host.output_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|n| n == device_name).unwrap_or(false)))
            .or_else(|| host.default_output_device())
    };
    let Some(device) = device else {
        return Err("no output device".into());
    };
    let name = device.name().unwrap_or_else(|_| "?".into());
    let sink = rodio::DeviceSinkBuilder::from_device(device)
        .and_then(|b| b.open_sink_or_fallback())
        .map_err(|e| format!("{name}: {e}"))?;
    Ok((sink, name))
}

fn audio_thread(rx: Receiver<RadioCmd>, devices: Arc<Mutex<Vec<String>>>, status: Arc<Mutex<String>>, initial: String) {
    if let Ok(mut d) = devices.lock() {
        *d = list_devices();
    }
    let set_status = |s: String| {
        if let Ok(mut st) = status.lock() {
            *st = s;
        }
    };
    let mut sink = match open_sink(&initial) {
        Ok((s, name)) => {
            set_status(format!("radio on {name}"));
            Some(s)
        }
        Err(e) => {
            set_status(format!("radio off: {e}"));
            None
        }
    };
    while let Ok(cmd) = rx.recv() {
        match cmd {
            RadioCmd::SetDevice(name) => {
                sink = None; // drop the old stream before opening the next one
                match open_sink(&name) {
                    Ok((s, n)) => {
                        set_status(format!("radio on {n}"));
                        sink = Some(s);
                    }
                    Err(e) => set_status(format!("radio off: {e}")),
                }
                if let Ok(mut d) = devices.lock() {
                    *d = list_devices();
                }
            }
            RadioCmd::Play { bytes, volume } => {
                let Some(s) = sink.as_ref() else { continue };
                match rodio::Decoder::new(Cursor::new(bytes)) {
                    Ok(src) => s.mixer().add(src.amplify(volume)),
                    Err(e) => set_status(format!("radio: bad clip: {e}")),
                }
            }
        }
    }
}

pub fn setup(mut commands: Commands, settings: Res<Settings>, spec: Res<SpecRes>) {
    let dir = settings::config_dir(spec.0.slug).join("sounds");
    let mut clips = HashMap::new();
    let mut overrides = Vec::new();
    for cue in Cue::ALL {
        let custom = ["wav", "ogg"]
            .iter()
            .map(|ext| dir.join(format!("{}.{ext}", cue.name())))
            .find(|p| p.exists())
            .and_then(|p| std::fs::read(&p).ok());
        let bytes: Arc<[u8]> = match custom {
            Some(b) => {
                overrides.push(cue.name().to_string());
                Arc::from(b)
            }
            None => match spec.0.sounds.iter().find(|(c, _)| *c == cue) {
                Some((_, b)) => Arc::from(*b),
                None => Arc::from(default_clip(cue)),
            },
        };
        clips.insert(cue, bytes);
    }
    let (tx, rx) = unbounded();
    let devices = Arc::new(Mutex::new(Vec::new()));
    let status = Arc::new(Mutex::new("radio starting".to_string()));
    {
        let (devices, status, initial) = (devices.clone(), status.clone(), settings.audio_device.clone());
        std::thread::Builder::new()
            .name("radio".into())
            .spawn(move || audio_thread(rx, devices, status, initial))
            .expect("spawn radio thread");
    }
    commands.insert_resource(Sounds {
        clips,
        overrides,
        dir,
        tx,
        devices,
        status,
    });
}

impl Sounds {
    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn device_names(&self) -> Vec<String> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn preview(&self, cue: Cue, volume: f32) {
        if let Some(bytes) = self.clips.get(&cue) {
            let _ = self.tx.send(RadioCmd::Play {
                bytes: bytes.clone(),
                volume: volume.clamp(0.0, 1.0),
            });
        }
    }
}

/// Drain the store's cue queue, respecting mute, volume and per-cue cooldowns.
pub fn play(
    time: Res<Time>,
    settings: Res<Settings>,
    sounds: Res<Sounds>,
    mut store: ResMut<Store>,
    mut last: Local<HashMap<Cue, f32>>,
    mut last_device: Local<Option<String>>,
) {
    if last_device.as_ref() != Some(&settings.audio_device) {
        if last_device.is_some() {
            let _ = sounds.tx.send(RadioCmd::SetDevice(settings.audio_device.clone()));
        }
        *last_device = Some(settings.audio_device.clone());
    }
    let queued: Vec<Cue> = std::mem::take(&mut store.cues);
    if queued.is_empty() || !settings.sound {
        return;
    }
    let now = time.elapsed_secs();
    for cue in queued {
        if cue == Cue::Blip && !settings.blips {
            continue;
        }
        if last.get(&cue).is_some_and(|t| now - t < cue.cooldown()) {
            continue;
        }
        last.insert(cue, now);
        sounds.preview(cue, settings.volume);
    }
}
