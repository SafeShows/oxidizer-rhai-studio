//! A graph script's tab: the sheets (the event graph and functions), variables and forms on the left with the
//! details of what is selected, the canvas or the form designer in the middle, and the generated code on the right
//! when CODE is on. Every change writes the file's text again (code and graph), so saving, reverting, unsaved dots
//! and following the disk work as for scripts.
//!
//! Breakpoints, the paused node, errors and live values go through the code that is running: the last saved
//! version's map from lines to nodes.

use crate::canvas::{self, Canvas, View};
use crate::debug::Debug;
use crate::editor::{self, Code};
use crate::form::{self, Designer};
use crate::gfx::{self, v2};
use crate::graph::{self, Compiled, Form, Graph, Loaded, Node, Param, Sheet, Var};
use crate::theme::{self, AMBER, MUTED, RED};
use oxidizer_api::imgui::{self, TextBuffer, text_flags};
use std::collections::HashMap;

const SIDE: f32 = 250.0;
const CODE_WIDTH: f32 = 440.0;
const TYPES: &[&str] = &["bool", "int", "float", "string", "array", "map", "any", "Obj"];
/// How often (in frames) live values are read.
const LIVE_EVERY: u64 = 10;

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Sheet(usize),
    Form(usize),
}

/// What the details show when nothing on the canvas is selected.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Var(usize),
    Function(usize),
}

/// What the tab asks of the window.
pub enum Action {
    None,
    /// Keep the hand-edited code: the file becomes a plain script.
    DropGraph,
}

pub struct Ctx<'a> {
    /// "mod/file.rhai"
    pub title: &'a str,
    pub debug: &'a mut Debug,
    /// The mod's error, if it stopped.
    pub problem: Option<&'a str>,
    pub frames: u64,
}

pub struct Editor {
    pub graph: Graph,
    stem: String,
    place: Place,
    views: HashMap<usize, View>,
    canvas: Canvas,
    designer: Designer,
    pick: Option<Pick>,
    undo: Vec<String>,
    redo: Vec<String>,
    /// The graph as of the last change (JSON): what undo goes back to.
    snap: String,
    compiled: Compiled,
    /// The saved file's compile: what is running.
    saved: Compiled,
    /// The code was edited by hand: ask which to keep.
    pub conflict: bool,
    show_code: bool,
    code: Code,
    live: HashMap<String, String>,
    live_missing: bool,
    fields: HashMap<String, TextBuffer>,
    note: Option<String>,
}

fn rhai_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// A one-line field over a string; true when it changed. Buffers are kept by id, and follow the value when it
/// changes elsewhere (undo).
fn field(fields: &mut HashMap<String, TextBuffer>, id: &str, value: &mut String) -> bool {
    let buf = fields.entry(id.to_string()).or_default();
    if buf.as_str() != value {
        buf.set(value);
    }
    gfx::next_width(-f32::MIN_POSITIVE);
    imgui::input_text(&format!("##{id}"), buf, 0);
    if buf.as_str() != value {
        *value = buf.as_str().to_string();
        return true;
    }
    false
}

fn code_field(fields: &mut HashMap<String, TextBuffer>, id: &str, value: &mut String) {
    let buf = fields.entry(id.to_string()).or_default();
    if buf.as_str() != value {
        buf.set(value);
    }
    imgui::input_text_multiline(&format!("##{id}"), buf, -f32::MIN_POSITIVE, 220.0, text_flags::ALLOW_TAB_INPUT);
    if buf.as_str() != value {
        *value = buf.as_str().to_string();
    }
}

/// A type: one of the usual ones, or a game type typed in.
fn type_field(fields: &mut HashMap<String, TextBuffer>, id: &str, ty: &mut String) {
    let mut i = TYPES.iter().position(|t| t == ty).map_or(TYPES.len() as i32, |i| i as i32);
    let mut items: Vec<&str> = TYPES.to_vec();
    items.push("game type...");
    gfx::next_width(110.0);
    if gfx::combo(&format!("##{id}_combo"), &mut i, &items) {
        *ty = items.get(i as usize).filter(|_| (i as usize) < TYPES.len()).map_or("Vehicle".into(), |t| t.to_string());
    }
    if !TYPES.contains(&ty.as_str()) {
        imgui::same_line();
        field(fields, &format!("{id}_name"), ty);
    }
}

fn line_in(problem: &str) -> Option<usize> {
    let at = problem.find("line ")? + 5;
    problem[at..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

impl Editor {
    /// The editor for a file, if it is a graph script.
    pub fn load(text: &str, stem: &str) -> Result<Option<Editor>, String> {
        match graph::from_file(text) {
            Loaded::Script => Ok(None),
            Loaded::Broken(e) => Err(e),
            Loaded::Graph { graph, edited } => {
                let mut e = Editor::new(graph, stem);
                e.conflict = edited;
                Ok(Some(e))
            }
        }
    }

    pub fn new(graph: Graph, stem: &str) -> Editor {
        let compiled = graph::compile(&graph, stem);
        Editor {
            snap: serde_json::to_string(&graph).unwrap_or_default(),
            saved: graph::compile(&graph, stem),
            compiled,
            graph,
            stem: stem.into(),
            place: Place::Sheet(0),
            views: HashMap::new(),
            canvas: Canvas::default(),
            designer: Designer::default(),
            pick: None,
            undo: vec![],
            redo: vec![],
            conflict: false,
            show_code: false,
            code: Code::default(),
            live: HashMap::new(),
            live_missing: false,
            fields: HashMap::new(),
            note: None,
        }
    }

    /// The file's text for the graph.
    pub fn file(&self) -> String {
        graph::to_file(&self.graph, &self.stem).0
    }

    fn rewrite(&mut self, text: &mut Code) {
        let (file, c) = graph::to_file(&self.graph, &self.stem);
        text.set(&file);
        self.code.set(&c.code);
        self.compiled = c;
    }

    /// After the file was saved: what runs is the new code, so breakpoints move to their nodes' new lines.
    pub fn saved(&mut self, title: &str, debug: &mut Debug) {
        let old = debug.breakpoints(title);
        let lines: Vec<usize> = old
            .iter()
            .filter_map(|&l| self.saved.lines.iter().find(|x| x.0 == l))
            .filter_map(|&(_, sheet, node)| self.compiled.line_of(sheet, node))
            .collect();
        if lines != old {
            debug.set(title, lines);
        }
        self.saved = graph::compile(&self.graph, &self.stem);
    }

    /// Breakpoint nodes of a sheet, from the running code's lines.
    fn breakpoints(&self, title: &str, debug: &Debug, sheet: usize) -> Vec<u32> {
        debug.breakpoints(title).iter().filter_map(|&l| self.saved.lines.iter().find(|x| x.0 == l && x.1 == sheet)).map(|x| x.2).collect()
    }

    pub fn draw(&mut self, text: &mut Code, cx: &mut Ctx) -> Action {
        if self.conflict {
            return self.draw_conflict(text);
        }
        self.poll_live(cx);
        self.draw_toolbar();
        let body = gfx::avail().y;
        gfx::begin_child("rhai_studio_bp_side", v2(SIDE, body), 0, 0);
        self.draw_side();
        gfx::end_child();
        imgui::same_line();
        gfx::begin_child("rhai_studio_bp_main", v2(if self.show_code { -(CODE_WIDTH + 8.0) } else { 0.0 }, body), 0, 0);
        match self.place {
            Place::Sheet(i) => self.draw_canvas(i, cx),
            Place::Form(i) => {
                if let Some(f) = self.graph.forms.get_mut(i) {
                    self.designer.draw(f);
                }
            }
        }
        gfx::end_child();
        if self.show_code {
            imgui::same_line();
            gfx::begin_child("rhai_studio_bp_code", v2(0.0, body), 0, 0);
            let bps = cx.debug.breakpoints(cx.title);
            let marks = editor::Marks { breakpoints: &bps, paused_at: cx.debug.paused_at(cx.title), read_only: true };
            if let Some(line) = editor::code_editor("rhai_studio_bp_code_view", &mut self.code, v2(0.0, 0.0), &marks) {
                cx.debug.toggle(cx.title, line);
            }
            gfx::end_child();
        }
        self.commit(text);
        Action::None
    }

    /// Undo, redo, and writing the file when the graph changed (not in the middle of a drag).
    fn commit(&mut self, text: &mut Code) {
        if !gfx::any_item_active() {
            if imgui::ctrl_shortcut('z') && let Some(prev) = self.undo.pop() {
                self.redo.push(std::mem::replace(&mut self.snap, prev.clone()));
                if let Ok(g) = serde_json::from_str(&prev) {
                    self.graph = g;
                }
            } else if imgui::ctrl_shortcut('y') && let Some(next) = self.redo.pop() {
                self.undo.push(std::mem::replace(&mut self.snap, next.clone()));
                if let Ok(g) = serde_json::from_str(&next) {
                    self.graph = g;
                }
            }
        }
        if gfx::mouse_down(gfx::LEFT) {
            return;
        }
        let now = serde_json::to_string(&self.graph).unwrap_or_default();
        if now != self.snap {
            if self.undo.last() != Some(&self.snap) {
                self.undo.push(std::mem::replace(&mut self.snap, now));
                self.undo.truncate(200);
                self.redo.clear();
            } else {
                self.snap = now;
            }
        }
        // The text follows the graph (also after undo).
        let file = self.file();
        if text.as_str() != file {
            self.rewrite(text);
        }
    }

    fn poll_live(&mut self, cx: &Ctx) {
        if !self.graph.live || self.live_missing || !cx.frames.is_multiple_of(LIVE_EVERY) {
            return;
        }
        match oxidizer_api::rhai_eval(&format!("graph_probes({})", rhai_str(cx.title))) {
            Ok(json) => self.live = serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => self.live_missing = true,
        }
    }

    fn draw_conflict(&mut self, text: &mut Code) -> Action {
        imgui::text_colored(AMBER, "The code in this file was changed by hand since its graph was saved.");
        imgui::text_colored(MUTED, "Keep the graph (its code replaces the edits), or keep the code (the file becomes a plain script).");
        let mut action = Action::None;
        if theme::primary_button("KEEP GRAPH", v2(theme::button_width("KEEP GRAPH") + 16.0, 0.0)) {
            self.conflict = false;
            self.rewrite(text);
        }
        imgui::same_line();
        if gfx::button_sized("KEEP CODE", v2(theme::button_width("KEEP CODE"), 0.0)) {
            action = Action::DropGraph;
        }
        let marks = editor::Marks { read_only: true, ..Default::default() };
        editor::code_editor("rhai_studio_bp_conflict", text, v2(0.0, 0.0), &marks);
        action
    }

    fn draw_toolbar(&mut self) {
        let h = gfx::frame_height();
        if theme::toggle_button("CODE", self.show_code, v2(theme::button_width("CODE"), h)) {
            self.show_code = !self.show_code;
            self.code.set(&self.compiled.code);
        }
        imgui::same_line();
        if theme::toggle_button("LIVE", self.graph.live, v2(theme::button_width("LIVE"), h)) {
            self.graph.live = !self.graph.live;
        }
        if let Place::Sheet(_) = self.place {
            imgui::same_line();
            if gfx::button_sized("FIT", v2(theme::button_width("FIT"), h)) {
                self.canvas.fit = true;
            }
        }
        imgui::same_line();
        let place = match self.place {
            Place::Sheet(0) => "Event Graph".to_string(),
            Place::Sheet(i) => format!("function {}", self.graph.sheets.get(i).map_or("", |s| s.name.as_str())),
            Place::Form(i) => format!("form {}", self.graph.forms.get(i).map_or("", |f| f.title.as_str())),
        };
        imgui::text_colored(MUTED, &place);
        if self.graph.live && self.live_missing {
            imgui::same_line();
            imgui::text_colored(MUTED, "  live values need Oxidizer 0.14");
        }
        let errors = self.compiled.errors.len();
        if errors > 0 {
            imgui::same_line();
            imgui::text_colored(RED, &format!("  {errors} problem{}", if errors == 1 { "" } else { "s" }));
            if gfx::item_hovered() && gfx::begin_tooltip_at(gfx::mouse_pos()) {
                for (sheet, _, msg) in &self.compiled.errors {
                    let name = self.graph.sheets.get(*sheet).map_or("", |s| s.name.as_str());
                    gfx::text_wrapped_at(&format!("{name}: {msg}"), 420.0);
                }
                gfx::end_tooltip();
            }
        }
        if let Some(n) = &self.note {
            imgui::same_line();
            imgui::text_colored(AMBER, &format!("  {n}"));
        }
    }

    fn go(&mut self, place: Place) {
        if self.place != place {
            self.place = place;
            self.canvas.sel.clear();
            self.designer.sel = None;
            self.note = None;
        }
    }

    fn draw_canvas(&mut self, sheet: usize, cx: &mut Ctx) {
        if sheet >= self.graph.sheets.len() {
            self.place = Place::Sheet(0);
            return;
        }
        let bps = self.breakpoints(cx.title, cx.debug, sheet);
        let paused = cx.debug.paused_at(cx.title).and_then(|l| self.saved.node_at(l)).filter(|x| x.0 == sheet).map(|x| x.1);
        let mut errors: Vec<(u32, String)> = self.compiled.errors.iter().filter(|e| e.0 == sheet).map(|e| (e.1, e.2.clone())).collect();
        if let Some(p) = cx.problem
            && let Some((s, n)) = line_in(p).and_then(|l| self.saved.node_at(l))
            && s == sheet
        {
            errors.push((n, p.to_string()));
        }
        let marks = canvas::Marks { breakpoints: &bps, paused, errors: &errors, live: &self.live };
        let view = self.views.entry(sheet).or_default();
        if let Some(canvas::Event::ToggleBreakpoint(node)) = self.canvas.draw(&mut self.graph, sheet, view, &marks) {
            self.toggle_breakpoint(sheet, node, cx);
        }
    }

    fn toggle_breakpoint(&mut self, sheet: usize, node: u32, cx: &mut Ctx) {
        match self.saved.line_of(sheet, node) {
            Some(line) => cx.debug.toggle(cx.title, line),
            None => self.note = Some("save first: that node isn't in the running code yet (or it has no exec pins)".into()),
        }
    }

    // ---- the left column ----

    fn draw_side(&mut self) {
        if gfx::header("Graphs", true) {
            gfx::begin_child("rhai_studio_bp_graphs", v2(0.0, 0.0), gfx::CHILD_BORDERS | gfx::CHILD_AUTO_RESIZE_Y, 0);
            for i in 0..self.graph.sheets.len() {
                let label = if i == 0 { "Event Graph".to_string() } else { format!("fn {}", self.graph.sheets[i].name) };
                if gfx::selectable(&format!("{label}##sheet{i}"), self.place == Place::Sheet(i)) {
                    self.go(Place::Sheet(i));
                    self.pick = (i > 0).then_some(Pick::Function(i));
                }
            }
            if gfx::button_sized("+ FUNCTION", v2(-f32::MIN_POSITIVE, 0.0)) {
                self.add_function();
            }
            gfx::end_child();
        }
        if gfx::header("Variables", true) {
            gfx::begin_child("rhai_studio_bp_vars", v2(0.0, 0.0), gfx::CHILD_BORDERS | gfx::CHILD_AUTO_RESIZE_Y, 0);
            for i in 0..self.graph.vars.len() {
                let v = &self.graph.vars[i];
                imgui::text_colored(canvas::ty_color(&v.ty), "●");
                imgui::same_line();
                if gfx::selectable(&format!("{}##var{i}", v.name), self.pick == Some(Pick::Var(i))) {
                    self.pick = Some(Pick::Var(i));
                    self.canvas.sel.clear();
                }
            }
            if gfx::button_sized("+ VARIABLE", v2(-f32::MIN_POSITIVE, 0.0)) {
                let name = (1..).map(|n| format!("var{n}")).find(|n| !self.graph.vars.iter().any(|v| &v.name == n)).unwrap();
                self.graph.vars.push(Var { name, ty: "float".into(), default: "0".into() });
                self.pick = Some(Pick::Var(self.graph.vars.len() - 1));
                self.canvas.sel.clear();
            }
            gfx::end_child();
        }
        if gfx::header("Forms", true) {
            gfx::begin_child("rhai_studio_bp_forms", v2(0.0, 0.0), gfx::CHILD_BORDERS | gfx::CHILD_AUTO_RESIZE_Y, 0);
            for i in 0..self.graph.forms.len() {
                if gfx::selectable(&format!("{}##form{i}", self.graph.forms[i].title), self.place == Place::Form(i)) {
                    self.go(Place::Form(i));
                }
            }
            if gfx::button_sized("+ FORM", v2(-f32::MIN_POSITIVE, 0.0)) {
                let id = self.graph.new_id();
                let title = format!("Window {}", self.graph.forms.len() + 1);
                self.graph.forms.push(Form { id, title, size: [320, 200], open: true, menu: true, widgets: vec![] });
                self.go(Place::Form(self.graph.forms.len() - 1));
            }
            gfx::end_child();
        }
        if gfx::header("Details", true) {
            gfx::begin_child("rhai_studio_bp_details", v2(0.0, 0.0), gfx::CHILD_BORDERS, 0);
            match self.place {
                Place::Form(i) => self.form_details(i),
                Place::Sheet(s) => self.sheet_details(s),
            }
            gfx::end_child();
        }
    }

    fn add_function(&mut self) {
        let name = (1..).map(|n| format!("function{n}")).find(|n| self.graph.function(n).is_none()).unwrap();
        let (entry, ret) = (self.graph.new_id(), self.graph.new_id());
        let mut s = Sheet { name, ..Default::default() };
        s.nodes.push(Node::new(entry, "fn.entry", [40.0, 40.0], &[]));
        s.nodes.push(Node::new(ret, "fn.return", [360.0, 40.0], &[]));
        s.links.push(graph::Link { from: entry, out: "then".into(), to: ret, inp: graph::EXEC.into() });
        self.graph.sheets.push(s);
        let i = self.graph.sheets.len() - 1;
        self.go(Place::Sheet(i));
        self.pick = Some(Pick::Function(i));
    }

    /// Puts a node in the middle of the event graph's canvas (from a button in the details) and shows it.
    fn place_node(&mut self, sheet: usize, kind: &str, data: &[(&str, &str)]) {
        let id = self.graph.new_id();
        let view = self.views.entry(sheet).or_default();
        let at = if self.place == Place::Sheet(sheet) { self.canvas.center } else { [view.pan.x + 300.0, view.pan.y + 200.0] };
        self.graph.sheets[sheet].nodes.push(Node::new(id, kind, [at[0] - 60.0, at[1] - 30.0], data));
        self.go(Place::Sheet(sheet));
        self.canvas.sel = [id].into();
    }

    fn sheet_details(&mut self, sheet: usize) {
        let sel: Vec<u32> = self.canvas.sel.iter().copied().collect();
        if let [id] = sel[..] {
            if let Some(ni) = self.graph.sheets[sheet].nodes.iter().position(|n| n.id == id) {
                return self.node_details(sheet, ni);
            }
            if let Some(c) = self.graph.sheets[sheet].comments.iter_mut().find(|c| c.id == id) {
                imgui::text_colored(AMBER, "Comment");
                field(&mut self.fields, &format!("comment{id}"), &mut c.text);
                return;
            }
        }
        if sel.len() > 1 {
            imgui::text_colored(MUTED, &format!("{} selected. Delete removes them, C puts a comment around them.", sel.len()));
            return;
        }
        match self.pick {
            Some(Pick::Var(i)) if i < self.graph.vars.len() => self.var_details(i),
            Some(Pick::Function(i)) if i > 0 && i < self.graph.sheets.len() => self.function_details(i),
            _ => gfx::text_wrapped(
                "Right-click the canvas for nodes, or drag a wire from a pin into empty space. Drag with the right button to pan, the wheel zooms. \
                 Double-click a wire for a reroute dot; Alt+click a pin cuts its wires. Ctrl+Z / Ctrl+Y undo and redo.",
            ),
        }
    }

    fn node_details(&mut self, sheet: usize, ni: usize) {
        let n = self.graph.sheets[sheet].nodes[ni].clone();
        let sh = self.graph.shape(sheet, &n);
        imgui::text_colored(AMBER, &sh.title);
        let fields = &mut self.fields;
        let mut n2 = n.clone();
        let edit = |key: &str, label: &str, fields: &mut HashMap<String, TextBuffer>, n2: &mut Node| {
            imgui::text(label);
            let mut v = n2.get(key).to_string();
            if field(fields, &format!("n{}_{key}", n2.id), &mut v) {
                n2.data.insert(key.into(), v);
            }
        };
        match n.kind.as_str() {
            "event.hotkey" => {
                edit("key", "Key (the game's names: K, F5, Space, LeftShift)", fields, &mut n2);
            }
            "sequence" => edit("count", "Outputs", fields, &mut n2),
            "literal" => edit("value", "Value", fields, &mut n2),
            "cast" => edit("ty", "Game type (Vehicle, Part, ...)", fields, &mut n2),
            "custom" | "expr" | "script" => {
                imgui::text_colored(MUTED, match n.kind.as_str() {
                    "custom" => "Rhai statements; this, the callback's parameters and nN_pin locals are in reach.",
                    "expr" => "A Rhai expression: its value is the output.",
                    _ => "Rhai at the top of the file: functions, constants.",
                });
                let mut code = n2.get("code").to_string();
                code_field(fields, &format!("n{}_code", n.id), &mut code);
                n2.data.insert("code".into(), code);
            }
            "api" => {
                imgui::text_colored(MUTED, n.get("path"));
                imgui::text_colored(MUTED, n.get("sig"));
                if let Some(it) = crate::api::index().and_then(|ix| ix.items.iter().find(|i| i.name == n.get("path").rsplit("::").next().unwrap_or("") && i.sig == n.get("sig"))) {
                    gfx::text_wrapped(&it.doc.replace('`', ""));
                }
                if n.get("member") != "get" && n.get("member") != "set" {
                    let mut pure = n.get("pure") == "1";
                    if imgui::checkbox("Pure (no exec pins: runs where its value is used)", &mut pure) {
                        n2.data.insert("pure".into(), if pure { "1" } else { "0" }.into());
                    }
                }
            }
            "var.get" | "var.set" => {
                let names: Vec<&str> = self.graph.vars.iter().map(|v| v.name.as_str()).collect();
                let mut i = names.iter().position(|v| *v == n.get("name")).map_or(-1, |i| i as i32);
                gfx::next_width(-f32::MIN_POSITIVE);
                if gfx::combo("##var_pick", &mut i, &names) && i >= 0 {
                    n2.data.insert("name".into(), names[i as usize].into());
                }
            }
            _ => {}
        }
        // Values of unwired inputs.
        let links = &self.graph.sheets[sheet].links;
        for p in sh.ins.iter().filter(|p| p.ty != graph::EXEC && !graph::is_object(&p.ty) && p.ty != graph::WILD) {
            if links.iter().any(|l| l.to == n.id && l.inp == p.name) {
                continue;
            }
            let key = format!("in.{}", p.name);
            if p.ty == "bool" {
                let mut b = n2.get(&key) == "true";
                if imgui::checkbox(&format!("{}##{key}", p.label), &mut b) {
                    n2.data.insert(key, b.to_string());
                }
            } else {
                edit(&key, &format!("{} ({})", p.label, p.ty), fields, &mut n2);
                if let Some(options) = canvas::arg_options(&self.graph, &n2, &p.name) {
                    let mut v = n2.get(&key).to_string();
                    if canvas::suggestions(&key, &mut v, &options) {
                        n2.data.insert(key, v);
                    }
                }
            }
        }
        if !sh.pure() {
            imgui::text_colored(MUTED, "Right-click the node for a breakpoint.");
        }
        if n2 != n {
            self.graph.sheets[sheet].nodes[ni] = n2;
            prune(&mut self.graph, sheet);
        }
    }

    fn var_details(&mut self, i: usize) {
        let mut v = self.graph.vars[i].clone();
        imgui::text("Name");
        field(&mut self.fields, &format!("var{i}_name"), &mut v.name);
        imgui::text("Type");
        type_field(&mut self.fields, &format!("var{i}_ty"), &mut v.ty);
        imgui::text("Starts as");
        field(&mut self.fields, &format!("var{i}_default"), &mut v.default);
        imgui::text_colored(MUTED, &format!("Kept in this.{} (survives reloads).", graph::code_name(&v.name)));
        let old = self.graph.vars[i].name.clone();
        if v.name != old {
            v.name = graph::code_name(&v.name);
            for s in &mut self.graph.sheets {
                for n in s.nodes.iter_mut().filter(|n| n.kind.starts_with("var.") && n.get("name") == old) {
                    n.data.insert("name".into(), v.name.clone());
                }
            }
        }
        let name = v.name.clone();
        self.graph.vars[i] = v;
        let w = (gfx::avail().x - 8.0) / 2.0;
        if gfx::button_sized("ADD GET", v2(w, 0.0)) {
            self.place_node(self.sheet_now(), "var.get", &[("name", &name)]);
        }
        imgui::same_line();
        if gfx::button_sized("ADD SET", v2(w, 0.0)) {
            self.place_node(self.sheet_now(), "var.set", &[("name", &name)]);
        }
        if gfx::button_sized("DELETE VARIABLE", v2(-f32::MIN_POSITIVE, 0.0)) {
            self.graph.vars.remove(i);
            self.pick = None;
        }
    }

    fn sheet_now(&self) -> usize {
        match self.place {
            Place::Sheet(s) => s,
            Place::Form(_) => 0,
        }
    }

    fn function_details(&mut self, i: usize) {
        let mut s = std::mem::take(&mut self.graph.sheets[i]);
        let old = s.name.clone();
        imgui::text("Name");
        field(&mut self.fields, &format!("fn{i}_name"), &mut s.name);
        s.name = graph::code_name(&s.name);
        for (what, list) in [("Inputs", &mut s.params), ("Outputs", &mut s.outputs)] {
            imgui::text_colored(AMBER, what);
            let mut remove = None;
            for (j, p) in list.iter_mut().enumerate() {
                if gfx::button_sized(&format!("x##{what}{j}"), v2(0.0, 0.0)) {
                    remove = Some(j);
                }
                imgui::same_line();
                gfx::next_width(90.0);
                let buf = self.fields.entry(format!("fn{i}{what}{j}")).or_default();
                if buf.as_str() != p.name {
                    buf.set(&p.name);
                }
                imgui::input_text(&format!("##fn{i}{what}{j}"), buf, 0);
                p.name = graph::code_name(buf.as_str());
                imgui::same_line();
                type_field(&mut self.fields, &format!("fn{i}{what}{j}_ty"), &mut p.ty);
            }
            if let Some(j) = remove {
                list.remove(j);
            }
            if gfx::button_sized(&format!("+ {}##{what}", what[..what.len() - 1].to_uppercase()), v2(-f32::MIN_POSITIVE, 0.0)) {
                let name = (1..).map(|n| format!("{}{n}", if what == "Inputs" { "in" } else { "out" })).find(|n| !list.iter().any(|p| &p.name == n)).unwrap();
                list.push(Param { name, ty: "float".into() });
            }
        }
        let rename = s.name != old;
        let name = s.name.clone();
        self.graph.sheets[i] = s;
        if rename {
            for sh in &mut self.graph.sheets {
                for n in sh.nodes.iter_mut().filter(|n| n.kind == "fn.call" && n.get("fn") == old) {
                    n.data.insert("fn".into(), name.clone());
                }
            }
        }
        for s in 0..self.graph.sheets.len() {
            prune(&mut self.graph, s);
        }
        if gfx::button_sized("ADD CALL NODE", v2(-f32::MIN_POSITIVE, 0.0)) {
            self.place_node(0, "fn.call", &[("fn", &name)]);
        }
        if gfx::button_sized("DELETE FUNCTION", v2(-f32::MIN_POSITIVE, 0.0)) {
            self.graph.sheets.remove(i);
            self.go(Place::Sheet(0));
            self.pick = None;
        }
    }

    fn form_details(&mut self, i: usize) {
        let Some(mut f) = self.graph.forms.get(i).cloned() else {
            self.place = Place::Sheet(0);
            return;
        };
        let fid = f.id.to_string();
        if let Some(wi) = self.designer.sel.and_then(|id| f.widgets.iter().position(|w| w.id == id)) {
            let mut w = f.widgets[wi].clone();
            let name = graph::WIDGETS.iter().find(|x| x.0 == w.kind).map_or("", |x| x.1);
            imgui::text_colored(AMBER, name);
            imgui::text(if w.kind == "text" { "Text" } else { "Label" });
            field(&mut self.fields, &format!("w{}_label", w.id), &mut w.label);
            let prop = |key: &str, label: &str, fields: &mut HashMap<String, TextBuffer>, w: &mut graph::Widget| {
                imgui::text(label);
                let mut v = w.get(key).to_string();
                if field(fields, &format!("w{}_{key}", w.id), &mut v) {
                    w.data.insert(key.into(), v);
                }
            };
            match w.kind.as_str() {
                "slider" => {
                    prop("min", "Min", &mut self.fields, &mut w);
                    prop("max", "Max", &mut self.fields, &mut w);
                    prop("default", "Starts at", &mut self.fields, &mut w);
                }
                "combo" => {
                    imgui::text("Items (one per line)");
                    let mut items = w.get("items").to_string();
                    code_field(&mut self.fields, &format!("w{}_items", w.id), &mut items);
                    w.data.insert("items".into(), items);
                }
                "checkbox" | "input_text" | "input_float" | "input_int" | "progress" => prop("default", "Starts as", &mut self.fields, &mut w),
                _ => {}
            }
            let wid = w.id.to_string();
            let full = v2(-f32::MIN_POSITIVE, 0.0);
            let ty = graph::widget_type(&w.kind);
            let event = if w.kind == "button" { "ADD 'CLICKED' EVENT" } else { "ADD 'CHANGED' EVENT" };
            if (w.kind == "button" || !matches!(w.kind.as_str(), "text" | "progress" | "separator")) && gfx::button_sized(event, full) {
                self.place_node(0, "event.widget", &[("form", &fid), ("widget", &wid)]);
            }
            if !ty.is_empty() {
                if gfx::button_sized("ADD GET NODE", full) {
                    self.place_node(0, "widget.get", &[("form", &fid), ("widget", &wid)]);
                }
                if gfx::button_sized("ADD SET NODE", full) {
                    self.place_node(0, "widget.set", &[("form", &fid), ("widget", &wid)]);
                }
            }
            if gfx::button_sized("DELETE WIDGET", full) {
                f.widgets.remove(wi);
                self.designer.sel = None;
            } else {
                f.widgets[wi] = w;
            }
        } else {
            imgui::text("Title");
            field(&mut self.fields, &format!("f{fid}_title"), &mut f.title);
            imgui::checkbox("Open at start", &mut f.open);
            imgui::checkbox("In the Mods menu", &mut f.menu);
            imgui::text_colored(MUTED, &format!("{} x {} px (drag the corner)", f.size[0], f.size[1]));
            imgui::text_colored(AMBER, "Add a widget");
            let half = (gfx::avail().x - 8.0) / 2.0;
            for (k, (kind, name, _, width)) in graph::WIDGETS.iter().enumerate() {
                if k % 2 == 1 {
                    imgui::same_line();
                }
                if gfx::button_sized(&format!("{name}##add{kind}"), v2(half, 0.0)) {
                    let id = self.graph.new_id();
                    form::add(&mut f, id, kind, name, *width);
                    self.designer.sel = Some(id);
                }
            }
            let full = v2(-f32::MIN_POSITIVE, 0.0);
            if gfx::button_sized("ADD 'SHOW' NODE", full) {
                self.place_node(0, "form.open", &[("form", &fid), ("in.shown", "true")]);
            }
            if gfx::button_sized("DELETE FORM", full) {
                self.graph.forms.remove(i);
                self.go(Place::Sheet(0));
                return;
            }
        }
        if let Some(slot) = self.graph.forms.get_mut(i) {
            *slot = f;
        }
    }
}

/// Removes wires whose pins are gone (a function lost a parameter, a call became pure).
fn prune(g: &mut Graph, sheet: usize) {
    let s = &g.sheets[sheet];
    let shapes: HashMap<u32, graph::Shape> = s.nodes.iter().map(|n| (n.id, g.shape(sheet, n))).collect();
    let keep: Vec<bool> = s
        .links
        .iter()
        .map(|l| shapes.get(&l.from).is_some_and(|sh| sh.output(&l.out).is_some()) && shapes.get(&l.to).is_some_and(|sh| sh.input(&l.inp).is_some()))
        .collect();
    let mut k = keep.into_iter();
    g.sheets[sheet].links.retain(|_| k.next().unwrap_or(true));
}
