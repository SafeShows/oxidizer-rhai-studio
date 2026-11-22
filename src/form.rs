//! The form designer: a window drawn as it will look in the game, with its widgets on an 8-pixel grid. Drag a
//! widget to move it, its right edge to change its width, the window's corner to resize it; Delete removes the
//! selected widget. The toolbox and the properties are in the editor's details column; the code is in graph.rs
//! (`set_cursor_pos` puts each widget where it was placed).

use crate::gfx::{self, V2, v2};
use crate::graph::{Form, GRID, TITLE_ROOM, Widget};
use crate::theme;

const MARGIN: f32 = 16.0;
const GRID_LINE: [f32; 4] = [0.16, 0.17, 0.19, 1.0];

enum Drag {
    Move { id: u32, from: V2, start: [i32; 2] },
    Width { id: u32, from: V2, start: i32 },
    Window { from: V2, start: [i32; 2] },
}

#[derive(Default)]
pub struct Designer {
    pub sel: Option<u32>,
    drag: Option<Drag>,
}

fn snap(v: f32) -> i32 {
    ((v / GRID as f32).round() as i32) * GRID
}

/// Whether a widget's width can be changed (buttons, text and checkboxes size to their label).
fn sized(kind: &str) -> bool {
    !matches!(kind, "button" | "text" | "checkbox")
}

/// The width a widget takes in the window: its frame, then its label (ImGui puts labels right of fields).
fn extent(w: &Widget) -> (f32, f32) {
    let label = gfx::text_width(&w.label);
    match w.kind.as_str() {
        "button" => (theme::button_width(&w.label), 0.0),
        "text" => (label, 0.0),
        "checkbox" => (gfx::frame_height(), label + 6.0),
        "progress" | "separator" => (w.width as f32, 0.0),
        _ => (w.width as f32, label + 6.0),
    }
}

/// Adds a widget under the lowest one.
pub fn add(f: &mut Form, id: u32, kind: &str, label: &str, width: i32) {
    let h = snap(gfx::frame_height() + 6.0);
    let y = f.widgets.iter().map(|w| w.pos[1] + h).max().unwrap_or(TITLE_ROOM + GRID).max(TITLE_ROOM + GRID);
    let mut data = std::collections::BTreeMap::new();
    if kind == "slider" {
        data.insert("max".into(), "1".into());
    }
    if kind == "combo" {
        data.insert("items".into(), "First\nSecond\nThird".into());
    }
    f.widgets.push(Widget { id, kind: kind.into(), label: label.into(), pos: [GRID, y], width, data });
    f.size[1] = f.size[1].max(y + h + GRID);
}

impl Designer {
    pub fn draw(&mut self, f: &mut Form) {
        gfx::push_color(gfx::COL_CHILD_BG, [0.07, 0.075, 0.085, 1.0]);
        gfx::begin_child("rhai_studio_designer", v2(0.0, 0.0), gfx::CHILD_BORDERS, gfx::WINDOW_HORIZONTAL_SCROLLBAR);
        gfx::pop_colors(1);
        let room = gfx::avail();
        let start = gfx::cursor_pos();
        let o = v2(start.x + MARGIN, start.y + MARGIN);
        let (w, h) = (f.size[0] as f32, f.size[1] as f32);
        gfx::invisible_button("##designer", v2(room.x.max(w + MARGIN * 2.0), room.y.max(h + MARGIN * 2.0)));
        let hovered = gfx::item_hovered();
        let mouse = gfx::mouse_pos();
        let fh = gfx::frame_height();

        // The window: panel, grid, title bar, resize corner.
        let max = v2(o.x + w, o.y + h);
        gfx::draw_rect(o, max, theme::PANEL, 6.0);
        let mut x = GRID as f32;
        while x < w {
            gfx::draw_line(v2(o.x + x, o.y + TITLE_ROOM as f32), v2(o.x + x, max.y), GRID_LINE);
            x += (GRID * 2) as f32;
        }
        let mut y = TITLE_ROOM as f32;
        while y < h {
            gfx::draw_line(v2(o.x, o.y + y), v2(max.x, o.y + y), GRID_LINE);
            y += (GRID * 2) as f32;
        }
        gfx::draw_rect(o, v2(max.x, o.y + fh + 4.0), theme::SECTION, 6.0);
        gfx::draw_text(v2(o.x + 10.0, o.y + 4.0), theme::TEXT, &f.title);
        gfx::draw_frame(o, max, theme::LINE, 6.0, 1.0);
        gfx::draw_triangle(v2(max.x - 14.0, max.y), max, v2(max.x, max.y - 14.0), theme::FIELD_LINE);

        // Widgets, as ImGui draws them.
        let mut hit = None;
        let mut edge = None;
        for wd in &f.widgets {
            let at = v2(o.x + wd.pos[0] as f32, o.y + wd.pos[1] as f32);
            let (frame, label) = extent(wd);
            let fmax = v2(at.x + frame, at.y + fh);
            match wd.kind.as_str() {
                "text" => gfx::draw_text(v2(at.x, at.y + 3.0), theme::TEXT, &wd.label),
                "button" => {
                    gfx::draw_rect(at, fmax, theme::BUTTON, 3.0);
                    gfx::draw_frame(at, fmax, theme::LINE, 3.0, 1.0);
                    gfx::draw_text(v2(at.x + 9.0, at.y + 3.0), theme::TEXT, &wd.label);
                }
                "separator" => gfx::draw_line(v2(o.x + 8.0, at.y + fh / 2.0), v2(max.x - 8.0, at.y + fh / 2.0), theme::LINE),
                "progress" => {
                    gfx::draw_rect(at, fmax, theme::FIELD, 3.0);
                    gfx::draw_rect(at, v2(at.x + frame * 0.4, fmax.y), theme::AMBER, 3.0);
                }
                kind => {
                    gfx::draw_rect(at, fmax, theme::FIELD, 3.0);
                    gfx::draw_frame(at, fmax, theme::FIELD_LINE, 3.0, 1.0);
                    let inside = match kind {
                        "checkbox" => "",
                        "slider" => "0.000",
                        "input_float" => "0.000000",
                        "input_int" => "0",
                        "combo" => wd.get("items").lines().next().unwrap_or(""),
                        "color" => "R:255 G:255 B:255",
                        _ => wd.get("default"),
                    };
                    gfx::draw_text(v2(at.x + if kind == "slider" { 20.0 } else { 6.0 }, at.y + 3.0), theme::MUTED, inside);
                    if kind == "slider" {
                        gfx::draw_rect(v2(at.x + 2.0, at.y + 2.0), v2(at.x + 14.0, fmax.y - 2.0), theme::FIELD_LINE, 2.0);
                    }
                    if label > 0.0 {
                        gfx::draw_text(v2(fmax.x + 6.0, at.y + 3.0), theme::TEXT, &wd.label);
                    }
                }
            }
            let total = v2(fmax.x + label, fmax.y);
            if self.sel == Some(wd.id) {
                gfx::draw_frame(v2(at.x - 2.0, at.y - 2.0), v2(total.x + 2.0, total.y + 2.0), theme::AMBER, 3.0, 1.5);
                if sized(&wd.kind) {
                    gfx::draw_rect(v2(fmax.x - 3.0, at.y + 4.0), v2(fmax.x + 1.0, fmax.y - 4.0), theme::AMBER, 1.0);
                }
            }
            if hovered && mouse.x >= at.x && mouse.x <= total.x && mouse.y >= at.y && mouse.y <= total.y {
                hit = Some(wd.id);
                edge = (sized(&wd.kind) && (mouse.x - fmax.x).abs() < 5.0).then_some(wd.id);
            }
        }

        if hovered && gfx::mouse_clicked(gfx::LEFT) {
            let corner = mouse.x > max.x - 16.0 && mouse.x < max.x + 4.0 && mouse.y > max.y - 16.0 && mouse.y < max.y + 4.0;
            self.sel = hit;
            self.drag = if corner {
                Some(Drag::Window { from: mouse, start: f.size })
            } else if let Some(id) = edge {
                f.widgets.iter().find(|w| w.id == id).map(|w| Drag::Width { id, from: mouse, start: w.width })
            } else {
                hit.and_then(|id| f.widgets.iter().find(|w| w.id == id)).map(|w| Drag::Move { id: w.id, from: mouse, start: w.pos })
            };
        }
        if !gfx::mouse_down(gfx::LEFT) {
            self.drag = None;
        }
        match &self.drag {
            Some(Drag::Move { id, from, start }) => {
                if let Some(w) = f.widgets.iter_mut().find(|w| w.id == *id) {
                    let x = snap(start[0] as f32 + mouse.x - from.x).clamp(GRID, (f.size[0] - GRID * 2).max(GRID));
                    let y = snap(start[1] as f32 + mouse.y - from.y).clamp(TITLE_ROOM, (f.size[1] - GRID * 3).max(TITLE_ROOM));
                    w.pos = [x, y];
                }
            }
            Some(Drag::Width { id, from, start }) => {
                if let Some(w) = f.widgets.iter_mut().find(|w| w.id == *id) {
                    w.width = snap(*start as f32 + mouse.x - from.x).max(GRID * 4);
                }
            }
            Some(Drag::Window { from, start }) => {
                f.size = [snap(start[0] as f32 + mouse.x - from.x).max(120), snap(start[1] as f32 + mouse.y - from.y).max(TITLE_ROOM + GRID * 4)];
            }
            None => {}
        }
        if gfx::window_focused() && !gfx::any_item_active() && gfx::key_pressed(gfx::KEY_DELETE)
            && let Some(id) = self.sel.take()
        {
            f.widgets.retain(|w| w.id != id);
        }
        gfx::end_child();
    }
}
