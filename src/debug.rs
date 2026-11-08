//! Breakpoints: set from the editor's gutter, shown in the DEBUG panel. Oxidizer (0.11 or later) does the work: a
//! mod that reaches a breakpoint leaves a snapshot (variables, `this`, the call stack) and pauses, while the game
//! keeps running, until CONTINUE. This talks to it through the console functions `debug_breakpoints`,
//! `debug_state` and `debug_continue`.

use crate::gfx::{self, v2};
use crate::theme::{self, AMBER, MUTED, RED, TEXT};
use oxidizer_api::imgui;
use serde_json::Value;

/// How often (in frames) the state is read while scripts are open.
const POLL_EVERY: u64 = 15;

#[derive(Default)]
pub struct Debug {
    /// The last `debug_state()`: `{"breakpoints": {source: [lines]}, "hits": [...]}`.
    state: Value,
    /// Why breakpoints don't work (an Oxidizer without them), once someone tried to set one.
    pub problem: Option<String>,
    /// Oxidizer has no breakpoints: stop asking.
    unsupported: bool,
}

const TOO_OLD: &str = "Breakpoints need Oxidizer 0.11 or later.";

fn rhai_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default() // JSON's escapes are Rhai's too
}

impl Debug {
    /// Reads the state now and then (`frame` counts frames).
    pub fn poll(&mut self, frame: u64) {
        if !frame.is_multiple_of(POLL_EVERY) || self.unsupported {
            return;
        }
        match oxidizer_api::rhai_eval("debug_state()") {
            Ok(text) => self.state = serde_json::from_str(&text).unwrap_or(Value::Null),
            Err(_) => self.unsupported = true,
        }
    }

    /// The breakpoint lines of a script ("mod/file.rhai").
    pub fn breakpoints(&self, source: &str) -> Vec<usize> {
        self.state["breakpoints"][source].as_array().map(|a| a.iter().filter_map(Value::as_u64).map(|l| l as usize).collect()).unwrap_or_default()
    }

    /// Adds or removes a breakpoint.
    pub fn toggle(&mut self, source: &str, line: usize) {
        let mut lines = self.breakpoints(source);
        match lines.iter().position(|&l| l == line) {
            Some(i) => {
                lines.remove(i);
            }
            None => lines.push(line),
        }
        lines.sort_unstable();
        let list = lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
        match oxidizer_api::rhai_eval(&format!("debug_breakpoints({}, [{list}])", rhai_str(source))) {
            Ok(_) => self.state["breakpoints"][source] = lines.into(), // until the next poll
            Err(_) => (self.unsupported, self.problem) = (true, Some(TOO_OLD.into())),
        }
    }

    /// Removes a script's breakpoints (its tab was closed) and lets its mod run on.
    pub fn clear(&mut self, source: &str, mod_id: &str) {
        if !self.unsupported && !self.breakpoints(source).is_empty() {
            let _ = oxidizer_api::rhai_eval(&format!("debug_breakpoints({}, []); debug_continue({})", rhai_str(source), rhai_str(mod_id)));
        }
    }

    /// The line a paused mod stopped at in this script, if one did.
    pub fn paused_at(&self, source: &str) -> Option<usize> {
        self.hits().find(|h| h["source"] == source && h["paused"] == true).and_then(|h| h["line"].as_u64()).map(|l| l as usize)
    }

    fn hits(&self) -> impl Iterator<Item = &Value> {
        self.state["hits"].as_array().into_iter().flatten()
    }

    /// Whether there is anything to show (a breakpoint was hit).
    pub fn active(&self) -> bool {
        self.hits().next().is_some()
    }

    /// The DEBUG panel: each breakpoint that was hit, with what it saw.
    pub fn draw(&mut self) {
        if let Some(p) = &self.problem {
            imgui::text_colored(RED, p);
            return;
        }
        let mut resume = None;
        for (i, h) in self.hits().enumerate() {
            let paused = h["paused"] == true;
            let at = format!("{} line {}", h["source"].as_str().unwrap_or(""), h["line"]);
            imgui::text_colored(if paused { AMBER } else { MUTED }, if paused { "PAUSED" } else { "RUNNING" });
            imgui::same_line();
            imgui::text(&at);
            imgui::same_line();
            imgui::text_colored(MUTED, &format!("hit {}x", h["count"]));
            if paused && theme::primary_button(&format!("CONTINUE##{i}"), v2(theme::button_width("CONTINUE") + 16.0, 0.0)) {
                resume = h["mod"].as_str().map(str::to_string);
            }
            let stack: Vec<&str> = h["stack"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !stack.is_empty() {
                imgui::text_colored(MUTED, &format!("in {}", stack.join(" < ")));
            }
            for v in h["vars"].as_array().into_iter().flatten() {
                row(v[0].as_str().unwrap_or(""), v[2].as_str().unwrap_or(""), v[1].as_str());
            }
            for v in h["this"].as_array().into_iter().flatten() {
                row(&format!("this.{}", v[0].as_str().unwrap_or("")), v[1].as_str().unwrap_or(""), None);
            }
            imgui::separator();
        }
        if let Some(m) = resume {
            let _ = oxidizer_api::rhai_eval(&format!("debug_continue({})", rhai_str(&m)));
        }
    }
}

/// `name = value` (and its type, muted).
fn row(name: &str, value: &str, ty: Option<&str>) {
    imgui::text_colored(TEXT, name);
    imgui::same_line();
    gfx::text_wrapped(&match ty {
        Some(t) => format!("= {value}   ({t})"),
        None => format!("= {value}"),
    });
}
