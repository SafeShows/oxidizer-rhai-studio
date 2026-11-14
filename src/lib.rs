//! Rhai Studio: write Oxidizer's Rhai mods inside the game. Open a mod's scripts in tabs, save with Ctrl+S and
//! Oxidizer reloads them within a second, make new mods, and try code in a console against the live game.
//! An Oxidizer mod in Rust: https://docs.rs/oxidizer_api
//!
//! - editor.rs: the code editor with syntax colors (highlight.rs colors Rhai) and autocomplete
//! - debug.rs: breakpoints and the DEBUG panel (Oxidizer pauses the mod)
//! - inspector.rs: the INSPECT panel (the live game, object by object); watch.rs: WATCH (values and graphs)
//! - api.rs: what scripts can call, from Oxidizer's definition files; explorer.rs: the API panel
//! - files.rs: finding Rhai mods, open documents, new mods
//! - console.rs: the log and the console line
//! - theme.rs: the look of the game's own dialogs
//! - gfx.rs: ImGui drawing the API doesn't wrap
//! - this file: the window

mod api;
mod console;
mod debug;
mod editor;
mod explorer;
mod files;
mod gfx;
mod highlight;
mod inspector;
mod theme;
mod watch;

use console::Console;
use explorer::Explorer;
use files::{Doc, RhaiMod};
use gfx::v2;
use oxidizer_api::imgui::{self, TextBuffer, tab_flags, text_flags};
use oxidizer_api::{Mod, ModStatus, config_path, key_pressed, log};
use theme::{GREEN, MUTED, RED};

const TITLE: &str = "RHAI STUDIO";
const TITLE_HEIGHT: f32 = 34.0;
const FOOTER_HEIGHT: f32 = 50.0;
const SIDEBAR_WIDTH: f32 = 240.0;
/// Height of the console drawer when open.
const DRAWER_HEIGHT: f32 = 200.0;
const EXPLORER_WIDTH: f32 = 360.0;

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
    /// The console drawer: whether it is open, and its height now (it slides between 0 and DRAWER_HEIGHT).
    drawer_open: bool,
    drawer: f32,
    /// The panels on the right (DEBUG opens by itself when a breakpoint is hit).
    api_open: bool,
    inspect_open: bool,
    watch_open: bool,
    explorer: Explorer,
    debug: debug::Debug,
    inspector: inspector::Inspector,
    watch: watch::Watch,
    frames: u64,
}

impl Mod for Studio {
    fn init() -> Self {
        log("rhai_studio: F9 opens Rhai Studio");
        let s = load_settings();
        let flag = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let (visible, drawer_open) = (flag("visible", true), flag("console", true));
        let watches: Vec<String> = s.get("watches").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
        let root = s.get("inspect_root").and_then(|v| v.as_str()).map(String::from);
        std::thread::spawn(api::load);
        Studio {
            visible,
            mods: files::rhai_mods(),
            statuses: vec![],
            docs: vec![],
            select: None,
            active: None,
            new_name: TextBuffer::default(),
            new_error: None,
            console: Console::new(),
            drawer_open,
            drawer: if drawer_open { DRAWER_HEIGHT } else { 0.0 },
            api_open: flag("api", false),
            inspect_open: flag("inspect", false),
            watch_open: flag("watch", false),
            explorer: Explorer::default(),
            debug: debug::Debug::default(),
            inspector: root.map(|r| inspector::Inspector::new(&r)).unwrap_or_default(),
            watch: watch::Watch::new(&watches),
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

    fn gui(&mut self, dt: f64) {
        if !self.visible {
            return;
        }
        self.statuses = oxidizer_api::mod_statuses();
        if !self.docs.is_empty() {
            self.debug.poll(self.frames);
        }
        self.watch.sample();
        let target = if self.drawer_open { DRAWER_HEIGHT } else { 0.0 };
        self.drawer += (target - self.drawer) * (dt as f32 * 14.0).min(1.0);
        if (target - self.drawer).abs() < 0.5 {
            self.drawer = target;
        }

        theme::push();
        imgui::set_next_window_size(1100.0, 720.0);
        if gfx::begin_window(TITLE, gfx::WINDOW_NO_TITLE_BAR | gfx::WINDOW_NO_COLLAPSE | gfx::WINDOW_NO_SCROLLBAR) {
            let (pos, size) = (gfx::window_pos(), gfx::window_size());
            // Title: centered, over a line across the window, like the game's dialogs.
            gfx::draw_text(v2(pos.x + (size.x - gfx::text_width(TITLE)) / 2.0, pos.y + 9.0), theme::TEXT, TITLE);
            gfx::draw_line(v2(pos.x, pos.y + TITLE_HEIGHT), v2(pos.x + size.x, pos.y + TITLE_HEIGHT), theme::LINE);
            // Close: hides the window; Mods > Rhai Studio (or F9) opens it again, with its tabs as they were.
            gfx::set_cursor_pos(v2(pos.x + size.x - 34.0, pos.y + 5.0));
            let close = theme::close_button(v2(26.0, 24.0));
            gfx::set_cursor_pos(v2(pos.x + 12.0, pos.y + TITLE_HEIGHT + 10.0));

            let body = gfx::avail().y - self.drawer - FOOTER_HEIGHT;
            gfx::begin_child("rhai_studio_side", v2(SIDEBAR_WIDTH, body), 0, 0);
            self.draw_sidebar();
            gfx::end_child();
            imgui::same_line();
            // The right column: the open panels, as tabs when there are several (DEBUG first: it opens by
            // itself when a breakpoint is hit).
            let debugging = self.debug.active() || self.debug.problem.is_some();
            let panels: Vec<&str> = [("DEBUG", debugging), ("INSPECT", self.inspect_open), ("WATCH", self.watch_open), ("API", self.api_open)]
                .into_iter()
                .filter_map(|(name, open)| open.then_some(name))
                .collect();
            let main_width = if panels.is_empty() { 0.0 } else { -(EXPLORER_WIDTH + 8.0) };
            gfx::begin_child("rhai_studio_main", v2(main_width, body), 0, 0);
            self.draw_tabs();
            gfx::end_child();
            if !panels.is_empty() {
                imgui::same_line();
                gfx::begin_child("rhai_studio_side_right", v2(0.0, body), 0, 0);
                let several = panels.len() > 1;
                let tabs = several && imgui::begin_tab_bar("rhai_studio_side_tabs");
                for name in panels {
                    if several && !(tabs && imgui::begin_tab_item(name, None, 0)) {
                        continue;
                    }
                    self.draw_panel(name);
                    if several {
                        imgui::end_tab_item();
                    }
                }
                if tabs {
                    imgui::end_tab_bar();
                }
                gfx::end_child();
            }
            if self.drawer > 40.0 {
                self.console.draw(self.drawer - 40.0);
            }
            self.draw_footer(pos, size);
            if close {
                self.toggle();
            }
        }
        imgui::end();
        theme::pop();
        if imgui::ctrl_shortcut('s')
            && let Some(d) = self.active.and_then(|i| self.docs.get_mut(i))
        {
            d.save();
        }
    }
}

impl Studio {
    fn toggle(&mut self) {
        self.visible = !self.visible;
        self.save_settings();
    }

    fn save_settings(&self) {
        let mut t = toml::Table::new();
        for (k, v) in [("visible", self.visible), ("console", self.drawer_open), ("api", self.api_open), ("inspect", self.inspect_open), ("watch", self.watch_open)] {
            t.insert(k.into(), v.into());
        }
        t.insert("inspect_root".into(), self.inspector.root().into());
        t.insert("watches".into(), self.watch.exprs().into());
        let _ = std::fs::write(config_path("rhai_studio"), t.to_string());
    }

    fn draw_panel(&mut self, name: &str) {
        match name {
            "DEBUG" => {
                gfx::begin_child("rhai_studio_debug", v2(0.0, 0.0), gfx::CHILD_BORDERS, 0);
                self.debug.draw();
                gfx::end_child();
            }
            "INSPECT" => self.inspector.draw(self.frames),
            "WATCH" => {
                if self.watch.draw() {
                    self.save_settings();
                }
            }
            _ => {
                if let Some(code) = self.explorer.draw()
                    && let Some(d) = self.active.and_then(|i| self.docs.get_mut(i))
                {
                    d.text.insert(&code);
                }
            }
        }
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

    /// The darker band at the bottom: the console toggle and the newest log line on the left, REVERT and the
    /// amber SAVE on the right.
    fn draw_footer(&mut self, pos: gfx::V2, size: gfx::V2) {
        let top = pos.y + size.y - FOOTER_HEIGHT;
        gfx::draw_rect(v2(pos.x + 1.0, top), v2(pos.x + size.x - 1.0, pos.y + size.y - 1.0), theme::FOOTER, 6.0);
        gfx::draw_line(v2(pos.x, top), v2(pos.x + size.x, top), theme::LINE);
        let h = gfx::line_height() + 12.0;
        let y = top + (FOOTER_HEIGHT - h) / 2.0;

        gfx::set_cursor_pos(v2(pos.x + 12.0, y));
        if theme::toggle_button("CONSOLE", self.drawer_open, v2(theme::button_width("CONSOLE"), h)) {
            self.drawer_open = !self.drawer_open;
            self.save_settings();
        }
        let (mut toggles, mut changed) = (theme::button_width("CONSOLE"), false);
        for (label, open) in [("INSPECT", &mut self.inspect_open), ("WATCH", &mut self.watch_open), ("API", &mut self.api_open)] {
            imgui::same_line();
            toggles += theme::button_width(label) + 8.0;
            if theme::toggle_button(label, *open, v2(theme::button_width(label), h)) {
                *open = !*open;
                changed = true;
            }
        }
        if changed {
            self.save_settings();
        }
        self.console.poll_log();
        if !self.drawer_open
            && let Some((color, line)) = self.console.last()
        {
            // Folded: the newest line, so nothing goes unseen.
            gfx::draw_text(v2(pos.x + 28.0 + toggles, y + 6.0), *color, line);
        }

        let (save_w, revert_w) = (theme::button_width("SAVE") + 16.0, theme::button_width("REVERT"));
        let doc = self.active.and_then(|i| self.docs.get_mut(i));
        gfx::set_cursor_pos(v2(pos.x + size.x - 12.0 - save_w - 8.0 - revert_w, y));
        if gfx::button_sized("REVERT", v2(revert_w, h))
            && let Some(d) = doc
        {
            d.revert();
        }
        imgui::same_line();
        if theme::primary_button("SAVE", v2(save_w, h))
            && let Some(d) = self.active.and_then(|i| self.docs.get_mut(i))
        {
            d.save();
        }
    }

    fn draw_sidebar(&mut self) {
        let current = self.active.and_then(|i| self.docs.get(i)).map(|d| d.path.clone());
        let mut to_open = None;
        if gfx::header("Mods", true) {
            gfx::begin_child("rhai_studio_mods", v2(0.0, 0.0), gfx::CHILD_BORDERS | gfx::CHILD_AUTO_RESIZE_Y, 0);
            for m in &self.mods {
                let (dot, state) = match self.status(&m.id) {
                    Some(s) if s.problem.is_some() => (RED, "error"),
                    Some(_) => (GREEN, "running"),
                    None => (MUTED, "not loaded"),
                };
                imgui::text_colored(dot, "●");
                imgui::same_line();
                imgui::text(&m.id);
                imgui::same_line();
                imgui::text_colored(MUTED, state);
                for f in &m.files {
                    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if gfx::selectable(&format!("    {name}##{}", f.display()), current.as_deref() == Some(f.as_path())) {
                        to_open = Some((f.clone(), m.id.clone()));
                    }
                }
            }
            if self.mods.is_empty() {
                imgui::text_colored(MUTED, "No Rhai mods yet.");
            }
            gfx::end_child();
        }
        if let Some((path, id)) = to_open {
            self.open(&path, &id);
        }

        if gfx::header("New mod", true) {
            gfx::begin_child("rhai_studio_new", v2(0.0, 0.0), gfx::CHILD_BORDERS | gfx::CHILD_AUTO_RESIZE_Y, 0);
            imgui::text("Name");
            gfx::next_width(-f32::MIN_POSITIVE);
            let enter = imgui::input_text("##new_mod", &mut self.new_name, text_flags::ENTER_RETURNS_TRUE);
            if gfx::button_sized("CREATE", v2(-f32::MIN_POSITIVE, 0.0)) || enter {
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
            gfx::end_child();
        }
    }

    fn draw_tabs(&mut self) {
        self.active = None;
        if self.docs.is_empty() {
            imgui::text_colored(MUTED, "Pick a script on the left, or make a new mod.");
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
            let d = self.docs.remove(i);
            self.debug.clear(&d.title, &d.mod_id);
        }
    }

    fn draw_doc(&mut self, i: usize) {
        let status = self.status(&self.docs[i].mod_id).cloned();
        let d = &mut self.docs[i];
        let paused = self.debug.paused_at(&d.title);
        let (color, state) = match &status {
            _ if paused.is_some() => (theme::AMBER, "PAUSED AT A BREAKPOINT"),
            Some(s) if s.problem.is_some() => (RED, "ERROR"),
            Some(_) => (GREEN, "RUNNING"),
            None => (MUTED, "NOT LOADED"),
        };
        imgui::text_colored(color, &format!("● {state}"));
        imgui::same_line();
        let lines = d.text.as_str().lines().count();
        imgui::text_colored(MUTED, &format!("  {lines} lines   {}   Ctrl+S saves", if d.dirty() { "modified" } else { "saved" }));
        if let Some(e) = &d.error {
            imgui::text_colored(RED, e);
        }
        if let Some(p) = status.as_ref().and_then(|s| s.problem.as_deref()) {
            imgui::text_colored(RED, p);
        }
        let breakpoints = self.debug.breakpoints(&d.title);
        let marks = editor::Marks { breakpoints: &breakpoints, paused_at: self.debug.paused_at(&d.title) };
        if let Some(line) = editor::code_editor("rhai_studio_editor", &mut d.text, v2(0.0, 0.0), &marks) {
            self.debug.toggle(&d.title, line);
        }
    }
}

/// Rhai Studio's settings file, as a table (empty when there is none).
fn load_settings() -> toml::Table {
    std::fs::read_to_string(config_path("rhai_studio")).ok().and_then(|t| t.parse::<toml::Table>().ok()).unwrap_or_default()
}

oxidizer_api::oxidizer_mod!(Studio);
