//! Rhai Studio: write Oxidizer's Rhai mods inside the game. Open a mod's scripts in tabs, save with Ctrl+S and
//! Oxidizer reloads them within a second, make new mods, and try code in a console against the live game.
//! An Oxidizer mod in Rust: https://docs.rs/oxidizer_api
//!
//! - editor.rs: the code editor with syntax colors (highlight.rs colors Rhai)
//! - files.rs: finding Rhai mods, open documents, new mods
//! - console.rs: the log and the console line
//! - gfx.rs: ImGui drawing the API doesn't wrap
//! - this file: the window, styled after the game's own panels

mod console;
mod editor;
mod files;
mod gfx;
mod highlight;

use console::Console;
use files::{Doc, RhaiMod};
use gfx::v2;
use oxidizer_api::imgui::{self, TextBuffer, tab_flags, text_flags};
use oxidizer_api::{Mod, ModStatus, config_path, key_pressed, log};

// The game's panel colors: green and red filled buttons with dark text, grey labels, dark frames.
const GREEN: [f32; 4] = [0.33, 0.80, 0.36, 1.0];
const RED: [f32; 4] = [0.80, 0.22, 0.18, 1.0];
const AMBER: [f32; 4] = [0.93, 0.78, 0.30, 1.0];
const DARK: [f32; 4] = [0.16, 0.18, 0.20, 1.0];
const DARK_HOVER: [f32; 4] = [0.24, 0.27, 0.30, 1.0];
const INK: [f32; 4] = [0.05, 0.06, 0.07, 1.0];
const LABEL: [f32; 4] = [0.62, 0.66, 0.70, 1.0];
const TEXT: [f32; 4] = [0.92, 0.93, 0.95, 1.0];
const SIDEBAR_WIDTH: f32 = 230.0;
/// Height of the log and console under the editor.
const OUTPUT_HEIGHT: f32 = 170.0;

struct Studio {
    visible: bool,
    mods: Vec<RhaiMod>,
    statuses: Vec<ModStatus>,
    docs: Vec<Doc>,
    /// The tab to bring to the front next frame (a file just opened).
    select: Option<usize>,
    /// The tab shown this frame.
    active: Option<usize>,
    new_name: TextBuffer,
    new_error: Option<String>,
    console: Console,
    frames: u64,
}

/// A button in the game's style: dark, or filled with `fill` and dark text.
fn button(label: &str, fill: Option<[f32; 4]>) -> bool {
    let (bg, fg) = fill.map_or((DARK, TEXT), |c| (c, INK));
    let hover = fill.map_or(DARK_HOVER, |c| [c[0] * 1.12, c[1] * 1.12, c[2] * 1.12, 1.0]);
    gfx::push_color(gfx::COL_BUTTON, bg);
    gfx::push_color(gfx::COL_BUTTON_HOVERED, hover);
    gfx::push_color(gfx::COL_BUTTON_ACTIVE, hover);
    gfx::push_color(gfx::COL_TEXT, fg);
    let clicked = imgui::button(label);
    gfx::pop_colors(4);
    clicked
}

/// A section label, like the game's panel headings.
fn heading(s: &str) {
    imgui::text_colored(LABEL, s);
}

impl Mod for Studio {
    fn init() -> Self {
        log("rhai_studio: F9 opens Rhai Studio");
        Studio {
            visible: load_visible(),
            mods: files::rhai_mods(),
            statuses: vec![],
            docs: vec![],
            select: None,
            active: None,
            new_name: TextBuffer::default(),
            new_error: None,
            console: Console::new(),
            frames: 0,
        }
    }

    fn frame(&mut self, _t: f64, _dt: f64) {
        if key_pressed("F9") {
            self.toggle();
        }
        self.frames += 1;
        if self.frames.is_multiple_of(60) && self.visible {
            // About once a second: mods added by hand, and files changed in another editor.
            self.mods = files::rhai_mods();
            self.docs.iter_mut().for_each(Doc::follow_disk);
        }
    }

    fn menu(&mut self) {
        if imgui::menu_item("Rhai Studio  (F9)", self.visible) {
            self.toggle();
        }
    }

    fn gui(&mut self, _dt: f64) {
        if !self.visible {
            return;
        }
        self.statuses = oxidizer_api::mod_statuses();
        imgui::set_next_window_size(1100.0, 720.0);
        gfx::push_var(gfx::VAR_FRAME_ROUNDING, 3.0);
        let mut open = true;
        if imgui::begin_closable("RHAI STUDIO", &mut open) {
            let top = gfx::avail().y - OUTPUT_HEIGHT - 50.0;
            gfx::begin_child("rhai_studio_mods", v2(SIDEBAR_WIDTH, top), gfx::CHILD_BORDERS, 0);
            self.draw_sidebar();
            gfx::end_child();
            imgui::same_line();
            gfx::begin_child("rhai_studio_main", v2(0.0, top), 0, 0);
            self.draw_tabs();
            gfx::end_child();
            heading("CONSOLE");
            self.console.draw(OUTPUT_HEIGHT);
        }
        imgui::end();
        gfx::pop_vars(1);
        if !open {
            self.toggle();
        }
        if imgui::ctrl_shortcut('s')
            && let Some(d) = self.active.and_then(|i| self.docs.get_mut(i)) {
                d.save();
            }
    }
}

impl Studio {
    fn toggle(&mut self) {
        self.visible = !self.visible;
        let _ = std::fs::write(config_path("rhai_studio"), format!("visible = {}\n", self.visible));
    }

    fn status(&self, id: &str) -> Option<&ModStatus> {
        self.statuses.iter().find(|s| s.id == id && s.kind == "Rhai")
    }

    fn open(&mut self, path: &std::path::Path, mod_id: &str) {
        let i = match self.docs.iter().position(|d| d.path == path) {
            Some(i) => i,
            None => {
                self.docs.push(Doc::open(path, mod_id));
                self.docs.len() - 1
            }
        };
        self.select = Some(i);
    }

    fn draw_sidebar(&mut self) {
        heading("MODS");
        let current = self.active.and_then(|i| self.docs.get(i)).map(|d| d.path.clone());
        let mut to_open = None;
        for m in &self.mods {
            let dot = match self.status(&m.id) {
                Some(s) if s.problem.is_some() => RED,
                Some(_) => GREEN,
                None => LABEL,
            };
            imgui::text_colored(dot, "●");
            imgui::same_line();
            imgui::text(&m.id);
            for f in &m.files {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if gfx::selectable(&format!("    {name}##{}", f.display()), current.as_deref() == Some(f.as_path())) {
                    to_open = Some((f.clone(), m.id.clone()));
                }
            }
        }
        if self.mods.is_empty() {
            imgui::text_colored(LABEL, "No Rhai mods yet.");
        }
        if let Some((path, id)) = to_open {
            self.open(&path, &id);
        }

        gfx::dummy(v2(0.0, 8.0));
        imgui::separator();
        heading("NEW MOD");
        let enter = imgui::input_text("##new_mod", &mut self.new_name, text_flags::ENTER_RETURNS_TRUE);
        if button("CREATE", Some(GREEN)) || enter {
            let id = self.new_name.as_str().trim().to_string();
            match files::new_mod(&id) {
                Ok(main) => {
                    log(&format!("rhai_studio: made mods/{id}; Oxidizer starts it within a second"));
                    self.new_name.set("");
                    self.new_error = None;
                    self.mods = files::rhai_mods();
                    self.open(&main, &id);
                }
                Err(e) => self.new_error = Some(e),
            }
        }
        if let Some(e) = &self.new_error {
            imgui::text_colored(RED, e);
        }
    }

    fn draw_tabs(&mut self) {
        self.active = None;
        if self.docs.is_empty() {
            heading("EDITOR");
            imgui::text_colored(LABEL, "Pick a script on the left, or make a new mod.");
            return;
        }
        let mut close = None;
        if imgui::begin_tab_bar("rhai_studio_tabs") {
            let select = self.select.take();
            for i in 0..self.docs.len() {
                let mut keep = true;
                let mut flags = if self.docs[i].dirty() { tab_flags::UNSAVED_DOCUMENT } else { 0 };
                if select == Some(i) {
                    flags |= tab_flags::SET_SELECTED;
                }
                let label = format!("{}##{}", self.docs[i].title, self.docs[i].path.display());
                if imgui::begin_tab_item(&label, Some(&mut keep), flags) {
                    self.active = Some(i);
                    self.draw_doc(i);
                    imgui::end_tab_item();
                }
                if !keep {
                    close = Some(i);
                }
            }
            imgui::end_tab_bar();
        }
        if let Some(i) = close {
            // ponytail: closing a tab with unsaved edits drops them; ask first if people lose work this way.
            self.docs.remove(i);
        }
    }

    fn draw_doc(&mut self, i: usize) {
        let status = self.status(&self.docs[i].mod_id).cloned();
        let d = &mut self.docs[i];
        let dirty = d.dirty();
        if button("SAVE", dirty.then_some(GREEN)) {
            d.save();
        }
        imgui::same_line();
        if button("REVERT", None) {
            d.revert();
        }
        imgui::same_line();
        // The mod's state, as a filled label like the game's ENGINE / SHUTDN indicators.
        match &status {
            Some(s) if s.problem.is_some() => button("ERROR", Some(RED)),
            Some(_) => button("RUNNING", Some(GREEN)),
            None => button("NOT LOADED", Some(AMBER)),
        };
        imgui::same_line();
        let lines = d.text.as_str().lines().count();
        imgui::text_colored(LABEL, &format!("{lines} lines  {}  Ctrl+S saves", if dirty { "MODIFIED" } else { "SAVED" }));
        if let Some(e) = &d.error {
            imgui::text_colored(RED, e);
        }
        if let Some(p) = status.as_ref().and_then(|s| s.problem.as_deref()) {
            imgui::text_colored(RED, p);
        }
        editor::code_editor("rhai_studio_editor", &mut d.text, v2(0.0, 0.0));
    }
}

fn load_visible() -> bool {
    std::fs::read_to_string(config_path("rhai_studio"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("visible")?.as_bool())
        .unwrap_or(true)
}

oxidizer_api::oxidizer_mod!(Studio);
