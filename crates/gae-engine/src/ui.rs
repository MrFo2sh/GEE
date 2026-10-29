//! egui layer: HUD with the XP bar (top), Needs-You queue and unit tree (left), selected unit
//! (right), status strip (bottom), signal badges over units, unit labels, toasts, the level-up
//! flash, and the settings and trophies cards. Reports which screen areas it covers so the
//! camera ignores input that belongs to the UI.

use super::cards::{begin_card, end_card, floating_card};
use super::settings::Settings;
use super::sounds::Sounds;
use super::store::{SpecRes, Store, ToastKind};
use super::theme::{self, UiTheme};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use gae_core::model::{CORE, How, Severity, Status, Unit, now_millis};
use gae_core::spec::{Cue, Shape, Spec};
use std::collections::HashMap;

#[derive(bevy::ecs::system::SystemParam)]
pub struct UiParams<'w, 's> {
    store: ResMut<'w, Store>,
    settings: ResMut<'w, Settings>,
    spec: Res<'w, SpecRes>,
    th: Res<'w, UiTheme>,
    sounds: Res<'w, Sounds>,
    diagnostics: Res<'w, DiagnosticsStore>,
    monitors: Query<'w, 's, &'static bevy::window::Monitor>,
    styled: Local<'s, bool>,
    last_scale: Local<'s, f32>,
}

pub fn display(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("display".into()))
}

/// Installs fonts and visuals once. Returns true on that frame: egui applies fonts next frame, so
/// a family installed this frame must not be used this frame.
fn style_once(ctx: &egui::Context, th: &UiTheme, done: &mut bool) -> bool {
    if *done {
        return false;
    }
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vt323".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/VT323-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "display".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/BlackOpsOne-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "vt323".to_owned());
    fonts.families.insert(
        egui::FontFamily::Name("display".into()),
        vec!["display".to_owned(), "vt323".to_owned()],
    );
    ctx.set_fonts(fonts);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        for font in style.text_styles.values_mut() {
            if font.family == egui::FontFamily::Proportional {
                font.size *= 1.25;
            }
        }
    });
    let mut v = egui::Visuals::dark();
    v.panel_fill = th.panel;
    v.window_fill = th.panel;
    v.override_text_color = Some(th.text);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, th.dim.gamma_multiply(0.4));
    v.widgets.inactive.bg_fill = th.card;
    v.widgets.hovered.bg_fill = th.accent.gamma_multiply(0.25);
    v.widgets.active.bg_fill = th.accent.gamma_multiply(0.4);
    v.selection.bg_fill = th.accent.gamma_multiply(0.3);
    v.selection.stroke = egui::Stroke::new(1.0, th.accent);
    v.hyperlink_color = th.accent;
    ctx.set_visuals(v);
    *done = true;
    true
}

pub fn age(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

fn status_of(unit: &Unit, spec: &Spec, now: i64, th: &UiTheme) -> (&'static str, egui::Color32) {
    if unit.last_failed() && unit.ended.is_none() {
        return ("failing", th.alert);
    }
    match unit.status(now, spec.active_window_ms) {
        Status::Active => ("active", th.good),
        Status::Idle => ("idle", th.dim),
        Status::Done => ("done", th.dim.gamma_multiply(0.6)),
    }
}

/// Units in tree order (parents before children, siblings by arrival), with their depth.
fn tree_order(store: &Store) -> Vec<(usize, u32)> {
    let mut kids: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, u) in store.world.units.iter().enumerate() {
        if let Some(p) = u.parent.as_deref() {
            kids.entry(p).or_default().push(i);
        }
    }
    for v in kids.values_mut() {
        v.sort_by_key(|&i| store.world.units[i].order);
    }
    let mut out = Vec::with_capacity(store.world.units.len());
    let mut stack: Vec<(usize, u32)> = store
        .world
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.parent.is_none())
        .map(|(i, _)| (i, 0))
        .collect();
    while let Some((i, d)) = stack.pop() {
        out.push((i, d));
        if let Some(ch) = kids.get(store.world.units[i].id.as_str()) {
            for &c in ch.iter().rev() {
                stack.push((c, d + 1));
            }
        }
    }
    out
}

pub fn draw(mut contexts: EguiContexts, p: UiParams) -> Result {
    let UiParams {
        mut store,
        mut settings,
        spec,
        th,
        sounds,
        diagnostics,
        monitors,
        mut styled,
        mut last_scale,
    } = p;
    let spec = &spec.0;
    let th = &*th;
    let ctx = contexts.ctx_mut()?;
    if style_once(ctx, th, &mut styled) {
        return Ok(());
    }
    if (*last_scale - settings.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(settings.ui_scale);
        *last_scale = settings.ui_scale;
    }
    let now = now_millis();
    store.typing = ctx.memory(|m| m.focused().is_some());
    // Where the monitors meet, in window points: the dock grid puts a column line on each seam.
    {
        let ppp = ctx.pixels_per_point();
        let origin = monitors.iter().map(|m| m.physical_position.x).min().unwrap_or(0);
        let mut seams: Vec<f32> = monitors
            .iter()
            .map(|m| (m.physical_position.x - origin) as f32 / ppp)
            .filter(|x| *x > 1.0)
            .collect();
        seams.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        store.monitor_seams = seams;
    }
    if let Some((id, step)) = store.zoom_request.take() {
        let cs = store.cards.entry(id).or_default();
        cs.zoom = if step == 0 {
            1.0
        } else {
            (cs.zoom * 1.1_f32.powi(step)).clamp(0.6, 2.5)
        };
    }

    // ---- cinematic mode: scene, badges and toasts only ----
    if store.hide_ui {
        store.scene_insets = Some((0.0, 0.0, 0.0, 0.0));
        store.pointer_over_ui = false;
        egui::Area::new(egui::Id::new("cinematic-hint"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Esc or F: back to the console")
                        .small()
                        .color(th.dim),
                );
            });
        draw_badges(ctx, &mut store, th, false);
        draw_toasts(ctx, &store, th);
        draw_level_flash(ctx, &store, th);
        return Ok(());
    }

    let mut ui_rects: Vec<egui::Rect> = Vec::new();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // ---- top: HUD ----
    let hud = egui::Panel::top("hud").show(&mut root, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(spec.name.to_uppercase())
                    .font(display(20.0))
                    .color(th.accent),
            );
            if !spec.tagline.is_empty() {
                ui.label(egui::RichText::new(spec.tagline).small().color(th.dim));
            }
            ui.separator();
            let units = store.world.units.len().saturating_sub(1);
            ui.label(egui::RichText::new(format!("{units} {}", spec.nouns.units)).color(th.text));
            let mut by = [0usize; 5];
            for s in &store.world.open {
                by[s.severity.index()] += 1;
            }
            for sev in Severity::ALL.iter().rev() {
                let n = by[sev.index()];
                if n > 0 {
                    ui.label(
                        egui::RichText::new(format!("{n} {}", sev.label()))
                            .monospace()
                            .color(th.severity(*sev)),
                    );
                }
            }
            if store.world.open.is_empty() {
                ui.label(egui::RichText::new(format!("no open {}", spec.nouns.signals)).color(th.good));
            }
            ui.separator();
            // The game: level, XP to the next level, streak.
            let level = store.game.level();
            let (into, span) = store.game.progress();
            ui.label(
                egui::RichText::new(format!("LV {level}"))
                    .font(display(18.0))
                    .color(th.warn),
            );
            ui.add(
                egui::ProgressBar::new(into as f32 / span.max(1) as f32)
                    .desired_width(160.0)
                    .fill(th.warn.gamma_multiply(0.8))
                    .text(egui::RichText::new(format!("{into} / {span} XP")).color(th.text)),
            );
            let streak = store.game.streak;
            ui.label(egui::RichText::new(format!("🔥 {streak}")).color(if streak >= 5 { th.warn } else { th.text }))
                .on_hover_text(format!(
                    "Streak: {} handled in a row. A {} left open past {} breaks it. Best: {}.",
                    spec.nouns.signals,
                    spec.nouns.signal,
                    age(spec.xp.stale_ms),
                    store.game.best_streak
                ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⚙").on_hover_text("Settings").clicked() {
                    store.show_settings = !store.show_settings;
                }
                let sound_label = if settings.sound { "🔊" } else { "🔇" };
                if ui.small_button(sound_label).on_hover_text("Sound on/off").clicked() {
                    settings.sound = !settings.sound;
                }
                if ui.small_button("🎬").on_hover_text("Cinematic mode (F)").clicked() {
                    store.hide_ui = true;
                }
                if ui.small_button("⌂").on_hover_text("Reset the view (R)").clicked() {
                    store.reset_view = true;
                }
                let earned = store.game.badges.len();
                if ui
                    .small_button(format!("🏆 {earned}/{}", spec.achievements.len()))
                    .on_hover_text("Achievements")
                    .clicked()
                {
                    store.show_trophies = !store.show_trophies;
                }
            });
        });
        ui.add_space(2.0);
    });
    ui_rects.push(hud.response.rect);
    let inset_top = hud.response.rect.height();

    // ---- bottom: status strip ----
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let bottom = egui::Panel::bottom("status").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{fps:5.0} fps")).monospace().color(th.dim));
            ui.label(
                egui::RichText::new(format!("{} events", store.events_seen))
                    .monospace()
                    .color(th.dim),
            );
            ui.separator();
            let mut status: Vec<_> = store.status.iter().collect();
            status.sort();
            for (k, v) in status {
                ui.label(egui::RichText::new(format!("{k}: {v}")).monospace().color(th.dim));
            }
            ui.separator();
            ui.label(
                egui::RichText::new("drag rotate · scroll zoom · right-drag pan · R home · F cinematic · Esc back")
                    .small()
                    .color(th.dim),
            );
            if let Some((_, e)) = store.errors.back() {
                ui.separator();
                ui.label(egui::RichText::new(e).monospace().color(th.alert));
            }
        });
    });
    ui_rects.push(bottom.response.rect);
    let inset_bottom = bottom.response.rect.height();

    // ---- left: Needs-You queue and the unit tree ----
    let mut select: Option<String> = None;
    let mut close: Option<(String, How)> = None;
    let left = egui::Panel::left("queue")
        .resizable(true)
        .default_size(330.0)
        .show(&mut root, |ui| {
            ui.add_space(6.0);
            let open = store.world.open.len();
            ui.label(
                egui::RichText::new(format!("NEEDS YOU  {open}"))
                    .font(display(16.0))
                    .color(if open > 0 { th.warn } else { th.good }),
            );
            ui.add_space(2.0);
            let queue_h = (ui.available_height() * 0.55).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt("queue-scroll")
                .max_height(queue_h)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if open == 0 {
                        ui.label(
                            egui::RichText::new(format!("All clear. New {} land here.", spec.nouns.signals))
                                .color(th.dim),
                        );
                    }
                    let shown = store.queue_shown;
                    let queue = store.world.queue();
                    for s in queue.iter().take(shown) {
                        let color = th.severity(s.severity);
                        egui::Frame::NONE
                            .fill(th.card)
                            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.6)))
                            .inner_margin(6.0)
                            .corner_radius(4.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(s.severity.label().to_uppercase())
                                            .monospace()
                                            .color(color),
                                    );
                                    let unit = store.world.unit(&s.unit).map(|u| u.title()).unwrap_or(&s.unit);
                                    if ui.link(egui::RichText::new(unit).color(th.text)).clicked() {
                                        select = Some(s.unit.clone());
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let stale = if s.stale { " stale" } else { "" };
                                        ui.label(
                                            egui::RichText::new(format!("{}{stale}", age(now - s.at)))
                                                .small()
                                                .color(if s.stale { th.alert } else { th.dim }),
                                        );
                                        if s.count > 1 {
                                            ui.label(
                                                egui::RichText::new(format!("×{}", s.count)).small().color(th.warn),
                                            );
                                        }
                                    });
                                });
                                ui.label(egui::RichText::new(&s.title).color(th.text));
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(egui::RichText::new(format!("✔ {}", spec.nouns.resolve)).color(th.good))
                                        .clicked()
                                    {
                                        close = Some((s.id.clone(), How::Resolved));
                                    }
                                    if ui
                                        .button(egui::RichText::new(format!("✖ {}", spec.nouns.dismiss)).color(th.dim))
                                        .on_hover_text("Noise or a false positive: less XP, keeps the streak")
                                        .clicked()
                                    {
                                        close = Some((s.id.clone(), How::Dismissed));
                                    }
                                });
                            });
                        ui.add_space(3.0);
                    }
                    if queue.len() > shown && ui.button(format!("show {} more", queue.len() - shown)).clicked() {
                        store.queue_shown += theme::PAGE;
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(spec.nouns.units.to_uppercase())
                        .font(display(16.0))
                        .color(th.accent),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut store.tree_filter)
                        .hint_text("filter")
                        .desired_width(140.0),
                );
            });
            let filter = store.tree_filter.trim().to_ascii_lowercase();
            let order = tree_order(&store);
            egui::ScrollArea::vertical()
                .id_salt("tree-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut drawn = 0;
                    let mut more = 0;
                    for (i, depth) in order {
                        let u = &store.world.units[i];
                        if !filter.is_empty()
                            && !u.id.to_ascii_lowercase().contains(&filter)
                            && !u.label.to_ascii_lowercase().contains(&filter)
                            && !u.kind.to_ascii_lowercase().contains(&filter)
                        {
                            continue;
                        }
                        if drawn >= store.tree_shown {
                            more += 1;
                            continue;
                        }
                        drawn += 1;
                        let style = spec.kind(&u.kind);
                        let (_, scolor) = status_of(u, spec, now, th);
                        ui.horizontal(|ui| {
                            ui.add_space(if filter.is_empty() { depth as f32 * 12.0 } else { 0.0 });
                            let shape = (u.id != CORE).then_some(style.shape);
                            kind_icon(ui, shape, theme::color32(style.color), 12.0);
                            let sel = store.selected.as_deref() == Some(u.id.as_str());
                            if ui
                                .selectable_label(sel, egui::RichText::new(u.title()).color(scolor))
                                .clicked()
                            {
                                select = Some(u.id.clone());
                            }
                            if let Some(sev) = store.world.unit_alert(&u.id) {
                                dot(ui, th.severity(sev), 8.0);
                            }
                        });
                    }
                    if more > 0 && ui.button(format!("show {more} more")).clicked() {
                        store.tree_shown += theme::PAGE;
                    }
                });
        });
    ui_rects.push(left.response.rect);
    let inset_left = left.response.rect.width();

    // ---- right: the selected unit ----
    let mut inset_right = 0.0_f32;
    let mut deselect = false;
    if let Some(id) = store.selected.clone()
        && store.world.unit(&id).is_some()
    {
        let right = egui::Panel::right("unit")
            .resizable(true)
            .default_size(440.0)
            .max_size(ctx.viewport_rect().width() * 0.5)
            .show(&mut root, |ui| {
                ui.add_space(6.0);
                if ui.small_button("✕ close  (Esc)").clicked() {
                    deselect = true;
                }
                unit_panel(ui, &mut store, spec, th, &id, now, &mut select, &mut close);
            });
        ui_rects.push(right.response.rect);
        inset_right = right.response.rect.width();
    }

    // ---- cards ----
    let mut cards: Vec<(&str, egui::Rect)> = Vec::new();
    if store.show_settings
        && let Some(r) = draw_settings(ctx, &mut store, &mut settings, &sounds, th)
    {
        cards.push(("settings-card", r));
    }
    if store.show_trophies
        && let Some(r) = draw_trophies(ctx, &mut store, &mut settings, spec, th)
    {
        cards.push(("trophies-card", r));
    }
    let pointer = ctx.pointer_latest_pos();
    store.hot_card = cards
        .iter()
        .find(|(_, r)| pointer.is_some_and(|p| r.contains(p)))
        .map(|(id, _)| id.to_string());
    ui_rects.extend(cards.iter().map(|(_, r)| *r));

    ui_rects.extend(draw_badges(ctx, &mut store, th, true));
    draw_labels(ctx, &store, th);
    draw_toasts(ctx, &store, th);
    draw_level_flash(ctx, &store, th);

    if let Some((sid, how)) = close {
        store.close_signal(spec, &sid, how);
    }
    if let Some(u) = select {
        store.selected = Some(u);
        store.timeline_shown = 50;
    }
    if deselect {
        store.selected = None;
    }
    store.scene_insets = Some((inset_left, inset_top, inset_right, inset_bottom));
    store.pointer_over_ui = ctx
        .pointer_latest_pos()
        .map(|p| ui_rects.iter().any(|r| r.contains(p)))
        .unwrap_or(false);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn unit_panel(
    ui: &mut egui::Ui,
    store: &mut Store,
    spec: &Spec,
    th: &UiTheme,
    id: &str,
    now: i64,
    select: &mut Option<String>,
    close: &mut Option<(String, How)>,
) {
    let Some(u) = store.world.unit(id) else { return };
    let style = spec.kind(&u.kind);
    let (status, scolor) = status_of(u, spec, now, th);
    ui.horizontal(|ui| {
        kind_icon(
            ui,
            (u.id != CORE).then_some(style.shape),
            theme::color32(style.color),
            20.0,
        );
        ui.label(egui::RichText::new(u.title()).font(display(20.0)).color(th.text));
    });
    ui.horizontal_wrapped(|ui| {
        let kind = if u.id == CORE {
            spec.nouns.core
        } else if u.kind.is_empty() {
            spec.nouns.unit
        } else {
            u.kind.as_str()
        };
        ui.label(egui::RichText::new(kind).monospace().color(th.dim));
        ui.label(egui::RichText::new(status).color(scolor));
        ui.label(
            egui::RichText::new(format!("seen {} ago", age(now - u.last_seen)))
                .small()
                .color(th.dim),
        );
        if let Some(p) = u.parent.as_deref() {
            let pt = store.world.unit(p).map(|x| x.title()).unwrap_or(p);
            ui.label(egui::RichText::new("under").small().color(th.dim));
            if ui.link(pt).clicked() {
                *select = Some(p.to_string());
            }
        }
    });
    ui.label(egui::RichText::new(&u.id).small().monospace().color(th.dim));
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("unit-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let signals: Vec<_> = store.world.open.iter().filter(|s| s.unit == id).collect();
            if !signals.is_empty() {
                ui.label(
                    egui::RichText::new(spec.nouns.signals.to_uppercase())
                        .font(display(14.0))
                        .color(th.warn),
                );
                for s in signals {
                    let color = th.severity(s.severity);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(s.severity.label()).monospace().color(color));
                        ui.label(egui::RichText::new(&s.title).color(th.text));
                        ui.label(egui::RichText::new(age(now - s.at)).small().color(th.dim));
                    });
                    if !s.detail.is_empty() {
                        ui.label(egui::RichText::new(&s.detail).small().monospace().color(th.dim));
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button(format!("✔ {}", spec.nouns.resolve)).clicked() {
                            *close = Some((s.id.clone(), How::Resolved));
                        }
                        if ui.small_button(format!("✖ {}", spec.nouns.dismiss)).clicked() {
                            *close = Some((s.id.clone(), How::Dismissed));
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.separator();
            }
            if !u.meta.is_empty() || !u.metrics.is_empty() {
                egui::Grid::new("unit-meta")
                    .num_columns(2)
                    .spacing([14.0, 2.0])
                    .show(ui, |ui| {
                        for (k, v) in &u.meta {
                            ui.label(egui::RichText::new(k).color(th.dim));
                            ui.label(egui::RichText::new(v).monospace());
                            ui.end_row();
                        }
                        for (k, v) in &u.metrics {
                            ui.label(egui::RichText::new(k).color(th.dim));
                            ui.label(egui::RichText::new(format!("{v:.2}")).monospace().color(th.accent));
                            ui.end_row();
                        }
                    });
                ui.separator();
            }
            let kids: Vec<(String, String)> = store
                .world
                .children(id)
                .take(60)
                .map(|c| (c.id.clone(), c.title().to_string()))
                .collect();
            if !kids.is_empty() {
                ui.label(
                    egui::RichText::new(spec.nouns.units.to_uppercase())
                        .font(display(14.0))
                        .color(th.accent),
                );
                ui.horizontal_wrapped(|ui| {
                    for (cid, title) in kids {
                        if ui.small_button(title).clicked() {
                            *select = Some(cid);
                        }
                    }
                });
                ui.separator();
            }
            if !u.artifacts.is_empty() {
                ui.label(egui::RichText::new("ARTIFACTS").font(display(14.0)).color(th.accent));
                for a in u.artifacts.iter().rev().take(40) {
                    ui.horizontal_wrapped(|ui| {
                        if !a.kind.is_empty() {
                            ui.label(egui::RichText::new(&a.kind).small().monospace().color(th.dim));
                        }
                        let text = if a.label.is_empty() { &a.target } else { &a.label };
                        if a.target.starts_with("http://") || a.target.starts_with("https://") {
                            ui.hyperlink_to(text, &a.target);
                        } else {
                            ui.label(egui::RichText::new(text).monospace()).on_hover_text(&a.target);
                        }
                    });
                }
                ui.separator();
            }
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}  {} · {} failed",
                        spec.nouns.actions.to_uppercase(),
                        u.action_count,
                        u.fail_count
                    ))
                    .font(display(14.0))
                    .color(th.accent),
                );
            });
            ui.add(
                egui::TextEdit::singleline(&mut store.timeline_query)
                    .hint_text("🔍 search")
                    .desired_width(f32::INFINITY),
            );
            let q = store.timeline_query.trim().to_ascii_lowercase();
            let Some(u) = store.world.unit(id) else { return };
            let mut drawn = 0;
            let mut more = false;
            for a in u.actions.iter().rev() {
                if !q.is_empty()
                    && !a.summary.to_ascii_lowercase().contains(&q)
                    && !a.kind.to_ascii_lowercase().contains(&q)
                    && !a.output.to_ascii_lowercase().contains(&q)
                {
                    continue;
                }
                if drawn >= store.timeline_shown {
                    more = true;
                    break;
                }
                drawn += 1;
                let (mark, color) = match a.ok {
                    None => ("…", th.good),
                    Some(true) => ("✔", th.dim),
                    Some(false) => ("✖", th.alert),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(mark).color(color));
                    ui.label(egui::RichText::new(age(now - a.started)).small().color(th.dim));
                    if !a.kind.is_empty() {
                        ui.label(egui::RichText::new(&a.kind).small().monospace().color(th.accent));
                    }
                    ui.label(egui::RichText::new(&a.summary).monospace());
                });
                if !a.output.is_empty() {
                    let lines: Vec<&str> = a.output.lines().take(6).collect();
                    ui.label(egui::RichText::new(lines.join("\n")).small().monospace().color(th.dim));
                }
            }
            if drawn == 0 {
                ui.label(egui::RichText::new(format!("No {} yet.", spec.nouns.actions)).color(th.dim));
            }
            if more && ui.button("show more").clicked() {
                store.timeline_shown += 50;
            }
        });
}

/// A unit's shape, painted (font glyphs for ◆ ● ◉ are missing from the bundled fonts).
/// `None` is the core: a ring around a dot.
fn kind_icon(ui: &mut egui::Ui, shape: Option<Shape>, color: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = size * 0.42;
    match shape {
        None => {
            p.circle_stroke(c, r, egui::Stroke::new(1.5, color));
            p.circle_filled(c, r * 0.45, color);
        }
        Some(Shape::Sphere | Shape::Capsule) => {
            p.circle_filled(c, r, color);
        }
        Some(Shape::Cube) => {
            p.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(r * 1.7, r * 1.7)),
                1.0,
                color,
            );
        }
        Some(Shape::Cylinder) => {
            let outer = egui::Rect::from_center_size(c, egui::vec2(r * 2.0, r * 2.0));
            p.rect_stroke(outer, 2.0, egui::Stroke::new(1.5, color), egui::StrokeKind::Inside);
            p.rect_filled(outer.shrink(r * 0.55), 1.0, color);
        }
        Some(Shape::Diamond) => {
            let pts = vec![
                c + egui::vec2(0.0, -r),
                c + egui::vec2(r, 0.0),
                c + egui::vec2(0.0, r),
                c + egui::vec2(-r, 0.0),
            ];
            p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
        }
    }
}

fn dot(ui: &mut egui::Ui, color: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size * 0.5, color);
}

/// A badge over every unit with open signals; click to select. Returns their rects.
fn draw_badges(ctx: &egui::Context, store: &mut Store, th: &UiTheme, interactive: bool) -> Vec<egui::Rect> {
    let mut rects = Vec::new();
    let mut badges = store.badges.clone();
    badges.sort_by_key(|b| std::cmp::Reverse(b.severity));
    let mut pick = None;
    for b in badges.iter().take(200) {
        let color = th.severity(b.severity);
        let resp = egui::Area::new(egui::Id::new(("badge", &b.unit)))
            .fixed_pos(egui::pos2(b.x - 14.0, b.y - 30.0))
            .order(egui::Order::Middle)
            .interactable(interactive)
            .show(ctx, |ui| {
                let text = if b.count > 1 {
                    format!("! {}", b.count)
                } else {
                    "!".to_string()
                };
                egui::Frame::NONE
                    .fill(color)
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(text).strong().color(th.ink));
                    });
                if interactive {
                    let rect = ui.min_rect();
                    ui.interact(rect, egui::Id::new(("badge-click", &b.unit)), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
        let r = resp.response.rect;
        // A click sensor of its own: re-sensing the area's response would reuse its id.
        if interactive
            && ctx
                .read_response(egui::Id::new(("badge-click", &b.unit)))
                .is_some_and(|c| c.clicked())
        {
            pick = Some(b.unit.clone());
        }
        rects.push(r);
    }
    if let Some(u) = pick {
        store.selected = Some(u);
    }
    rects
}

fn draw_labels(ctx: &egui::Context, store: &Store, th: &UiTheme) {
    for (i, (text, x, y)) in store.labels.iter().enumerate() {
        egui::Area::new(egui::Id::new(("label", i)))
            .fixed_pos(egui::pos2(x + 14.0, y - 8.0))
            .order(egui::Order::Background)
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(text).monospace().color(th.text));
            });
    }
}

fn draw_toasts(ctx: &egui::Context, store: &Store, th: &UiTheme) {
    if store.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .interactable(false)
        .show(ctx, |ui| {
            for t in store.toasts.iter().rev() {
                let alpha = (1.0 - t.born.elapsed().as_secs_f32() / 7.0).clamp(0.0, 1.0);
                if alpha <= 0.0 {
                    continue;
                }
                let color = match t.kind {
                    ToastKind::Xp => th.warn,
                    ToastKind::Level | ToastKind::Badge => th.accent,
                    ToastKind::Error => th.alert,
                    ToastKind::Warn => th.warn,
                    ToastKind::Info => th.dim,
                };
                egui::Frame::NONE
                    .fill(th.card.gamma_multiply(alpha))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(alpha)))
                    .inner_margin(8.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(
                            egui::RichText::new(&t.text)
                                .color(color.gamma_multiply(alpha))
                                .monospace(),
                        );
                    });
                ui.add_space(4.0);
            }
        });
}

fn draw_level_flash(ctx: &egui::Context, store: &Store, th: &UiTheme) {
    let Some(t) = store.level_flash else { return };
    let s = t.elapsed().as_secs_f32();
    if s > 2.5 {
        return;
    }
    let alpha = (1.0 - s / 2.5).clamp(0.0, 1.0);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("level-flash"),
    ));
    let c = ctx.viewport_rect().center();
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        format!("LEVEL {}", store.game.level()),
        display(64.0 + 24.0 * s),
        th.warn.gamma_multiply(alpha),
    );
    ctx.request_repaint();
}

fn draw_settings(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    sounds: &Sounds,
    th: &UiTheme,
) -> Option<egui::Rect> {
    let (mut cs, mut dock, grid) = begin_card(store, settings, "settings-card", ctx);
    let default_pos = ctx.viewport_rect().center() - egui::vec2(220.0, 260.0);
    let mut preview: Option<Cue> = None;
    let out = floating_card(
        ctx,
        th,
        "settings-card",
        false,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(440.0, 520.0),
        None,
        |ui| {
            ui.label(egui::RichText::new("SETTINGS").font(display(16.0)).color(th.accent));
        },
        |ui, _| {
            egui::ScrollArea::vertical().id_salt("settings-scroll").show(ui, |ui| {
                ui.label(egui::RichText::new("camera").color(th.dim));
                ui.add(egui::Slider::new(&mut settings.orbit_sensitivity, 0.05..=2.0).text("orbit"));
                ui.add(egui::Slider::new(&mut settings.zoom_sensitivity, 0.05..=2.0).text("zoom"));
                ui.add(egui::Slider::new(&mut settings.pan_sensitivity, 0.05..=2.0).text("pan"));
                ui.checkbox(&mut settings.idle_orbit, "slow orbit when idle");
                ui.add(egui::Slider::new(&mut settings.drift_speed, 0.0..=4.0).text("drift"));
                ui.separator();
                ui.label(egui::RichText::new("look").color(th.dim));
                ui.add(egui::Slider::new(&mut settings.bloom, 0.0..=0.6).text("bloom"));
                ui.add(egui::Slider::new(&mut settings.key_light, 0.0..=8_000.0).text("key light"));
                ui.add(egui::Slider::new(&mut settings.fill_light, 0.0..=4_000.0).text("fill light"));
                ui.add(egui::Slider::new(&mut settings.ambient_light, 0.0..=800.0).text("ambient"));
                ui.add(egui::Slider::new(&mut settings.fog, 0.2..=3.0).text("fog"));
                ui.add(egui::Slider::new(&mut settings.ui_scale, 0.6..=1.8).text("ui scale"));
                ui.checkbox(&mut settings.links, "beams to parents");
                ui.checkbox(&mut settings.fullscreen, "full screen at start");
                ui.add(egui::Slider::new(&mut settings.grid_rows, 1..=4).text("dock rows"));
                ui.separator();
                ui.label(egui::RichText::new("sound").color(th.dim));
                ui.checkbox(&mut settings.sound, "sound");
                ui.add(egui::Slider::new(&mut settings.volume, 0.0..=1.0).text("volume"));
                ui.checkbox(&mut settings.blips, "blip on every action");
                egui::ComboBox::from_id_salt("audio-device")
                    .selected_text(if settings.audio_device.is_empty() {
                        "system default".to_string()
                    } else {
                        settings.audio_device.clone()
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(settings.audio_device.is_empty(), "system default")
                            .clicked()
                        {
                            settings.audio_device.clear();
                        }
                        for name in sounds.device_names() {
                            if ui.selectable_label(settings.audio_device == name, &name).clicked() {
                                settings.audio_device = name;
                            }
                        }
                    });
                ui.label(egui::RichText::new(sounds.status()).small().color(th.dim));
                egui::Grid::new("cue-glossary")
                    .num_columns(2)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        for cue in Cue::ALL {
                            if ui.small_button(cue.name()).clicked() {
                                preview = Some(cue);
                            }
                            ui.label(egui::RichText::new(cue.when()).small().color(th.dim));
                            ui.end_row();
                        }
                    });
                ui.label(
                    egui::RichText::new(format!(
                        "Replace any cue with {}\\<name>.wav. Replaced: {}",
                        sounds.dir.display(),
                        if sounds.overrides.is_empty() {
                            "none".to_string()
                        } else {
                            sounds.overrides.join(", ")
                        }
                    ))
                    .small()
                    .color(th.dim),
                );
                ui.separator();
                if ui.button("reset defaults").clicked() {
                    let keep = (settings.panes.clone(), settings.dir.clone());
                    *settings = Settings::default();
                    settings.panes = keep.0;
                    settings.dir = keep.1;
                }
                ui.label(
                    egui::RichText::new(settings.dir.display().to_string())
                        .small()
                        .color(th.dim),
                );
            });
        },
    );
    end_card(store, settings, "settings-card", cs, dock, &out.sash);
    if let Some(cue) = preview {
        sounds.preview(cue, settings.volume);
    }
    if out.close {
        store.show_settings = false;
    }
    out.rect
}

fn draw_trophies(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    spec: &Spec,
    th: &UiTheme,
) -> Option<egui::Rect> {
    let (mut cs, mut dock, grid) = begin_card(store, settings, "trophies-card", ctx);
    let default_pos = ctx.viewport_rect().center() - egui::vec2(200.0, 220.0);
    let game = store.game.clone();
    let out = floating_card(
        ctx,
        th,
        "trophies-card",
        false,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(400.0, 440.0),
        None,
        |ui| {
            ui.label(egui::RichText::new("ACHIEVEMENTS").font(display(16.0)).color(th.accent));
        },
        |ui, _| {
            egui::Grid::new("game-stats")
                .num_columns(2)
                .spacing([14.0, 2.0])
                .show(ui, |ui| {
                    let rows = [
                        ("level", game.level().to_string()),
                        ("xp", game.xp.to_string()),
                        (spec.nouns.resolve, game.resolved.to_string()),
                        (spec.nouns.dismiss, game.dismissed.to_string()),
                        ("fast", game.fast.to_string()),
                        ("best streak", game.best_streak.to_string()),
                        ("went stale", game.stale.to_string()),
                    ];
                    for (k, v) in rows {
                        ui.label(egui::RichText::new(k).color(th.dim));
                        ui.label(egui::RichText::new(v).monospace());
                        ui.end_row();
                    }
                });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("trophies-scroll").show(ui, |ui| {
                for a in &spec.achievements {
                    let earned = game.has(a.id);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(if earned { "🏆" } else { "○" }).color(if earned {
                            th.warn
                        } else {
                            th.dim
                        }));
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(a.title).color(if earned { th.text } else { th.dim }));
                            ui.label(egui::RichText::new(a.detail).small().color(th.dim));
                        });
                    });
                }
            });
        },
    );
    end_card(store, settings, "trophies-card", cs, dock, &out.sash);
    if out.close {
        store.show_trophies = false;
    }
    out.rect
}
