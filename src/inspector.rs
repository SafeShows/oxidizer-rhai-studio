//! The INSPECT panel: the live game, object by object. Start from any Rhai expression (the controlled vehicle by
//! default), click an object to go into it, UP to come back. Values refresh a few times a second; click a number,
//! bool or string that can be set to change it in the running game. Uses Oxidizer's `inspect()` (0.12 or later).

use crate::gfx::{self, v2};
use crate::theme::{AMBER, MUTED, RED};
use oxidizer_api::imgui::{self, TextBuffer, text_flags};
use serde_json::Value;

const REFRESH_EVERY: u64 = 20;
const DEFAULT_ROOT: &str = "ksa::Program::controlled_vehicle()";

enum Step {
    Member(String),
    Index(usize),
}

pub struct Inspector {
    root: TextBuffer,
    path: Vec<Step>,
    data: Value,
    error: Option<String>,
    /// The member being edited, with its text.
    editing: Option<(String, TextBuffer)>,
    stale: bool,
}

impl Default for Inspector {
    fn default() -> Self {
        Inspector::new(DEFAULT_ROOT)
    }
}

fn rhai_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

impl Inspector {
    pub fn new(root: &str) -> Self {
        Inspector { root: TextBuffer::new(root), path: vec![], data: Value::Null, error: None, editing: None, stale: true }
    }

    pub fn root(&self) -> String {
        self.root.as_str().trim().to_string()
    }

    /// The Rhai expression for the object shown.
    fn expr(&self) -> String {
        let mut e = format!("({})", self.root());
        for s in &self.path {
            match s {
                Step::Member(m) => e += &format!(".get({})", rhai_str(m)),
                Step::Index(i) => e += &format!("[{i}]"),
            }
        }
        e
    }

    fn refresh(&mut self) {
        self.stale = false;
        match oxidizer_api::rhai_eval(&format!("inspect({})", self.expr())) {
            Ok(text) => {
                self.data = serde_json::from_str(&text).unwrap_or(Value::Null);
                self.error = None;
            }
            Err(e) if e.contains("Function not found: inspect") => self.error = Some("The inspector needs Oxidizer 0.12 or later.".into()),
            Err(e) => self.error = Some(e),
        }
    }

    fn set(&mut self, name: &str, literal: &str) {
        if let Err(e) = oxidizer_api::rhai_eval(&format!("{}.set({}, {literal})", self.expr(), rhai_str(name))) {
            self.error = Some(e);
        }
        self.stale = true;
    }

    pub fn draw(&mut self, frame: u64) {
        if self.stale || frame.is_multiple_of(REFRESH_EVERY) {
            self.refresh();
        }
        gfx::next_width(-f32::MIN_POSITIVE);
        if imgui::input_text("##inspect_root", &mut self.root, text_flags::ENTER_RETURNS_TRUE) {
            self.path.clear();
            self.stale = true;
        }
        if !self.path.is_empty() {
            if gfx::button_sized("UP", v2(0.0, 0.0)) {
                self.path.pop();
                self.editing = None;
                self.stale = true;
            }
            imgui::same_line();
            let trail: String = self.path.iter().map(|s| match s {
                Step::Member(m) => format!(".{m}"),
                Step::Index(i) => format!("[{i}]"),
            }).collect();
            imgui::text_colored(MUTED, &trail);
        } else {
            imgui::text_colored(MUTED, "Any Rhai expression; Enter shows it");
        }
        if let Some(e) = &self.error {
            gfx::push_color(gfx::COL_TEXT, RED);
            gfx::text_wrapped(e);
            gfx::pop_colors(1);
            return;
        }
        imgui::text_colored(AMBER, self.data["type"].as_str().unwrap_or(""));
        if let Some(n) = self.data["count"].as_u64() {
            imgui::same_line();
            imgui::text_colored(MUTED, &format!("{n} items"));
        }

        let mut go = None;
        let mut set = None;
        gfx::begin_child("rhai_studio_inspect_list", v2(0.0, 0.0), gfx::CHILD_BORDERS, 0);
        for (i, m) in self.data["members"].as_array().into_iter().flatten().enumerate() {
            let name = m["name"].as_str().unwrap_or("");
            let kind = m["kind"].as_str().unwrap_or("");
            let value = m["value"].as_str().unwrap_or("");
            let label = format!("{}{name}: {} = {value}", if m["static"] == true { "static " } else { "" }, m["type"].as_str().unwrap_or(""));
            let editable = m["writable"] == true && matches!(kind, "bool" | "int" | "float" | "string");
            if let Some((editing, buf)) = &mut self.editing
                && editing == name
            {
                imgui::text(name);
                imgui::same_line();
                gfx::next_width(-f32::MIN_POSITIVE);
                if imgui::input_text(&format!("##edit{i}"), buf, text_flags::ENTER_RETURNS_TRUE) {
                    let t = buf.as_str().trim().to_string();
                    let literal = match kind {
                        "string" => Some(rhai_str(&t)),
                        _ => t.parse::<f64>().ok().map(|v| format!("{v:?}")),
                    };
                    set = literal.map(|l| (name.to_string(), l));
                }
                continue;
            }
            if gfx::selectable(&format!("{label}##m{i}"), false) {
                match kind {
                    "object" => go = Some(Step::Member(name.to_string())),
                    "bool" if editable => set = Some((name.to_string(), (value != "True" && value != "true").to_string())),
                    _ if editable => self.editing = Some((name.to_string(), TextBuffer::new(value))),
                    _ => {}
                }
            }
        }
        for it in self.data["items"].as_array().into_iter().flatten() {
            let i = it["index"].as_u64().unwrap_or(0) as usize;
            if gfx::selectable(&format!("[{i}] {}##item{i}", it["value"].as_str().unwrap_or("")), false) && it["kind"] == "object" {
                go = Some(Step::Index(i));
            }
        }
        gfx::end_child();
        if let Some(step) = go {
            self.path.push(step);
            self.editing = None;
            self.stale = true;
        }
        if let Some((name, literal)) = set {
            self.editing = None;
            self.set(&name, &literal);
        }
    }
}
