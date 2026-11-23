//! The graph editor's canvas: nodes and wires drawn with ImGui's draw lists, panned with the right mouse button,
//! zoomed with the wheel.
//!
//! Left mouse: drag a node (or the selection) to move it, drag from a pin to wire it (let go on empty space to pick
//! a node that fits), drag on empty space to select a box, click an unwired input's value to type one, Alt+click
//! a pin to cut its wires, double-click a wire for a reroute dot. Right click: the node list (on a node: its
//! menu). Delete, Ctrl+C / X / V / D, and C puts a comment around the selection.
//!
//! The wheel: the canvas is a child window that can scroll a long way; how far the wheel scrolled it this frame is
//! the zoom, and it is put back in the middle now and then. (ImGui has no call for the wheel itself.)

use crate::api::{self, Kind};
use crate::gfx::{self, V2, v2};
use crate::graph::{self, Cat, Comment, EXEC, Graph, Link, Node, Shape, WILD};
use crate::theme;
use oxidizer_api::imgui::{self, TextBuffer, text_flags};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

const HEADER: f32 = 24.0;
const ROW: f32 = 22.0;
const PIN_R: f32 = 5.0;
const LITERAL_W: f32 = 64.0;
/// Longest value shown in a node's value box, in characters (the rest is in the details).
const LITERAL_CHARS: usize = 16;
const MIN_W: f32 = 110.0;
const COMMENT_HEADER: f32 = 26.0;
/// How far the canvas can scroll up and down, for the wheel.
const WHEEL_ROOM: f32 = 100_000.0;

const CANVAS_BG: [f32; 4] = [0.07, 0.075, 0.085, 1.0];
const GRID: [f32; 4] = [0.14, 0.15, 0.17, 1.0];
const NODE_BG: [f32; 4] = [0.11, 0.115, 0.13, 0.96];
const NODE_LINE: [f32; 4] = [0.24, 0.25, 0.28, 1.0];
const COMMENT_BG: [f32; 4] = [0.35, 0.37, 0.42, 0.16];
const COMMENT_HEAD: [f32; 4] = [0.35, 0.37, 0.42, 0.45];
const BREAKPOINT: [f32; 4] = [0.90, 0.30, 0.28, 1.0];

fn cat_color(c: Cat) -> [f32; 4] {
    match c {
        Cat::Event => [0.55, 0.16, 0.15, 1.0],
        Cat::Flow => [0.30, 0.31, 0.34, 1.0],
        Cat::Call => [0.16, 0.32, 0.55, 1.0],
        Cat::Pure => [0.20, 0.42, 0.25, 1.0],
        Cat::Var => [0.40, 0.24, 0.50, 1.0],
        Cat::Custom => [0.55, 0.40, 0.12, 1.0],
        Cat::Form => [0.12, 0.42, 0.44, 1.0],
        Cat::Function => [0.42, 0.30, 0.16, 1.0],
    }
}

/// A pin's (and its wires') color by type.
pub fn ty_color(ty: &str) -> [f32; 4] {
    match ty {
        EXEC => [0.92, 0.93, 0.95, 1.0],
        "bool" => [0.85, 0.25, 0.25, 1.0],
        "int" => [0.25, 0.85, 0.75, 1.0],
        "float" => [0.55, 0.88, 0.30, 1.0],
        "string" => [0.95, 0.35, 0.85, 1.0],
        "array" => [0.98, 0.60, 0.20, 1.0],
        "map" => [0.95, 0.85, 0.30, 1.0],
        "any" | WILD => [0.62, 0.64, 0.68, 1.0],
        _ => [0.30, 0.60, 1.0, 1.0], // game objects
    }
}

#[derive(Clone, Copy)]
pub struct View {
    /// The graph position at the canvas's top-left corner.
    pub pan: V2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        View { pan: v2(-40.0, -40.0), zoom: 1.0 }
    }
}

/// What the canvas shows besides the graph.
pub struct Marks<'a> {
    pub breakpoints: &'a [u32],
    pub paused: Option<u32>,
    pub errors: &'a [(u32, String)],
    /// Live values by "node.pin".
    pub live: &'a HashMap<String, String>,
}

/// What happened on the canvas that the editor acts on.
pub enum Event {
    ToggleBreakpoint(u32),
}

#[derive(Default)]
enum Drag {
    #[default]
    None,
    /// Moving these (nodes and comments) from where they were.
    Move { from: V2, start: Vec<(u32, [f32; 2])> },
    Wire { node: u32, pin: String, output: bool, ty: String },
    Select { from: V2 },
    Resize { id: u32, from: V2, size: [f32; 2] },
    /// The right button is down: a click opens the node list, a drag pans.
    Right { from: V2, pan: V2, moved: bool },
}

#[derive(Default)]
struct Search {
    query: TextBuffer,
    /// Where the new node goes (graph position).
    at: [f32; 2],
    /// The pin a wire was dragged from: (node, pin, is an output, type).
    from: Option<(u32, String, bool, String)>,
    results: Vec<Entry>,
    searched: Option<String>,
    focus: bool,
}

/// A node the node list offers.
#[derive(Clone)]
pub struct Entry {
    pub label: String,
    pub group: &'static str,
    pub kind: String,
    pub data: Vec<(String, String)>,
}

fn entry(group: &'static str, label: &str, kind: &str, data: &[(&str, &str)]) -> Entry {
    Entry { label: label.into(), group, kind: kind.into(), data: data.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
}

#[derive(Serialize, Deserialize)]
struct Clip {
    rhai_studio_nodes: Vec<Node>,
    links: Vec<Link>,
}

#[derive(Default)]
pub struct Canvas {
    /// Selected nodes and comments.
    pub sel: BTreeSet<u32>,
    drag: Drag,
    wheel: Option<f32>,
    search: Search,
    /// The input whose value is being typed, and the text.
    editing: Option<(u32, String)>,
    edit: TextBuffer,
    /// The node whose menu is open.
    menu: Option<u32>,
    /// The canvas's middle, in graph positions, as last drawn: where buttons outside it add nodes.
    pub center: [f32; 2],
    /// Frame the whole sheet on the next draw.
    pub fit: bool,
}

/// Where a node is on the screen, and its pins.
struct Lay {
    min: V2,
    max: V2,
    sh: Shape,
    ins: Vec<V2>,
    outs: Vec<V2>,
    /// The value box of each unwired input that takes a typed value.
    boxes: Vec<Option<(V2, V2)>>,
    /// Lines of code shown in the node (code nodes).
    code: Vec<String>,
}

fn bezier(a: V2, b: V2, z: f32) -> [V2; 4] {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * z);
    [a, v2(a.x + dx, a.y), v2(b.x - dx, b.y), b]
}

fn on_bezier(p: &[V2; 4], t: f32) -> V2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    v2(a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x, a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y)
}

fn near_bezier(p: &[V2; 4], m: V2, dist: f32) -> bool {
    (0..=24).any(|i| {
        let q = on_bezier(p, i as f32 / 24.0);
        (q.x - m.x).hypot(q.y - m.y) < dist
    })
}

fn inside(m: V2, min: V2, max: V2) -> bool {
    m.x >= min.x && m.x <= max.x && m.y >= min.y && m.y <= max.y
}

/// Whether an unwired input of this type gets a box to type its value in (game objects can't be typed).
fn typeable(ty: &str) -> bool {
    matches!(ty, "bool" | "int" | "float" | "string" | "any" | "array" | "map")
}

/// Values to pick from for a string input of a call: game types for `type_name`, the chosen type's methods
/// or members, prefix/postfix for a patch's `kind`, this graph's functions for `function`.
pub fn arg_options(g: &Graph, n: &Node, pin: &str) -> Option<Vec<String>> {
    if n.kind != "api" {
        return None;
    }
    let path = n.get("path");
    let ix = api::index()?;
    let ty = n.get("in.type_name");
    match pin {
        "type_name" => Some(ix.types.keys().cloned().collect()),
        "method" => Some(ix.members_of(ty, true)),
        "member" => Some(ix.members_of(ty, false)),
        "kind" if path == "patch" => Some(vec!["prefix".into(), "postfix".into()]),
        "function" => {
            let mut f: Vec<String> = g.sheets.iter().skip(1).map(|s| s.name.clone()).collect();
            for s in g.sheets.iter().flat_map(|s| &s.nodes).filter(|n| n.kind == "script") {
                f.extend(s.get("code").split("fn ").skip(1).filter_map(|r| r.split('(').next()).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()));
            }
            Some(f)
        }
        _ => None,
    }
}

/// Rows of the options that match what is typed, under a field (the best first); true when one was clicked,
/// which `value` then is.
pub fn suggestions(id: &str, value: &mut String, options: &[String]) -> bool {
    if options.iter().any(|o| o == value) {
        return false;
    }
    let q = value.trim().to_lowercase();
    let last = |o: &str| o.rsplit(['.', '+']).next().unwrap_or(o).to_lowercase();
    let mut hits: Vec<&String> = options.iter().filter(|o| o.to_lowercase().contains(&q)).collect();
    hits.sort_by_key(|o| (last(o) != q, !last(o).starts_with(&q), !o.to_lowercase().starts_with(&q), o.len()));
    for (i, o) in hits.iter().take(8).enumerate() {
        if gfx::selectable(&format!("  {o}##{id}_s{i}"), false) {
            *value = o.to_string();
            return true;
        }
    }
    if hits.len() > 8 {
        imgui::text_colored(theme::MUTED, &format!("  {} more: keep typing", hits.len() - 8));
    } else if hits.is_empty() && !options.is_empty() {
        imgui::text_colored(theme::MUTED, "  no match");
    }
    false
}

/// What a value box on a node shows: the typed value, shortened.
fn shown(n: &Node, pin: &str, ty: &str) -> String {
    let v = n.get(&format!("in.{pin}"));
    if ty == "bool" { if v == "true" { "☑ true".into() } else { "☐ false".into() } } else { trunc(v, LITERAL_CHARS) }
}

/// A value box's width: its text, at least LITERAL_W.
fn box_width(text: &str) -> f32 {
    (gfx::text_width(text) + 10.0).max(LITERAL_W)
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() }
}

/// Every built-in node the list offers on this sheet.
pub fn builtins(g: &Graph, sheet: usize) -> Vec<Entry> {
    let mut v = vec![];
    if sheet == 0 {
        for (kind, title, _, _) in graph::EVENTS {
            v.push(entry("Events", title, kind, &[]));
        }
        v.push(entry("Events", "On Key (hotkey)", "event.hotkey", &[("key", "K")]));
        for f in &g.forms {
            for w in &f.widgets {
                let (fid, wid) = (f.id.to_string(), w.id.to_string());
                match w.kind.as_str() {
                    "button" => v.push(entry("Events", &format!("On {} clicked ({})", w.label, f.title), "event.widget", &[("form", &fid), ("widget", &wid)])),
                    "text" | "progress" | "separator" => {}
                    _ => v.push(entry("Events", &format!("On {} changed ({})", w.label, f.title), "event.widget", &[("form", &fid), ("widget", &wid)])),
                }
            }
        }
    } else {
        v.push(entry("Function", "Return", "fn.return", &[]));
    }
    for (kind, label) in [("branch", "Branch (if)"), ("sequence", "Sequence"), ("for", "For Loop"), ("for_each", "For Each"), ("while", "While"), ("break", "Break"), ("print", "Print")] {
        v.push(entry("Flow", label, kind, &[]));
    }
    for f in g.sheets.iter().skip(1) {
        v.push(entry("Functions", &format!("Call {}", f.name), "fn.call", &[("fn", &f.name)]));
    }
    for var in &g.vars {
        v.push(entry("Variables", &format!("Get {}", var.name), "var.get", &[("name", &var.name)]));
        v.push(entry("Variables", &format!("Set {}", var.name), "var.set", &[("name", &var.name)]));
    }
    for f in &g.forms {
        let fid = f.id.to_string();
        v.push(entry("Forms", &format!("Show {}", f.title), "form.open", &[("form", &fid), ("in.shown", "true")]));
        v.push(entry("Forms", &format!("Is {} shown", f.title), "form.is_open", &[("form", &fid)]));
        for w in f.widgets.iter().filter(|w| !graph::widget_type(&w.kind).is_empty()) {
            let wid = w.id.to_string();
            v.push(entry("Forms", &format!("Get {} ({})", w.label, f.title), "widget.get", &[("form", &fid), ("widget", &wid)]));
            v.push(entry("Forms", &format!("Set {} ({})", w.label, f.title), "widget.set", &[("form", &fid), ("widget", &wid)]));
        }
    }
    for (op, ty, _) in graph::OPS {
        let label = match *op {
            "!" => "NOT".to_string(),
            "&&" => "AND".into(),
            "||" => "OR".into(),
            _ => format!("a {op} b ({ty})"),
        };
        v.push(entry("Math", &label, "op", &[("op", op), ("ty", ty)]));
    }
    for ty in ["float", "int", "bool", "string"] {
        v.push(entry("Values", &format!("{ty} value"), "literal", &[("ty", ty)]));
        v.push(entry("Values", &format!("To {ty}"), "convert", &[("to", ty)]));
    }
    v.push(entry("Values", "Cast to a game type", "cast", &[("ty", "Obj")]));
    v.push(entry("Code", "Rhai Code (statements)", "custom", &[("code", "// Rhai here")]));
    v.push(entry("Code", "Rhai Expression (a value)", "expr", &[("code", "1 + 1")]));
    if sheet == 0 {
        v.push(entry("Code", "Script Code (top of the file)", "script", &[("code", "// functions and constants")]));
    }
    v.push(entry("Canvas", "Comment", "comment", &[]));
    v
}

/// API nodes for a search: functions, methods, properties.
fn api_entries(q: &str, object: Option<&str>) -> Vec<Entry> {
    let Some(ix) = api::index() else { return vec![] };
    let hits: Vec<usize> = if q.is_empty() {
        // Dragged from a game object: what its type has.
        let Some(t) = object.filter(|t| *t != "Obj") else { return vec![] };
        ix.items
            .iter()
            .enumerate()
            .filter(|(_, it)| matches!(it.kind, Kind::Property | Kind::Method) && it.doc.rsplit(" on ").next().is_some_and(|on| on.split(", ").any(|x| x.trim() == t)))
            .map(|(i, _)| i)
            .take(300)
            .collect()
    } else {
        ix.search(q)
    };
    let mut out = vec![];
    for i in hits.into_iter().take(300) {
        let it = &ix.items[i];
        let pure = if it.pure { "1" } else { "0" };
        match it.kind {
            Kind::Function => out.push(entry("API", &it.label(), "api", &[("path", &it.path), ("sig", &it.sig), ("member", "fn"), ("pure", pure)])),
            Kind::Method => out.push(entry("API", &it.label(), "api", &[("path", &it.name), ("sig", &it.sig), ("member", "method"), ("pure", pure)])),
            Kind::Property => {
                out.push(entry("API", &format!("Get {}", it.label()), "api", &[("path", &it.name), ("sig", &it.sig), ("member", "get"), ("pure", "1")]));
                out.push(entry("API", &format!("Set {}", it.label()), "api", &[("path", &it.name), ("sig", &it.sig), ("member", "set")]));
            }
            _ => {}
        }
    }
    out
}

impl Canvas {
    fn to_screen(o: V2, view: &View, p: [f32; 2]) -> V2 {
        v2(o.x + (p[0] - view.pan.x) * view.zoom, o.y + (p[1] - view.pan.y) * view.zoom)
    }

    fn to_graph(o: V2, view: &View, p: V2) -> [f32; 2] {
        [(p.x - o.x) / view.zoom + view.pan.x, (p.y - o.y) / view.zoom + view.pan.y]
    }

    fn lay(g: &Graph, sheet: usize, n: &Node, o: V2, view: &View) -> Lay {
        let sh = g.shape(sheet, n);
        let z = view.zoom;
        let min = Self::to_screen(o, view, n.pos);
        if n.kind == "reroute" {
            let c = v2(min.x + 8.0 * z, min.y + 8.0 * z);
            return Lay { min, max: v2(min.x + 16.0 * z, min.y + 16.0 * z), sh, ins: vec![c], outs: vec![c], boxes: vec![None], code: vec![] };
        }
        let links = &g.sheets[sheet].links;
        let wired = |p: &str| links.iter().any(|l| l.to == n.id && l.inp == p);
        let code: Vec<String> = if matches!(n.kind.as_str(), "custom" | "expr" | "script") { n.get("code").lines().take(8).map(|l| trunc(l, 48)).collect() } else { vec![] };
        let tw = gfx::text_width;
        let ins_w = sh.ins.iter().map(|p| 18.0 + tw(&p.label) + if typeable(&p.ty) && !wired(&p.name) { box_width(&shown(n, &p.name, &p.ty)) + 8.0 } else { 0.0 }).fold(0.0, f32::max);
        let outs_w = sh.outs.iter().map(|p| 18.0 + tw(&p.label)).fold(0.0, f32::max);
        let code_w = code.iter().map(|l| tw(l) + 20.0).fold(0.0, f32::max);
        let w = (tw(&sh.title) + 34.0).max(ins_w + outs_w + 16.0).max(code_w).max(MIN_W);
        let rows = sh.ins.len().max(sh.outs.len()) as f32;
        let h = HEADER + rows * ROW + code.len() as f32 * gfx::line_height() + 8.0;
        let max = v2(min.x + w * z, min.y + h * z);
        let row_y = |i: usize| min.y + (HEADER + ROW * (i as f32 + 0.5)) * z;
        let ins: Vec<V2> = (0..sh.ins.len()).map(|i| v2(min.x, row_y(i))).collect();
        let outs = (0..sh.outs.len()).map(|i| v2(max.x, row_y(i))).collect();
        let boxes = sh
            .ins
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (typeable(&p.ty) && !wired(&p.name)).then(|| {
                    let x = min.x + (16.0 + tw(&p.label) + 6.0) * z;
                    (v2(x, row_y(i) - 8.0 * z), v2(x + box_width(&shown(n, &p.name, &p.ty)) * z, row_y(i) + 8.0 * z))
                })
            })
            .collect();
        Lay { min, max, sh, ins, outs, boxes, code }
    }

    fn text(at: V2, color: [f32; 4], z: f32, s: &str) {
        gfx::draw_text_sized(at, color, gfx::font_size() * z, s);
    }

    /// Draws the canvas for a sheet, filling the space left; edits the graph.
    pub fn draw(&mut self, g: &mut Graph, sheet: usize, view: &mut View, marks: &Marks) -> Option<Event> {
        let mut event = None;
        gfx::next_content_size(v2(0.0, WHEEL_ROOM * 2.0));
        gfx::push_color(gfx::COL_CHILD_BG, CANVAS_BG);
        gfx::begin_child("rhai_studio_canvas", v2(0.0, 0.0), gfx::CHILD_BORDERS, gfx::WINDOW_NO_SCROLLBAR);
        gfx::pop_colors(1);
        let (o, size) = (gfx::window_pos(), gfx::window_size());
        let mouse = gfx::mouse_pos();

        // The wheel: how far it scrolled the (otherwise unused) scroll range.
        let sy = gfx::scroll_y();
        match self.wheel {
            None => gfx::set_scroll_y(WHEEL_ROOM), // where it was isn't the wheel
            Some(last) if sy != last => {
                let before = Self::to_graph(o, view, mouse);
                view.zoom = (view.zoom * 0.998_f32.powf(sy - last)).clamp(0.25, 2.5);
                let after = Self::to_graph(o, view, mouse);
                view.pan = v2(view.pan.x + before[0] - after[0], view.pan.y + before[1] - after[1]);
            }
            _ => {}
        }
        self.wheel = Some(if self.wheel.is_none() { WHEEL_ROOM } else { sy });
        if (sy - WHEEL_ROOM).abs() > WHEEL_ROOM / 2.0 {
            gfx::set_scroll_y(WHEEL_ROOM);
            self.wheel = Some(WHEEL_ROOM);
        }
        gfx::set_cursor_pos(o);
        gfx::invisible_button("##canvas", size);
        let hovered = gfx::item_hovered();
        let z = view.zoom;
        self.center = Self::to_graph(o, view, v2(o.x + size.x / 2.0, o.y + size.y / 2.0));

        // Grid.
        let step = 32.0 * z;
        if step > 8.0 {
            let mut x = o.x - (view.pan.x * z).rem_euclid(step);
            while x < o.x + size.x {
                gfx::draw_line(v2(x, o.y), v2(x, o.y + size.y), GRID);
                x += step;
            }
            let mut y = o.y - (view.pan.y * z).rem_euclid(step);
            while y < o.y + size.y {
                gfx::draw_line(v2(o.x, y), v2(o.x + size.x, y), GRID);
                y += step;
            }
        }

        // Comments under everything.
        let mut comment_hit: Option<(u32, bool)> = None; // (id, on its resize corner)
        for c in &g.sheets[sheet].comments {
            let min = Self::to_screen(o, view, c.pos);
            let max = Self::to_screen(o, view, [c.pos[0] + c.size[0], c.pos[1] + c.size[1]]);
            let head = v2(max.x, min.y + COMMENT_HEADER * z);
            gfx::draw_rect(min, max, COMMENT_BG, 6.0 * z);
            gfx::draw_rect(min, head, COMMENT_HEAD, 6.0 * z);
            Self::text(v2(min.x + 8.0 * z, min.y + 5.0 * z), theme::TEXT, z, &c.text);
            gfx::draw_triangle(v2(max.x - 12.0 * z, max.y), max, v2(max.x, max.y - 12.0 * z), COMMENT_HEAD);
            if self.sel.contains(&c.id) {
                gfx::draw_frame(min, max, theme::AMBER, 6.0 * z, 1.5);
            }
            if inside(mouse, v2(max.x - 14.0 * z, max.y - 14.0 * z), max) {
                comment_hit = Some((c.id, true));
            } else if inside(mouse, min, head) {
                comment_hit = Some((c.id, false));
            }
        }

        let lays: Vec<(u32, Lay)> = g.sheets[sheet].nodes.iter().map(|n| (n.id, Self::lay(g, sheet, n, o, view))).collect();
        if std::mem::take(&mut self.fit) && !lays.is_empty() {
            let (mut a, mut b) = ([f32::MAX; 2], [f32::MIN; 2]);
            for (_, l) in &lays {
                let (p, q) = (Self::to_graph(o, view, l.min), Self::to_graph(o, view, l.max));
                (a, b) = ([a[0].min(p[0]), a[1].min(p[1])], [b[0].max(q[0]), b[1].max(q[1])]);
            }
            for c in &g.sheets[sheet].comments {
                (a, b) = ([a[0].min(c.pos[0]), a[1].min(c.pos[1])], [b[0].max(c.pos[0] + c.size[0]), b[1].max(c.pos[1] + c.size[1])]);
            }
            let margin = 30.0;
            view.zoom = ((size.x - margin * 2.0) / (b[0] - a[0])).min((size.y - margin * 2.0) / (b[1] - a[1])).clamp(0.25, 1.2);
            view.pan = v2((a[0] + b[0]) / 2.0 - size.x / 2.0 / view.zoom, (a[1] + b[1]) / 2.0 - size.y / 2.0 / view.zoom);
        }
        let find = |id: u32| lays.iter().find(|(i, _)| *i == id).map(|(_, l)| l);
        let pin_at = |l: &Lay, name: &str, output: bool| -> Option<(V2, String)> {
            let pins = if output { &l.sh.outs } else { &l.sh.ins };
            let i = pins.iter().position(|p| p.name == name)?;
            Some((if output { l.outs[i] } else { l.ins[i] }, pins[i].ty.clone()))
        };

        // Wires.
        let mut wire_hit: Option<usize> = None;
        for (li, l) in g.sheets[sheet].links.iter().enumerate() {
            let (Some(a), Some(b)) = (find(l.from), find(l.to)) else { continue };
            let (Some((pa, ty)), Some((pb, _))) = (pin_at(a, &l.out, true), pin_at(b, &l.inp, false)) else { continue };
            let curve = bezier(pa, pb, z);
            let hot = hovered && matches!(self.drag, Drag::None) && near_bezier(&curve, mouse, 6.0);
            if hot {
                wire_hit = Some(li);
            }
            let mut color = ty_color(&ty);
            if hot {
                color = [color[0] * 1.2, color[1] * 1.2, color[2] * 1.2, 1.0];
            }
            gfx::draw_bezier(curve, color, if ty == EXEC { 2.6 } else { 2.0 } * z.max(0.6) + if hot { 1.5 } else { 0.0 });
        }

        // Nodes.
        let mut node_hit = None;
        let mut pin_hit: Option<(u32, String, bool, String)> = None;
        let mut box_hit: Option<(u32, String, String)> = None;
        let links = g.sheets[sheet].links.clone();
        for (id, l) in &lays {
            let Some(n) = g.sheets[sheet].nodes.iter().find(|n| n.id == *id) else { continue };
            let error = marks.errors.iter().find(|e| e.0 == *id);
            if n.kind == "reroute" {
                let ty = l.sh.outs[0].ty.clone();
                gfx::draw_circle(l.ins[0], 5.0 * z, ty_color(&ty), true);
                if self.sel.contains(id) {
                    gfx::draw_circle(l.ins[0], 7.0 * z, theme::AMBER, false);
                }
            } else {
                let r = 6.0 * z;
                gfx::draw_rect(l.min, l.max, NODE_BG, r);
                gfx::draw_rect(l.min, v2(l.max.x, l.min.y + HEADER * z), cat_color(l.sh.cat), r);
                let mut tx = l.min.x + 8.0 * z;
                if marks.breakpoints.contains(id) {
                    gfx::draw_circle(v2(tx + 4.0 * z, l.min.y + HEADER * z / 2.0), 4.5 * z, BREAKPOINT, true);
                    tx += 14.0 * z;
                }
                Self::text(v2(tx, l.min.y + 4.0 * z), theme::TEXT, z, &l.sh.title);
                let (outline, thick) = if marks.paused == Some(*id) {
                    (theme::AMBER, 3.5)
                } else if error.is_some() {
                    (theme::RED, 2.0)
                } else if self.sel.contains(id) {
                    (theme::AMBER, 2.0)
                } else {
                    (NODE_LINE, 1.0)
                };
                gfx::draw_frame(l.min, l.max, outline, r, thick);
                for (i, p) in l.sh.ins.iter().enumerate() {
                    let at = l.ins[i];
                    let linked = links.iter().any(|k| k.to == *id && k.inp == p.name);
                    draw_pin(at, &p.ty, linked, z);
                    Self::text(v2(at.x + 12.0 * z, at.y - 8.0 * z), theme::TEXT, z, &p.label);
                    if let Some((bmin, bmax)) = l.boxes[i] {
                        gfx::draw_rect(bmin, bmax, theme::FIELD, 3.0 * z);
                        Self::text(v2(bmin.x + 4.0 * z, bmin.y + 1.0 * z), theme::AMBER, z, &shown(n, &p.name, &p.ty));
                        if hovered && inside(mouse, bmin, bmax) {
                            box_hit = Some((*id, p.name.clone(), p.ty.clone()));
                        }
                    }
                }
                for (i, p) in l.sh.outs.iter().enumerate() {
                    let at = l.outs[i];
                    let linked = links.iter().any(|k| k.from == *id && k.out == p.name);
                    draw_pin(at, &p.ty, linked, z);
                    let w = gfx::text_width(&p.label) * z;
                    Self::text(v2(at.x - 12.0 * z - w, at.y - 8.0 * z), theme::TEXT, z, &p.label);
                }
                let rows = l.sh.ins.len().max(l.sh.outs.len()) as f32;
                for (i, line) in l.code.iter().enumerate() {
                    let y = l.min.y + (HEADER + rows * ROW + 2.0 + i as f32 * gfx::line_height()) * z;
                    Self::text(v2(l.min.x + 10.0 * z, y), theme::MUTED, z, line);
                }
            }
            if hovered && inside(mouse, v2(l.min.x - 8.0, l.min.y), v2(l.max.x + 8.0, l.max.y)) {
                node_hit = Some(*id);
                for (i, p) in l.sh.ins.iter().enumerate() {
                    if (l.ins[i].x - mouse.x).hypot(l.ins[i].y - mouse.y) < (PIN_R + 5.0) * z.max(0.7) {
                        pin_hit = Some((*id, p.name.clone(), false, p.ty.clone()));
                    }
                }
                for (i, p) in l.sh.outs.iter().enumerate() {
                    if (l.outs[i].x - mouse.x).hypot(l.outs[i].y - mouse.y) < (PIN_R + 5.0) * z.max(0.7) {
                        pin_hit = Some((*id, p.name.clone(), true, p.ty.clone()));
                    }
                }
            }
        }
        if node_hit.is_some() {
            comment_hit = None;
        }

        // The wire being dragged.
        if let Drag::Wire { node, pin, output, ty } = &self.drag
            && let Some(l) = find(*node)
            && let Some((p, _)) = pin_at(l, pin, *output)
        {
            let curve = if *output { bezier(p, mouse, z) } else { bezier(mouse, p, z) };
            gfx::draw_bezier(curve, ty_color(ty), 2.0 * z.max(0.6));
        }
        if let Drag::Select { from } = self.drag {
            let (min, max) = (v2(from.x.min(mouse.x), from.y.min(mouse.y)), v2(from.x.max(mouse.x), from.y.max(mouse.y)));
            gfx::draw_rect(min, max, [0.95, 0.71, 0.24, 0.08], 0.0);
            gfx::draw_frame(min, max, theme::AMBER, 0.0, 1.0);
        }

        // Tooltips: a wire's live value, a pin's type, a node's error.
        if matches!(self.drag, Drag::None) && hovered {
            let tip = if let Some((id, pin, output, ty)) = &pin_hit {
                let live = if *output { marks.live.get(&format!("{id}.{pin}")) } else { None };
                Some(match live {
                    Some(v) => format!("{ty}: {v}"),
                    None => ty.clone(),
                })
            } else if let Some(li) = wire_hit {
                let l = &g.sheets[sheet].links[li];
                marks.live.get(&format!("{}.{}", l.from, l.out)).map(|v| format!("= {v}")).or_else(|| (!marks.live.is_empty()).then(|| "(no value yet)".into()))
            } else {
                node_hit.and_then(|id| marks.errors.iter().find(|e| e.0 == id)).map(|e| e.1.clone())
            };
            if let Some(t) = tip.filter(|t| !t.is_empty() && t != EXEC)
                && gfx::begin_tooltip_at(v2(mouse.x + 14.0, mouse.y + 10.0))
            {
                gfx::text_wrapped_at(&t, 420.0);
                gfx::end_tooltip();
            }
        }

        // The mouse.
        let ctrl = gfx::key_down(gfx::MOD_CTRL) || gfx::key_down(gfx::MOD_SHIFT);
        let gpos = Self::to_graph(o, view, mouse);
        if hovered && gfx::mouse_double_clicked(gfx::LEFT) && node_hit.is_none() {
            if let Some(li) = wire_hit {
                let l = g.sheets[sheet].links.remove(li);
                let id = g.new_id();
                let s = &mut g.sheets[sheet];
                s.nodes.push(Node::new(id, "reroute", [gpos[0] - 8.0, gpos[1] - 8.0], &[]));
                s.links.push(Link { from: l.from, out: l.out, to: id, inp: "in".into() });
                s.links.push(Link { from: id, out: "out".into(), to: l.to, inp: l.inp });
            }
        } else if hovered && gfx::mouse_clicked(gfx::LEFT) {
            if let Some((id, pin, output, ty)) = pin_hit.clone() {
                if gfx::key_down(gfx::MOD_ALT) {
                    g.sheets[sheet].links.retain(|l| !(if output { l.from == id && l.out == pin } else { l.to == id && l.inp == pin }));
                } else {
                    self.drag = Drag::Wire { node: id, pin, output, ty };
                }
            } else if let Some((id, pin, ty)) = box_hit.clone() {
                let key = format!("in.{pin}");
                let n = g.sheets[sheet].nodes.iter_mut().find(|n| n.id == id).unwrap();
                if ty == "bool" {
                    let v = n.get(&key) == "true";
                    n.data.insert(key, (!v).to_string());
                } else {
                    self.edit.set(n.get(&key));
                    self.editing = Some((id, key));
                    gfx::open_popup("##rhai_studio_literal");
                }
            } else if let Some(id) = node_hit {
                if ctrl {
                    if !self.sel.remove(&id) {
                        self.sel.insert(id);
                    }
                } else if !self.sel.contains(&id) {
                    self.sel = BTreeSet::from([id]);
                }
                self.drag = Drag::Move { from: mouse, start: self.moving(g, sheet, o, view) };
            } else if let Some((id, corner)) = comment_hit {
                let c = g.sheets[sheet].comments.iter().find(|c| c.id == id).unwrap();
                if corner {
                    self.drag = Drag::Resize { id, from: mouse, size: c.size };
                } else {
                    if !ctrl {
                        self.sel = BTreeSet::from([id]);
                    } else {
                        self.sel.insert(id);
                    }
                    self.drag = Drag::Move { from: mouse, start: self.moving(g, sheet, o, view) };
                }
            } else {
                if !ctrl {
                    self.sel.clear();
                }
                self.drag = Drag::Select { from: mouse };
            }
        }
        if hovered && gfx::mouse_clicked(gfx::RIGHT) {
            self.drag = Drag::Right { from: mouse, pan: view.pan, moved: false };
        }

        match &mut self.drag {
            Drag::Move { from, start } if gfx::mouse_down(gfx::LEFT) => {
                let (dx, dy) = ((mouse.x - from.x) / z, (mouse.y - from.y) / z);
                let s = &mut g.sheets[sheet];
                for (id, p) in start.iter() {
                    let to = [(p[0] + dx).round(), (p[1] + dy).round()];
                    if let Some(n) = s.nodes.iter_mut().find(|n| n.id == *id) {
                        n.pos = to;
                    } else if let Some(c) = s.comments.iter_mut().find(|c| c.id == *id) {
                        c.pos = to;
                    }
                }
            }
            Drag::Resize { id, from, size } if gfx::mouse_down(gfx::LEFT) => {
                if let Some(c) = g.sheets[sheet].comments.iter_mut().find(|c| c.id == *id) {
                    c.size = [(size[0] + (mouse.x - from.x) / z).max(120.0), (size[1] + (mouse.y - from.y) / z).max(60.0)];
                }
            }
            Drag::Right { from, pan, moved } if gfx::mouse_down(gfx::RIGHT) => {
                if (mouse.x - from.x).hypot(mouse.y - from.y) > 4.0 {
                    *moved = true;
                }
                if *moved {
                    view.pan = v2(pan.x - (mouse.x - from.x) / z, pan.y - (mouse.y - from.y) / z);
                }
            }
            Drag::Right { moved, .. } => {
                if !*moved {
                    match node_hit {
                        Some(id) => {
                            self.menu = Some(id);
                            if !self.sel.contains(&id) {
                                self.sel = BTreeSet::from([id]);
                            }
                            gfx::open_popup("##rhai_studio_node_menu");
                        }
                        None => self.open_search(gpos, None),
                    }
                }
                self.drag = Drag::None;
            }
            Drag::Wire { node, pin, output, ty } if !gfx::mouse_down(gfx::LEFT) => {
                let (node, pin, output, ty) = (*node, pin.clone(), *output, ty.clone());
                self.drag = Drag::None;
                match pin_hit {
                    Some((id2, pin2, out2, ty2)) if out2 != output && id2 != node => {
                        let (from, to) = if output { ((node, pin, ty), (id2, pin2, ty2)) } else { ((id2, pin2, ty2), (node, pin, ty)) };
                        if graph::compatible(&from.2, &to.2) {
                            connect(g, sheet, from.0, &from.1, to.0, &to.1);
                        }
                    }
                    None if hovered && node_hit.is_none() => self.open_search(gpos, Some((node, pin, output, ty))),
                    _ => {}
                }
            }
            Drag::Select { from } if !gfx::mouse_down(gfx::LEFT) => {
                let (min, max) = (v2(from.x.min(mouse.x), from.y.min(mouse.y)), v2(from.x.max(mouse.x), from.y.max(mouse.y)));
                for (id, l) in &lays {
                    if l.min.x < max.x && l.max.x > min.x && l.min.y < max.y && l.max.y > min.y {
                        self.sel.insert(*id);
                    }
                }
                self.drag = Drag::None;
            }
            Drag::Move { .. } | Drag::Resize { .. } | Drag::Select { .. } | Drag::Wire { .. } => {
                if !gfx::mouse_down(gfx::LEFT) {
                    self.drag = Drag::None;
                }
            }
            Drag::None => {}
        }

        // Keys, while the canvas has the keyboard and nothing is being typed into.
        if gfx::window_focused() && !gfx::any_item_active() {
            let plain = !gfx::key_down(gfx::MOD_CTRL);
            if gfx::key_pressed(gfx::KEY_DELETE) {
                self.delete(g, sheet);
            } else if imgui::ctrl_shortcut('c') {
                self.copy(g, sheet);
            } else if imgui::ctrl_shortcut('x') {
                self.copy(g, sheet);
                self.delete(g, sheet);
            } else if imgui::ctrl_shortcut('v') {
                self.paste(g, sheet, &gfx::clipboard(), if hovered { gpos } else { self.center });
            } else if imgui::ctrl_shortcut('d') {
                self.copy(g, sheet);
                let at = self.sel.iter().filter_map(|id| g.sheets[sheet].nodes.iter().find(|n| n.id == *id)).map(|n| [n.pos[0] + 40.0, n.pos[1] + 40.0]).next();
                self.paste(g, sheet, &gfx::clipboard(), at.unwrap_or(self.center));
            } else if plain && gfx::key_pressed(gfx::KEY_A + 2) {
                self.comment(g, sheet, o, view, gpos);
            } else if gfx::key_pressed(gfx::KEY_ESCAPE) {
                self.sel.clear();
            }
        }

        event = event.or(self.popups(g, sheet));
        gfx::end_child();
        event
    }

    /// The selected nodes and comments, and the nodes inside selected comments, with where they are now.
    fn moving(&self, g: &Graph, sheet: usize, o: V2, view: &View) -> Vec<(u32, [f32; 2])> {
        let s = &g.sheets[sheet];
        let mut out: Vec<(u32, [f32; 2])> = vec![];
        for c in s.comments.iter().filter(|c| self.sel.contains(&c.id)) {
            out.push((c.id, c.pos));
            for n in &s.nodes {
                let l = Self::lay(g, sheet, n, o, view);
                let (a, b) = (Self::to_graph(o, view, l.min), Self::to_graph(o, view, l.max));
                if a[0] >= c.pos[0] && a[1] >= c.pos[1] && b[0] <= c.pos[0] + c.size[0] && b[1] <= c.pos[1] + c.size[1] {
                    out.push((n.id, n.pos));
                }
            }
        }
        for n in s.nodes.iter().filter(|n| self.sel.contains(&n.id)) {
            if !out.iter().any(|(id, _)| *id == n.id) {
                out.push((n.id, n.pos));
            }
        }
        out
    }

    fn open_search(&mut self, at: [f32; 2], from: Option<(u32, String, bool, String)>) {
        self.search.at = at;
        self.search.from = from;
        self.search.query.set("");
        self.search.searched = None;
        self.search.focus = true;
        gfx::open_popup("##rhai_studio_search");
    }

    /// The popups: the node list, a node's menu, typing an input's value.
    fn popups(&mut self, g: &mut Graph, sheet: usize) -> Option<Event> {
        let mut event = None;
        if gfx::begin_popup("##rhai_studio_search") {
            if std::mem::take(&mut self.search.focus) {
                imgui::set_keyboard_focus_here(0);
            }
            gfx::next_width(480.0);
            let enter = imgui::input_text("##search", &mut self.search.query, text_flags::ENTER_RETURNS_TRUE);
            let q = self.search.query.as_str().trim().to_lowercase();
            if self.search.searched.as_deref() != Some(q.as_str()) {
                self.search.results = self.results(g, sheet, &q);
                self.search.searched = Some(q.clone());
            }
            if let Some((_, _, _, ty)) = &self.search.from {
                imgui::text_colored(theme::MUTED, &format!("fits a {ty} pin"));
            }
            let mut pick = enter.then_some(0).filter(|_| !self.search.results.is_empty());
            gfx::begin_child("##results", v2(480.0, 360.0), 0, 0);
            let mut group = "";
            for (i, e) in self.search.results.iter().enumerate().take(200) {
                if e.group != group {
                    group = e.group;
                    imgui::text_colored(theme::AMBER, group);
                }
                if gfx::selectable(&format!("  {}##r{i}", e.label), false) {
                    pick = Some(i);
                }
            }
            if self.search.results.is_empty() {
                imgui::text_colored(theme::MUTED, if api::index().is_none() { "Reading the API..." } else { "Nothing found" });
            }
            gfx::end_child();
            if let Some(i) = pick {
                let e = self.search.results[i].clone();
                self.add(g, sheet, &e);
                gfx::close_popup();
            }
            gfx::end_popup();
        }
        if gfx::begin_popup("##rhai_studio_node_menu") {
            if let Some(id) = self.menu {
                if gfx::selectable("Breakpoint", false) {
                    event = Some(Event::ToggleBreakpoint(id));
                }
                if gfx::selectable("Duplicate", false) {
                    self.copy(g, sheet);
                    let at = g.sheets[sheet].nodes.iter().find(|n| n.id == id).map(|n| [n.pos[0] + 40.0, n.pos[1] + 40.0]);
                    self.paste(g, sheet, &gfx::clipboard(), at.unwrap_or(self.center));
                }
                if gfx::selectable("Cut its wires", false) {
                    g.sheets[sheet].links.retain(|l| l.from != id && l.to != id);
                }
                if gfx::selectable("Delete", false) {
                    self.delete(g, sheet);
                }
            }
            gfx::end_popup();
        }
        if gfx::begin_popup("##rhai_studio_literal") {
            if let Some((id, key)) = self.editing.clone() {
                gfx::next_width(240.0);
                if !gfx::any_item_active() {
                    imgui::set_keyboard_focus_here(0);
                }
                let mut done = imgui::input_text("##literal", &mut self.edit, text_flags::ENTER_RETURNS_TRUE);
                let pin = key.trim_start_matches("in.");
                let options = g.sheets[sheet].nodes.iter().find(|n| n.id == id).and_then(|n| arg_options(g, n, pin));
                if let Some(options) = options {
                    let mut v = self.edit.as_str().to_string();
                    if suggestions("literal", &mut v, &options) {
                        self.edit.set(&v);
                        done = true;
                    }
                }
                if let Some(n) = g.sheets[sheet].nodes.iter_mut().find(|n| n.id == id)
                    && n.get(&key) != self.edit.as_str()
                {
                    n.data.insert(key, self.edit.as_str().to_string());
                }
                imgui::text_colored(theme::MUTED, "Enter: done");
                if done {
                    gfx::close_popup();
                }
            }
            gfx::end_popup();
        } else {
            self.editing = None;
        }
        event
    }

    /// The node list for a search: built-ins that match, then the API.
    fn results(&self, g: &Graph, sheet: usize, q: &str) -> Vec<Entry> {
        let mut all: Vec<Entry> = builtins(g, sheet).into_iter().filter(|e| q.is_empty() || e.label.to_lowercase().contains(q) || e.group.to_lowercase() == q).collect();
        let object = self.search.from.as_ref().filter(|f| graph::is_object(&f.3)).map(|f| f.3.as_str());
        all.extend(api_entries(q, object));
        if let Some((_, _, output, ty)) = &self.search.from {
            all.retain(|e| e.kind != "comment" && fit(g, sheet, e, *output, ty).is_some());
        }
        all.dedup_by(|a, b| a.label == b.label && a.group == b.group); // overloads the list can't tell apart
        all
    }

    /// Adds a node from the list where the list was opened, wired to the pin it was dragged from.
    fn add(&mut self, g: &mut Graph, sheet: usize, e: &Entry) {
        let at = self.search.at;
        let id = g.new_id();
        if e.kind == "comment" {
            g.sheets[sheet].comments.push(Comment { id, pos: at, size: [320.0, 200.0], text: "Comment".into() });
            return;
        }
        let pin = self.search.from.as_ref().and_then(|(_, _, output, ty)| fit(g, sheet, e, *output, ty));
        let data: Vec<(&str, &str)> = e.data.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        g.sheets[sheet].nodes.push(Node::new(id, &e.kind, at, &data));
        if let (Some((node, from_pin, output, _)), Some(p)) = (self.search.from.clone(), pin) {
            if output {
                connect(g, sheet, node, &from_pin, id, &p);
            } else {
                connect(g, sheet, id, &p, node, &from_pin);
            }
        }
        self.sel = BTreeSet::from([id]);
    }

    pub fn delete(&mut self, g: &mut Graph, sheet: usize) {
        let s = &mut g.sheets[sheet];
        s.nodes.retain(|n| !self.sel.contains(&n.id));
        s.comments.retain(|c| !self.sel.contains(&c.id));
        s.links.retain(|l| !self.sel.contains(&l.from) && !self.sel.contains(&l.to));
        self.sel.clear();
    }

    fn copy(&self, g: &Graph, sheet: usize) {
        let s = &g.sheets[sheet];
        let nodes: Vec<Node> = s.nodes.iter().filter(|n| self.sel.contains(&n.id)).cloned().collect();
        let links = s.links.iter().filter(|l| self.sel.contains(&l.from) && self.sel.contains(&l.to)).cloned().collect();
        if !nodes.is_empty() {
            gfx::set_clipboard(&serde_json::to_string(&Clip { rhai_studio_nodes: nodes, links }).unwrap_or_default());
        }
    }

    /// Pastes nodes copied from a graph, their top-left corner at `at`.
    fn paste(&mut self, g: &mut Graph, sheet: usize, text: &str, at: [f32; 2]) {
        let Ok(clip) = serde_json::from_str::<Clip>(text) else { return };
        let (x0, y0) = clip.rhai_studio_nodes.iter().fold((f32::MAX, f32::MAX), |(x, y), n| (x.min(n.pos[0]), y.min(n.pos[1])));
        let mut ids = HashMap::new();
        self.sel.clear();
        for mut n in clip.rhai_studio_nodes {
            let id = g.new_id();
            ids.insert(n.id, id);
            n.id = id;
            n.pos = [n.pos[0] - x0 + at[0], n.pos[1] - y0 + at[1]];
            self.sel.insert(id);
            g.sheets[sheet].nodes.push(n);
        }
        for l in clip.links {
            if let (Some(&from), Some(&to)) = (ids.get(&l.from), ids.get(&l.to)) {
                g.sheets[sheet].links.push(Link { from, to, ..l });
            }
        }
    }

    /// A comment around the selected nodes (or at the mouse).
    fn comment(&mut self, g: &mut Graph, sheet: usize, o: V2, view: &View, at: [f32; 2]) {
        let s = &g.sheets[sheet];
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for n in s.nodes.iter().filter(|n| self.sel.contains(&n.id)) {
            let l = Self::lay(g, sheet, n, o, view);
            let (a, c) = (Self::to_graph(o, view, l.min), Self::to_graph(o, view, l.max));
            b = (b.0.min(a[0]), b.1.min(a[1]), b.2.max(c[0]), b.3.max(c[1]));
        }
        let (pos, size) = if b.0 == f32::MAX { (at, [320.0, 200.0]) } else { ([b.0 - 20.0, b.1 - 20.0 - COMMENT_HEADER], [b.2 - b.0 + 40.0, b.3 - b.1 + 40.0 + COMMENT_HEADER]) };
        let id = g.new_id();
        g.sheets[sheet].comments.push(Comment { id, pos, size, text: "Comment".into() });
        self.sel = BTreeSet::from([id]);
    }
}

fn draw_pin(at: V2, ty: &str, linked: bool, z: f32) {
    let c = ty_color(ty);
    if ty == EXEC {
        let r = PIN_R * z * 1.2;
        let fill = if linked { c } else { [c[0] * 0.35, c[1] * 0.35, c[2] * 0.35, 1.0] };
        gfx::draw_triangle(v2(at.x - r * 0.7, at.y - r), v2(at.x + r, at.y), v2(at.x - r * 0.7, at.y + r), fill);
    } else if linked {
        gfx::draw_circle(at, PIN_R * z, c, true);
    } else {
        gfx::draw_circle(at, PIN_R * z, theme::FIELD, true);
        gfx::draw_circle(at, PIN_R * z, c, false);
    }
}

/// Wires an output into an input: an input takes one wire, and so does an exec output.
pub fn connect(g: &mut Graph, sheet: usize, from: u32, out: &str, to: u32, inp: &str) {
    let exec = g.sheets[sheet].nodes.iter().find(|n| n.id == from).and_then(|n| g.shape(sheet, n).output(out).map(|p| p.ty == EXEC)).unwrap_or(false);
    let s = &mut g.sheets[sheet];
    s.links.retain(|l| !(l.to == to && l.inp == inp) && !(exec && l.from == from && l.out == out));
    s.links.push(Link { from, out: out.into(), to, inp: inp.into() });
}

/// The pin of a new node that a wire from a pin of type `ty` would go into (or come from, when it was dragged
/// from an input).
fn fit(g: &Graph, sheet: usize, e: &Entry, from_output: bool, ty: &str) -> Option<String> {
    let data: Vec<(&str, &str)> = e.data.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let sh = g.shape(sheet, &Node::new(u32::MAX, &e.kind, [0.0, 0.0], &data));
    if from_output {
        sh.ins.iter().find(|p| graph::compatible(ty, &p.ty) && (ty == EXEC) == (p.ty == EXEC)).map(|p| p.name.clone())
    } else {
        sh.outs.iter().find(|p| graph::compatible(&p.ty, ty) && (ty == EXEC) == (p.ty == EXEC)).map(|p| p.name.clone())
    }
}
