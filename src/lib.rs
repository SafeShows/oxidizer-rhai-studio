//! Rhai Studio: write Oxidizer's Rhai mods inside the game. Open a mod's scripts in tabs, save with Ctrl+S and
//! Oxidizer reloads them within a second, make new mods, and try code in a console against the live game.
//! An Oxidizer mod in Rust: https://docs.rs/oxidizer_api
//!
//! - files.rs: finding Rhai mods, open documents, new mods
//! - console.rs: the log and the console line
//! - this file: the window

mod console;
mod files;

use console::Console;
use files::{Doc, RhaiMod};
use oxidizer_api::imgui::{self, TextBuffer, tab_flags, text_flags};
use oxidizer_api::{Mod, ModStatus, config_path, key_pressed, log};

const GREEN: [f32; 4] = [0.5, 0.9, 0.5, 1.0];
const RED: [f32; 4] = [1.0, 0.45, 0.4, 1.0];
const GREY: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
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
        imgui::set_next_window_size(900.0, 680.0);
        let mut open = true;
        if imgui::begin_closable("Rhai Studio", &mut open) {
            self.draw_mods();
            self.draw_tabs();
            imgui::separator();
            self.console.draw(OUTPUT_HEIGHT);
        }
        imgui::end();
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

    /// Why a mod isn't running, if it isn't.
    fn problem(&self, id: &str) -> Option<&str> {
        self.statuses.iter().find(|s| s.id == id && s.kind == "Rhai").and_then(|s| s.problem.as_deref())
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

    fn draw_mods(&mut self) {
        if !imgui::collapsing_header("Rhai mods") {
            return;
        }
        let mut to_open = None;
        for m in &self.mods {
            match self.problem(&m.id) {
                None if self.statuses.iter().any(|s| s.id == m.id) => imgui::text_colored(GREEN, "running "),
                None => imgui::text_colored(GREY, "not loaded"),
                Some(_) => imgui::text_colored(RED, "error   "),
            }
            imgui::same_line();
            imgui::text(&m.id);
            for f in &m.files {
                imgui::same_line();
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if imgui::button(&format!("{name}##{}", f.display())) {
                    to_open = Some((f.clone(), m.id.clone()));
                }
            }
        }
        if let Some((path, id)) = to_open {
            self.open(&path, &id);
        }

        imgui::text("New mod:");
        imgui::same_line();
        let enter = imgui::input_text("##new_mod", &mut self.new_name, text_flags::ENTER_RETURNS_TRUE);
        imgui::same_line();
        if imgui::button("Create") || enter {
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
        imgui::separator();
    }

    fn draw_tabs(&mut self) {
        if self.docs.is_empty() {
            imgui::text_colored(GREY, "Open a script above, or make a new mod.");
            return;
        }
        self.active = None;
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
        let problem = self.problem(&self.docs[i].mod_id).map(String::from);
        let running = self.statuses.iter().any(|s| s.id == self.docs[i].mod_id);
        let d = &mut self.docs[i];
        if imgui::button("Save") {
            d.save();
        }
        imgui::same_line();
        if imgui::button("Revert") {
            d.revert();
        }
        imgui::same_line();
        let lines = d.text.as_str().lines().count();
        imgui::text_colored(GREY, &format!("{lines} lines, {}  (Ctrl+S saves)", if d.dirty() { "modified" } else { "saved" }));
        if let Some(e) = &d.error {
            imgui::text_colored(RED, e);
        }
        match (problem, running) {
            (Some(p), _) => imgui::text_colored(RED, &p),
            (None, true) => imgui::text_colored(GREEN, &format!("{} is running", d.mod_id)),
            (None, false) => imgui::text_colored(GREY, &format!("{} is not loaded yet", d.mod_id)),
        }
        // The text box fills the tab, leaving room for the log and console below it.
        imgui::input_text_multiline("##editor", &mut d.text, -f32::MIN_POSITIVE, -(OUTPUT_HEIGHT + 40.0), text_flags::ALLOW_TAB_INPUT);
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
