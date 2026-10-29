//! Cards: every floating window (settings, trophies, signal detail) shares this chrome. Drag
//! by the title row (a drag needs real pointer motion, so a slow click is still a click), drop
//! on a dock target (quarters, halves, monitors, strips) to dock, drag edges to resize (panes
//! sharing the edge follow), ⛶ to fill, Ctrl+F to search, Ctrl +/-/0 to zoom the card under
//! the pointer. Ported from agent-globe, where every rule here cost a bug to learn.

use super::settings::{Pane, Settings};
use super::store::{CardState, Store};
use super::theme::UiTheme;
use bevy_egui::egui;

/// What a floating card reports back: header buttons pressed, where it ended up, and what the
/// pointer did to its title row and edges this frame.
#[derive(Default)]
pub struct CardOut {
    pub close: bool,
    pub toggle_max: bool,
    pub rect: Option<egui::Rect>,
    /// The title is being dragged this frame (egui's one-frame start flag is not used: it fires
    /// before the pointer has moved far enough to tell a drag from a slow click).
    pub dragging: bool,
    pub drag_delta: egui::Vec2,
    pub drag_stopped: bool,
    /// An edge of a docked card being dragged: 0 left, 1 right, 2 top, 3 bottom, 4 corner.
    pub edge: Option<u8>,
    /// Edges this frame moved: (axis 0 = x / 1 = y, from, to) as fractions. Neighbouring panes
    /// sharing that line follow, so shrinking one pane grows the next.
    pub sash: Vec<(u8, f32, f32)>,
}

/// The dock grid. Columns follow the monitors: each monitor's free span (what is left of it
/// beside the sidebars and the agent panel) is split in two, so the seam between monitors is
/// always a grid line and a pane docked on one monitor never spills onto the next. Rows split
/// the free height evenly. Panes are fractions of the whole window, clipped to the free area
/// when drawn, so opening a panel pushes them and closing it gives the space back.
#[derive(Clone)]
pub struct DockGrid {
    vp: egui::Rect,
    area: egui::Rect,
    /// Column lines as fractions of the window width, ascending; first and last are the area's edges.
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl DockGrid {
    const MIN: f32 = 0.04;

    fn new(vp: egui::Rect, area: egui::Rect, seams: &[f32], rows: usize) -> DockGrid {
        let fx = |x: f32| ((x - vp.min.x) / vp.width().max(1.0)).clamp(0.0, 1.0);
        let fy = |y: f32| ((y - vp.min.y) / vp.height().max(1.0)).clamp(0.0, 1.0);
        let mut edges = vec![area.min.x];
        for s in seams {
            if *s > area.min.x + 40.0 && *s < area.max.x - 40.0 {
                edges.push(*s);
            }
        }
        edges.push(area.max.x);
        edges.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut xs: Vec<f32> = Vec::new();
        for w in edges.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a < 80.0 {
                continue;
            }
            xs.push(fx(a));
            xs.push(fx((a + b) * 0.5));
        }
        xs.push(fx(area.max.x));
        if xs.len() < 2 {
            xs = vec![fx(area.min.x), fx(area.max.x)];
        }
        let rows = rows.clamp(1, 6);
        let ys: Vec<f32> = (0..=rows)
            .map(|i| fy(area.min.y + area.height() * i as f32 / rows as f32))
            .collect();
        DockGrid { vp, area, xs, ys }
    }

    fn cols(&self) -> usize {
        self.xs.len() - 1
    }

    fn rows(&self) -> usize {
        self.ys.len() - 1
    }

    fn to_px(&self, p: Pane) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(
                self.vp.min.x + p.x0 * self.vp.width(),
                self.vp.min.y + p.y0 * self.vp.height(),
            ),
            egui::pos2(
                self.vp.min.x + p.x1 * self.vp.width(),
                self.vp.min.y + p.y1 * self.vp.height(),
            ),
        )
    }

    /// A pane kept inside the window and no smaller than a sliver.
    fn clamp(&self, p: Pane) -> Pane {
        let x0 = p.x0.clamp(0.0, 1.0 - Self::MIN);
        let y0 = p.y0.clamp(0.0, 1.0 - Self::MIN);
        Pane {
            x0,
            y0,
            x1: p.x1.clamp(x0 + Self::MIN, 1.0),
            y1: p.y1.clamp(y0 + Self::MIN, 1.0),
        }
    }

    /// Where a pane is drawn: its rectangle clipped to the free area, with a small margin, never
    /// smaller than a sliver (a pane pushed under a panel keeps a sliver at the area's edge).
    fn block(&self, p: Pane) -> egui::Rect {
        let r = self.to_px(self.clamp(p));
        let a = self.area;
        let min_w = 120.0_f32.min(a.width());
        let min_h = 80.0_f32.min(a.height());
        let mut x0 = r.min.x.max(a.min.x);
        let mut x1 = r.max.x.min(a.max.x);
        if x1 - x0 < min_w {
            if x0 + min_w <= a.max.x {
                x1 = x0 + min_w;
            } else {
                x1 = a.max.x;
                x0 = x1 - min_w;
            }
        }
        let mut y0 = r.min.y.max(a.min.y);
        let mut y1 = r.max.y.min(a.max.y);
        if y1 - y0 < min_h {
            if y0 + min_h <= a.max.y {
                y1 = y0 + min_h;
            } else {
                y1 = a.max.y;
                y0 = y1 - min_h;
            }
        }
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)).shrink(6.0)
    }

    fn cell_rect(&self, c: usize, r: usize) -> egui::Rect {
        self.to_px(self.pane_of(c as u8, c as u8 + 1, r as u8, r as u8 + 1))
    }

    fn pane_of(&self, c0: u8, c1: u8, r0: u8, r1: u8) -> Pane {
        let cx = |i: u8| self.xs[(i as usize).min(self.xs.len() - 1)];
        let cy = |i: u8| self.ys[(i as usize).min(self.ys.len() - 1)];
        Pane {
            x0: cx(c0),
            x1: cx(c1),
            y0: cy(r0),
            y1: cy(r1),
        }
    }

    /// Every block a card can be dropped into: spans of 1, 2 and 4 cells in each direction, each
    /// aligned to its own span. A quarter, a half, a monitor, a strip across the wall, the whole
    /// wall. Their targets sit at the blocks' centres, which never coincide. Once docked, edges
    /// move freely.
    fn targets(&self) -> Vec<Pane> {
        let (cols, rows) = (self.cols() as u8, self.rows() as u8);
        let mut out: Vec<Pane> = Vec::new();
        for &w in &[1u8, 2, 4] {
            for &h in &[1u8, 2, 4] {
                for r in (0..rows).step_by(h as usize) {
                    for c in (0..cols).step_by(w as usize) {
                        if c + w <= cols && r + h <= rows {
                            let p = self.pane_of(c, c + w, r, r + h);
                            if !out.contains(&p) {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// How many grid cells a pane covers: the cells whose centres fall inside it.
    fn cells(&self, p: Pane) -> f32 {
        let cols = self
            .xs
            .windows(2)
            .filter(|w| {
                let c = (w[0] + w[1]) * 0.5;
                c > p.x0 && c < p.x1
            })
            .count();
        let rows = self
            .ys
            .windows(2)
            .filter(|w| {
                let c = (w[0] + w[1]) * 0.5;
                c > p.y0 && c < p.y1
            })
            .count();
        (cols * rows) as f32
    }

    fn target_rect(&self, p: Pane) -> egui::Rect {
        let cells = self.cells(p);
        let size = if cells >= 7.5 {
            egui::vec2(170.0, 94.0)
        } else if cells >= 3.5 {
            egui::vec2(150.0, 84.0)
        } else if cells >= 1.5 {
            egui::vec2(130.0, 74.0)
        } else {
            egui::vec2(110.0, 64.0)
        };
        egui::Rect::from_center_size(self.block(p).center(), size)
    }

    fn target_at(&self, p: egui::Pos2) -> Option<Pane> {
        self.targets().into_iter().find(|d| self.target_rect(*d).contains(p))
    }

    /// A pointer x as a fraction of the window, pulled onto a grid line when within a few pixels.
    fn snap_x(&self, x: f32) -> f32 {
        let f = ((x - self.vp.min.x) / self.vp.width().max(1.0)).clamp(0.0, 1.0);
        let px = |g: f32| self.vp.min.x + g * self.vp.width();
        self.xs
            .iter()
            .copied()
            .find(|g| (px(*g) - x).abs() <= 12.0)
            .unwrap_or(f)
    }

    fn snap_y(&self, y: f32) -> f32 {
        let f = ((y - self.vp.min.y) / self.vp.height().max(1.0)).clamp(0.0, 1.0);
        let py = |g: f32| self.vp.min.y + g * self.vp.height();
        self.ys
            .iter()
            .copied()
            .find(|g| (py(*g) - y).abs() <= 12.0)
            .unwrap_or(f)
    }
}

/// Take a card's state out of the store and settings for the frame (so the card's closures can
/// borrow those freely), and the grid it docks into.
pub fn begin_card(
    store: &mut Store,
    settings: &mut Settings,
    id: &str,
    ctx: &egui::Context,
) -> (CardState, Option<Pane>, DockGrid) {
    let cs = store.cards.remove(id).unwrap_or_default();
    let dock = settings.panes.remove(id);
    // The dock area is the free space: right of both left sidebars, left of the agent panel,
    // under the top bar, above the status bar. Panes are fractions of it, so they follow when a
    // panel opens or closes.
    let vr = ctx.viewport_rect();
    let (left, top, right, bottom) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let mut area = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + left, vr.min.y + top),
        egui::pos2(vr.max.x - right, vr.max.y - bottom),
    );
    if area.width() < 240.0 || area.height() < 160.0 {
        area = vr;
    }
    let grid = DockGrid::new(vr, area, &store.monitor_seams, settings.grid_rows as usize);
    (cs, dock, grid)
}

pub fn end_card(
    store: &mut Store,
    settings: &mut Settings,
    id: &str,
    cs: CardState,
    dock: Option<Pane>,
    sash: &[(u8, f32, f32)],
) {
    store.cards.insert(id.to_string(), cs);
    // A moved edge drags every other docked edge on the same line with it: a shared sash, as
    // between editor groups. Panes never drop below a sliver.
    for (axis, from, to) in sash {
        for (k, p) in settings.panes.iter_mut() {
            if k == id {
                continue;
            }
            *p = sash_follow(*p, *axis, *from, *to);
        }
    }
    if let Some(d) = dock {
        settings.panes.insert(id.to_string(), d);
    }
}

/// `p` with any edge lying on `from` (axis 0: x, 1: y) moved to `to`, kept at least a sliver wide.
fn sash_follow(p: Pane, axis: u8, from: f32, to: f32) -> Pane {
    let eps = 0.004;
    let min = DockGrid::MIN;
    let mut q = p;
    if axis == 0 {
        if (p.x0 - from).abs() <= eps {
            q.x0 = to.min(q.x1 - min).max(0.0);
        }
        if (p.x1 - from).abs() <= eps {
            q.x1 = to.max(q.x0 + min).min(1.0);
        }
    } else {
        if (p.y0 - from).abs() <= eps {
            q.y0 = to.min(q.y1 - min).max(0.0);
        }
        if (p.y1 - from).abs() <= eps {
            q.y1 = to.max(q.y0 + min).min(1.0);
        }
    }
    q
}

/// While a title is dragged: the grid, faint, and a target for every block a card can fill
/// (quarter, half, whole monitor); the one under the pointer lights up. Dropping anywhere else
/// leaves the card floating where it is.
fn draw_dock_zones(ctx: &egui::Context, th: &UiTheme, grid: &DockGrid, pointer: egui::Pos2) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dock-zones")));
    let hot = grid.target_at(pointer);
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            let cell = grid.cell_rect(c, r).shrink(3.0);
            painter.rect_stroke(
                cell,
                4.0,
                egui::Stroke::new(1.0, th.accent.gamma_multiply(0.35)),
                egui::StrokeKind::Inside,
            );
        }
    }
    if let Some(d) = hot {
        painter.rect_filled(grid.block(d), 6.0, th.accent.gamma_multiply(0.12));
        painter.rect_stroke(
            grid.block(d),
            6.0,
            egui::Stroke::new(1.5, th.accent),
            egui::StrokeKind::Inside,
        );
    }
    for d in grid.targets() {
        let t = grid.target_rect(d);
        let lit = hot == Some(d);
        let cells = grid.cells(d).round() as u32;
        let label = match cells {
            1 => "¼",
            2 => "½",
            4 => "1",
            8 => "2",
            _ => "·",
        };
        painter.rect_filled(
            t,
            6.0,
            if lit {
                th.accent.gamma_multiply(0.75)
            } else {
                th.card.gamma_multiply(0.85)
            },
        );
        painter.rect_stroke(
            t,
            6.0,
            egui::Stroke::new(1.5, if lit { th.warn } else { th.accent }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            t.center(),
            egui::Align2::CENTER_CENTER,
            if lit { "dock here" } else { label },
            egui::FontId::proportional(if lit { 14.0 } else { 22.0 }),
            if lit { th.ink } else { th.warn },
        );
    }
}

/// The chrome shared by every card (reply, terminal, turn, settings): a Tooltip-order area above
/// the badges and labels, solid frame, a title row that drags the card (drop it on a dock target
/// to dock it, drag it again to undock), ✕ and ⛶, an optional search box (Ctrl+F) whose
/// lowercase text the body receives, a corner to resize when floating, edges that snap to the
/// grid when docked, or pinned to fill the screen.
#[allow(clippy::too_many_arguments)]
pub fn floating_card(
    ctx: &egui::Context,
    th: &UiTheme,
    id: &str,
    maxed: bool,
    cs: &mut CardState,
    dock: &mut Option<Pane>,
    grid: DockGrid,
    default_pos: egui::Pos2,
    default_size: egui::Vec2,
    query: Option<&mut String>,
    header: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui, &str),
) -> CardOut {
    let vr = ctx.viewport_rect();
    let margin = 16.0;
    let pad = 24.0; // the frame's inner margin, both sides
    let max_size = vr.size() - egui::vec2(2.0 * margin + pad, 2.0 * margin + pad);
    let docked: Option<egui::Rect> = if maxed { None } else { dock.map(|d| grid.block(d)) };
    let floating_pos = cs.pos.map(|p| egui::pos2(p[0], p[1])).unwrap_or(default_pos);
    let (pos, fixed_size) = if maxed {
        (vr.min + egui::vec2(margin, margin), Some(max_size))
    } else if let Some(r) = docked {
        (r.min, Some(r.size()))
    } else {
        (floating_pos, None)
    };
    let mut out = CardOut::default();
    let find =
        ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) && ctx.memory(|m| m.focused().is_none());
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new(id));
    let can_drag = !maxed;
    let zoom = cs.zoom;
    let content = |ui: &mut egui::Ui, out: &mut CardOut| {
        if (zoom - 1.0).abs() > 0.01 {
            for f in ui.style_mut().text_styles.values_mut() {
                f.size *= zoom;
            }
        }
        // The title row is the drag handle, declared first so the buttons drawn over it win clicks.
        let row_h = 24.0;
        let header_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), row_h));
        if can_drag {
            let drag = ui
                .interact(header_rect, egui::Id::new(id).with("drag"), egui::Sense::drag())
                .on_hover_cursor(egui::CursorIcon::Grab);
            out.dragging = drag.dragged();
            out.drag_delta = drag.drag_delta();
            out.drag_stopped = drag.drag_stopped();
        }
        let mut q_lower = String::new();
        ui.horizontal(|ui| {
            ui.set_min_height(row_h);
            // Title text does not select, so a drag anywhere on this row moves the card.
            ui.style_mut().interaction.selectable_labels = false;
            header(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕  (Esc)").clicked() {
                    out.close = true;
                }
                if ui
                    .small_button(if maxed { "🗗 restore" } else { "⛶ full" })
                    .on_hover_text(if maxed {
                        "Back to the card"
                    } else {
                        "Fill the screen. Floating: drag the corner to resize. Docked: drag an edge."
                    })
                    .clicked()
                {
                    out.toggle_max = true;
                }
                if let Some(q) = query {
                    if !q.is_empty() && ui.small_button("⌫").on_hover_text("Clear the search").clicked() {
                        q.clear();
                    }
                    let resp = ui.add(
                        egui::TextEdit::singleline(q)
                            .id(egui::Id::new((id, "search")))
                            .desired_width(180.0)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("🔍 search  (Ctrl+F)"),
                    );
                    if find {
                        resp.request_focus();
                    }
                    q_lower = q.trim().to_ascii_lowercase();
                }
            });
        });
        ui.separator();
        body(ui, &q_lower);
    };
    let resp = egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(th.card)
                .stroke(egui::Stroke::new(1.5, if cs.dragging { th.warn } else { th.accent }))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 18,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(180),
                })
                .inner_margin(12.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    if let Some(size) = fixed_size {
                        ui.set_width(size.x - pad);
                        ui.set_height(size.y - pad);
                        content(ui, &mut out);
                    } else {
                        egui::Resize::default()
                            .id_salt((id, "size"))
                            .default_size(default_size)
                            .min_size(egui::vec2(320.0, 160.0))
                            .max_size(max_size)
                            .show(ui, |ui| content(ui, &mut out));
                    }
                    // Edges of a docked card: drag to grow or shrink by whole cells.
                    if docked.is_some() && !maxed {
                        let r = ui.min_rect().expand(12.0);
                        let t = 8.0;
                        let strips = [
                            (
                                0u8,
                                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.min.x + t, r.max.y)),
                                egui::CursorIcon::ResizeHorizontal,
                            ),
                            (
                                1u8,
                                egui::Rect::from_min_max(egui::pos2(r.max.x - t, r.min.y), r.right_bottom()),
                                egui::CursorIcon::ResizeHorizontal,
                            ),
                            (
                                2u8,
                                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.max.x, r.min.y + t)),
                                egui::CursorIcon::ResizeVertical,
                            ),
                            (
                                3u8,
                                egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - t), r.right_bottom()),
                                egui::CursorIcon::ResizeVertical,
                            ),
                            (
                                4u8,
                                egui::Rect::from_min_max(
                                    egui::pos2(r.max.x - 2.0 * t, r.max.y - 2.0 * t),
                                    r.right_bottom(),
                                ),
                                egui::CursorIcon::ResizeNwSe,
                            ),
                        ];
                        for (side, strip, cursor) in strips {
                            let e = ui
                                .interact(strip, egui::Id::new(id).with(("edge", side)), egui::Sense::drag())
                                .on_hover_cursor(cursor);
                            if e.dragged() {
                                out.edge = Some(side);
                            }
                        }
                    }
                });
        });
    out.rect = Some(resp.response.rect);
    let pointer = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(pos);

    // ---- the title drag: float it, move it, dock it on a target. A press that has not moved
    // is a click on a header button (egui would call a long press a drag), so motion is required.
    let moved = ctx.input(|i| {
        i.pointer
            .press_origin()
            .zip(i.pointer.latest_pos())
            .is_some_and(|(a, b)| a.distance(b) > 6.0)
    });
    if out.dragging && !cs.dragging && moved {
        ctx.move_to_top(layer);
        cs.dragging = true;
        if dock.is_some() {
            *dock = None;
            cs.pos = Some([pointer.x - default_size.x * 0.5, pointer.y - 12.0]);
        } else if cs.pos.is_none() {
            cs.pos = Some([pos.x, pos.y]);
        }
    }
    if cs.dragging {
        if let Some(p) = cs.pos.as_mut() {
            p[0] = (p[0] + out.drag_delta.x).clamp(vr.min.x, vr.max.x - 160.0);
            p[1] = (p[1] + out.drag_delta.y).clamp(vr.min.y, vr.max.y - 48.0);
        }
        draw_dock_zones(ctx, th, &grid, pointer);
        let released = out.drag_stopped || !ctx.input(|i| i.pointer.primary_down());
        if released {
            cs.dragging = false;
            if let Some(d) = grid.target_at(pointer) {
                *dock = Some(d);
            }
        }
    }

    // ---- edges (and the corner) of a docked card move freely, snapping near grid lines
    if let (Some(side), Some(d)) = (out.edge, dock.as_mut()) {
        let cur = grid.clamp(*d);
        let (fx, fy) = (grid.snap_x(pointer.x), grid.snap_y(pointer.y));
        let min = DockGrid::MIN;
        let next = match side {
            0 => Pane {
                x0: fx.min(cur.x1 - min),
                ..cur
            },
            1 => Pane {
                x1: fx.max(cur.x0 + min),
                ..cur
            },
            2 => Pane {
                y0: fy.min(cur.y1 - min),
                ..cur
            },
            3 => Pane {
                y1: fy.max(cur.y0 + min),
                ..cur
            },
            _ => Pane {
                x1: fx.max(cur.x0 + min),
                y1: fy.max(cur.y0 + min),
                ..cur
            },
        };
        for (axis, from, to) in [
            (0u8, cur.x0, next.x0),
            (0, cur.x1, next.x1),
            (1, cur.y0, next.y0),
            (1, cur.y1, next.y1),
        ] {
            if (from - to).abs() > 1e-6 {
                out.sash.push((axis, from, to));
            }
        }
        *d = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_grid_columns_follow_monitor_seams() {
        let vp = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2000.0, 1000.0));
        let g = DockGrid::new(vp, vp, &[1000.0], 2);
        assert_eq!(g.cols(), 4, "two columns per monitor");
        assert_eq!(g.rows(), 2);
        assert!(g.xs.iter().any(|x| (x - 0.5).abs() < 1e-4), "the seam is a grid line");
        let t = g.targets();
        assert!(
            t.iter()
                .any(|p| (p.x1 - p.x0 - 0.25).abs() < 1e-4 && (p.y1 - p.y0 - 0.5).abs() < 1e-4)
        );
    }

    #[test]
    fn sash_moves_shared_edges_only() {
        let p = Pane {
            x0: 0.0,
            y0: 0.0,
            x1: 0.5,
            y1: 1.0,
        };
        let q = sash_follow(p, 0, 0.5, 0.6);
        assert!((q.x1 - 0.6).abs() < 1e-6);
        let r = sash_follow(p, 0, 0.3, 0.4);
        assert_eq!(r, p);
    }
}
