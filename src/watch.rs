//! The WATCH panel: Rhai expressions evaluated every frame against the live game, each with its value, and a
//! scrolling graph when it is a number (`ksa::Program::controlled_vehicle().get_barometric_altitude()`). For tuning
//! an autopilot or checking what the physics does. The list is kept in Rhai Studio's settings.

use crate::gfx::{self, v2};
use crate::theme::{AMBER, MUTED, RED, TEXT};
use oxidizer_api::imgui::{self, TextBuffer, text_flags};
use std::collections::VecDeque;

/// About five seconds at 60 fps.
const HISTORY: usize = 300;
const GRAPH_HEIGHT: f32 = 54.0;

struct Item {
    expr: String,
    value: String,
    error: bool,
    history: VecDeque<f64>,
}

#[derive(Default)]
pub struct Watch {
    input: TextBuffer,
    items: Vec<Item>,
}

impl Watch {
    pub fn new(exprs: &[String]) -> Self {
        let mut w = Watch::default();
        for e in exprs {
            w.add(e);
        }
        w
    }

    fn add(&mut self, expr: &str) {
        let expr = expr.trim();
        if !expr.is_empty() && !self.items.iter().any(|i| i.expr == expr) {
            self.items.push(Item { expr: expr.into(), value: String::new(), error: false, history: VecDeque::new() });
        }
    }

    /// The expressions, for the settings.
    pub fn exprs(&self) -> Vec<String> {
        self.items.iter().map(|i| i.expr.clone()).collect()
    }

    /// Evaluates every expression (each frame while Rhai Studio is open, so the graphs keep going in other tabs).
    pub fn sample(&mut self) {
        for it in &mut self.items {
            match oxidizer_api::rhai_eval(&it.expr) {
                Ok(v) => {
                    if let Ok(x) = v.trim().parse::<f64>() {
                        if it.history.len() == HISTORY {
                            it.history.pop_front();
                        }
                        it.history.push_back(x);
                    }
                    (it.value, it.error) = (v, false);
                }
                Err(e) => (it.value, it.error) = (e, true),
            }
        }
    }

    /// Draws the panel. Returns true when the list changed (to save it).
    pub fn draw(&mut self) -> bool {
        let mut changed = false;
        gfx::next_width(-f32::MIN_POSITIVE);
        if imgui::input_text("##watch_new", &mut self.input, text_flags::ENTER_RETURNS_TRUE) {
            let e = self.input.as_str().to_string();
            self.add(&e);
            self.input.set("");
            gfx::refocus_last(); // keep typing: the next expression
            changed = true;
        }
        imgui::text_colored(MUTED, if self.items.is_empty() { "Type a Rhai expression and press Enter" } else { "Enter adds; numbers get a graph" });
        let mut remove = None;
        gfx::begin_child("rhai_studio_watch_list", v2(0.0, 0.0), gfx::CHILD_BORDERS, 0);
        for (i, it) in self.items.iter().enumerate() {
            if gfx::button_sized(&format!("x##w{i}"), v2(0.0, 0.0)) {
                remove = Some(i);
            }
            imgui::same_line();
            imgui::text_colored(TEXT, &it.expr);
            if it.error {
                gfx::push_color(gfx::COL_TEXT, RED);
                gfx::text_wrapped(&it.value);
                gfx::pop_colors(1);
            } else {
                imgui::text_colored(AMBER, &it.value);
            }
            if it.history.len() >= 2 {
                graph(&it.history);
            }
            imgui::separator();
        }
        gfx::end_child();
        if let Some(i) = remove {
            self.items.remove(i);
            changed = true;
        }
        changed
    }
}

/// A line graph of the history, as wide as the panel, with its range.
fn graph(h: &VecDeque<f64>) {
    let at = gfx::cursor_pos();
    let w = gfx::avail().x.max(40.0);
    let (lo, hi) = h.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    let span = if hi > lo { hi - lo } else { 1.0 };
    gfx::draw_rect(at, v2(at.x + w, at.y + GRAPH_HEIGHT), [0.04, 0.045, 0.05, 1.0], 3.0);
    let point = |i: usize, x: f64| {
        v2(at.x + w * i as f32 / (HISTORY - 1) as f32, at.y + GRAPH_HEIGHT - 4.0 - (GRAPH_HEIGHT - 8.0) * ((x - lo) / span) as f32)
    };
    let start = HISTORY - h.len(); // the newest value at the right edge
    for (i, (a, b)) in h.iter().zip(h.iter().skip(1)).enumerate() {
        gfx::draw_line(point(start + i, *a), point(start + i + 1, *b), AMBER);
    }
    gfx::draw_text(v2(at.x + 4.0, at.y + 2.0), MUTED, &format!("{hi:.4}"));
    gfx::draw_text(v2(at.x + 4.0, at.y + GRAPH_HEIGHT - gfx::line_height() - 2.0), MUTED, &format!("{lo:.4}"));
    gfx::dummy(v2(w, GRAPH_HEIGHT));
}
