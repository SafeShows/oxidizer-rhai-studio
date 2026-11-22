//! Graph scripts: the graph, what each node looks like (its pins), and the Rhai it compiles to.
//!
//! A graph script is a normal `.rhai` file that Oxidizer runs as it is: the generated code, then the graph as JSON
//! in a comment at the end, with a hash of the code above it, so a hand edit to the code is noticed on open.
//!
//! Sheet 0 is the event graph: event nodes (On Init, On Frame, On Hotkey, a form's button, ...) start chains of
//! exec wires, which compile to the script's callbacks. The other sheets are functions (`fn name(params)`), called
//! as `this.name(...)` so they see the mod's `this`. Variables live in `this` too. Pure nodes (math, getters) have
//! no exec pins and compile to expressions where they are used; other nodes keep their results in locals.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EXEC: &str = "exec";
/// A reroute's type before anything is wired into it.
pub const WILD: &str = "wild";
/// The form designer's grid, in pixels.
pub const GRID: i32 = 8;
/// The top of a window's content in the designer: room for the title bar.
pub const TITLE_ROOM: i32 = 32;

const MARK: &str = "/* rhai-studio-graph ";

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Graph {
    pub sheets: Vec<Sheet>,
    #[serde(default)]
    pub vars: Vec<Var>,
    #[serde(default)]
    pub forms: Vec<Form>,
    /// Wires report their values to the editor (`graph_probe`, Oxidizer 0.14+).
    #[serde(default = "yes")]
    pub live: bool,
    #[serde(default)]
    pub next_id: u32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Sheet {
    pub name: String,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub outputs: Vec<Param>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Param {
    pub name: String,
    pub ty: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Var {
    pub name: String,
    pub ty: String,
    /// A literal of its type, as typed in the details ("1.5", "hello", "true").
    #[serde(default)]
    pub default: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Node {
    pub id: u32,
    pub kind: String,
    pub pos: [f32; 2],
    /// What the node is (`name` of a variable, `path` of a call, `code`, ...) and the literals of its unwired
    /// inputs (`in.<pin>`).
    #[serde(default)]
    pub data: BTreeMap<String, String>,
}

impl Node {
    pub fn new(id: u32, kind: &str, pos: [f32; 2], data: &[(&str, &str)]) -> Node {
        Node { id, kind: kind.into(), pos, data: data.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    pub fn get(&self, key: &str) -> &str {
        self.data.get(key).map_or("", String::as_str)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Link {
    pub from: u32,
    pub out: String,
    pub to: u32,
    pub inp: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Comment {
    pub id: u32,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
}

/// A window made in the form designer. Sizes and positions are pixels on the grid.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Form {
    pub id: u32,
    pub title: String,
    pub size: [i32; 2],
    /// Shown when the mod starts.
    #[serde(default = "yes")]
    pub open: bool,
    /// An entry in the game's Mods menu shows and hides it.
    #[serde(default = "yes")]
    pub menu: bool,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Widget {
    pub id: u32,
    pub kind: String,
    pub label: String,
    /// Top-left corner in the window, and width.
    pub pos: [i32; 2],
    pub width: i32,
    /// `min`, `max` (slider), `items` (combo, one per line), `default` (the starting value).
    #[serde(default)]
    pub data: BTreeMap<String, String>,
}

impl Widget {
    pub fn get(&self, key: &str) -> &str {
        self.data.get(key).map_or("", String::as_str)
    }
}

/// The widgets of the designer's toolbox: kind, name, type of its value ("" for none), width.
pub const WIDGETS: &[(&str, &str, &str, i32)] = &[
    ("button", "Button", "", 96),
    ("text", "Text", "string", 160),
    ("checkbox", "Checkbox", "bool", 160),
    ("slider", "Slider", "float", 200),
    ("input_text", "Text field", "string", 200),
    ("input_float", "Number", "float", 200),
    ("input_int", "Integer", "int", 200),
    ("combo", "Drop-down", "int", 200),
    ("color", "Color", "array", 240),
    ("progress", "Progress", "float", 200),
    ("separator", "Separator", "", 200),
];

pub fn widget_type(kind: &str) -> &'static str {
    WIDGETS.iter().find(|w| w.0 == kind).map_or("", |w| w.2)
}

impl Graph {
    /// A new graph: the event graph and nothing on it.
    pub fn empty() -> Graph {
        Graph { sheets: vec![Sheet { name: "Event Graph".into(), ..Default::default() }], vars: vec![], forms: vec![], live: true, next_id: 1 }
    }

    pub fn new_id(&mut self) -> u32 {
        let used = self.sheets.iter().flat_map(|s| s.nodes.iter().map(|n| n.id).chain(s.comments.iter().map(|c| c.id)));
        let used = used.chain(self.forms.iter().flat_map(|f| std::iter::once(f.id).chain(f.widgets.iter().map(|w| w.id))));
        self.next_id = self.next_id.max(used.max().unwrap_or(0) + 1);
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn form(&self, id: &str) -> Option<&Form> {
        self.forms.iter().find(|f| f.id.to_string() == id)
    }

    pub fn widget(&self, form: &str, id: &str) -> Option<(&Form, &Widget)> {
        let f = self.form(form)?;
        Some((f, f.widgets.iter().find(|w| w.id.to_string() == id)?))
    }

    pub fn function(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().skip(1).find(|s| s.name == name)
    }

    /// A new mod's graph: a form with a line of text and a button that prints.
    pub fn starter(id: &str) -> Graph {
        let mut g = Graph::empty();
        let (form, text, button) = (g.new_id(), g.new_id(), g.new_id());
        let widget = |id: u32, kind: &str, label: &str, pos: [i32; 2], width: i32| Widget { id, kind: kind.into(), label: label.into(), pos, width, data: BTreeMap::new() };
        g.forms.push(Form {
            id: form,
            title: id.into(),
            size: [320, 120],
            open: true,
            menu: true,
            widgets: vec![widget(text, "text", "Hello from a graph!", [8, 40], 240), widget(button, "button", "Say hi", [8, 72], 96)],
        });
        let (init, print, click, print2) = (g.new_id(), g.new_id(), g.new_id(), g.new_id());
        let (f, b) = (form.to_string(), button.to_string());
        let s = &mut g.sheets[0];
        s.nodes.push(Node::new(init, "event.init", [40.0, 40.0], &[]));
        s.nodes.push(Node::new(print, "print", [280.0, 40.0], &[("in.text", &format!("{id}: hello!"))]));
        s.nodes.push(Node::new(click, "event.widget", [40.0, 180.0], &[("form", &f), ("widget", &b)]));
        s.nodes.push(Node::new(print2, "print", [280.0, 180.0], &[("in.text", "hi!")]));
        s.links.push(Link { from: init, out: "then".into(), to: print, inp: "exec".into() });
        s.links.push(Link { from: click, out: "then".into(), to: print2, inp: "exec".into() });
        g
    }
}

// ---- types ----

/// A pin type from a definition file's type: "float", "int", "bool", "string", "array", "map", "any", "" (no
/// value), or a game type's name ("Vehicle"; "Obj" is any game object).
pub fn ty_from_def(t: &str) -> String {
    match t.trim() {
        "float" | "f64" | "f32" | "FLOAT" | "double" => "float",
        "int" | "i64" | "i32" | "INT" | "u8" | "u32" | "u64" | "i16" | "u16" => "int",
        "bool" => "bool",
        "string" | "String" | "ImmutableString" | "&str" | "char" | "LString" => "string",
        "array" | "Array" => "array",
        "map" | "Map" => "map",
        "?" | "Dynamic" | "" | "..." => "any",
        "()" => "",
        other => return other.to_string(),
    }
    .into()
}

pub const PRIMITIVES: &[&str] = &["bool", "int", "float", "string", "array", "map", "any"];

pub fn is_object(t: &str) -> bool {
    !t.is_empty() && t != EXEC && t != WILD && !PRIMITIVES.contains(&t)
}

/// Whether a wire may go from an output of type `from` into an input of type `to`. Ints go into floats (the code
/// converts them); "any" takes every value and "Obj" every game object. Other game types must match: a Cast node
/// says what an object is.
pub fn compatible(from: &str, to: &str) -> bool {
    if from == WILD || to == WILD {
        return true;
    }
    if from == EXEC || to == EXEC {
        return from == to;
    }
    from == to || to == "any" || from == "any" || (from == "int" && to == "float") || (to == "Obj" && is_object(from)) || (from == "Obj" && is_object(to))
}

/// A literal for an unwired input of type `ty`, from what was typed for it.
pub fn literal(ty: &str, text: &str) -> String {
    let t = text.trim();
    match ty {
        "float" => {
            let v = t.parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
            let s = format!("{v:?}");
            match (s.contains('.'), s.find('e')) {
                (true, _) => s,
                (false, Some(i)) => format!("{}.0{}", &s[..i], &s[i..]),
                (false, None) => s + ".0",
            }
        }
        "int" => t.parse::<f64>().map_or(0, |v| v as i64).to_string(),
        "bool" => (t == "true").to_string(),
        "string" => serde_json::to_string(text).unwrap_or_default(), // JSON's escapes are Rhai's too
        // A value of any type: what it looks like (code goes in a Rhai Expression node).
        "any" if t.is_empty() => "()".into(),
        "any" if t == "true" || t == "false" || t.parse::<i64>().is_ok() => t.into(),
        "any" if t.parse::<f64>().is_ok_and(f64::is_finite) => literal("float", t),
        "any" => literal("string", text),
        "array" if t.is_empty() => "[]".into(),
        "map" if t.is_empty() => "#{}".into(),
        _ if t.is_empty() => "()".into(),
        _ => format!("({t})"), // Rhai code, as typed
    }
}

// ---- nodes ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cat {
    Event,
    Flow,
    Call,
    Pure,
    Var,
    Custom,
    Form,
    Function,
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub name: String,
    pub ty: String,
    pub label: String,
}

fn pin(name: &str, ty: &str) -> Pin {
    Pin { name: name.into(), ty: ty.into(), label: if name == EXEC || name == "then" { String::new() } else { name.replace('_', " ") } }
}

fn labeled(name: &str, ty: &str, label: &str) -> Pin {
    Pin { name: name.into(), ty: ty.into(), label: label.into() }
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub title: String,
    pub cat: Cat,
    pub ins: Vec<Pin>,
    pub outs: Vec<Pin>,
}

impl Shape {
    /// No exec pins: compiles to an expression where its outputs are used.
    pub fn pure(&self) -> bool {
        !self.ins.iter().chain(&self.outs).any(|p| p.ty == EXEC)
    }

    pub fn input(&self, name: &str) -> Option<&Pin> {
        self.ins.iter().find(|p| p.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&Pin> {
        self.outs.iter().find(|p| p.name == name)
    }
}

/// The callbacks an event node can be: kind, title, the function, its parameters (also the node's outputs).
pub const EVENTS: &[(&str, &str, &str, &[&str])] = &[
    ("event.init", "On Init", "init", &[]),
    ("event.loaded", "On Game Loaded", "loaded", &[]),
    ("event.reload", "On Reload", "reload", &[]),
    ("event.frame", "On Frame", "frame", &["t", "dt"]),
    ("event.gui", "On GUI", "gui", &["dt"]),
    ("event.menu", "On Mods Menu", "menu", &[]),
    ("event.saving", "On Saving", "saving", &[]),
    ("event.save_loaded", "On Save Loaded", "save_loaded", &[]),
    ("event.unload", "On Unload", "unload", &[]),
];

/// Operators: symbol, operand type, result type.
pub const OPS: &[(&str, &str, &str)] = &[
    ("+", "float", "float"),
    ("-", "float", "float"),
    ("*", "float", "float"),
    ("/", "float", "float"),
    ("+", "int", "int"),
    ("-", "int", "int"),
    ("*", "int", "int"),
    ("/", "int", "int"),
    ("%", "int", "int"),
    ("+", "string", "string"),
    ("<", "float", "bool"),
    (">", "float", "bool"),
    ("<=", "float", "bool"),
    (">=", "float", "bool"),
    ("<", "int", "bool"),
    (">", "int", "bool"),
    ("==", "any", "bool"),
    ("!=", "any", "bool"),
    ("&&", "bool", "bool"),
    ("||", "bool", "bool"),
    ("!", "bool", "bool"),
];

/// Parses an API signature, "(speed: float, paused: bool) -> ()" or ": float", into parameters and a return type.
pub fn parse_sig(sig: &str) -> (Vec<Param>, String) {
    if let Some(t) = sig.strip_prefix(':') {
        return (vec![], ty_from_def(t));
    }
    let inner = sig.strip_prefix('(').and_then(|s| s.split_once(')'));
    let Some((params, rest)) = inner else { return (vec![], String::new()) };
    let ret = ty_from_def(rest.trim().trim_start_matches("->").trim());
    let params = params
        .split(", ")
        .filter(|p| !p.is_empty())
        .enumerate()
        .map(|(i, p)| match p.split_once(':') {
            Some((n, t)) => Param { name: n.trim().trim_start_matches("...").into(), ty: ty_from_def(t) },
            None => Param { name: format!("arg{}", i + 1), ty: ty_from_def(p) },
        })
        .collect();
    (params, ret)
}

impl Graph {
    /// What a node looks like: its title, color and pins.
    pub fn shape(&self, sheet: usize, n: &Node) -> Shape {
        let exec_in = || vec![pin(EXEC, EXEC)];
        let then = || vec![pin("then", EXEC)];
        let s = |title: &str, cat, ins: Vec<Pin>, outs: Vec<Pin>| Shape { title: title.into(), cat, ins, outs };
        let k = n.kind.as_str();
        if let Some((_, title, _, params)) = EVENTS.iter().find(|e| e.0 == k) {
            return s(title, Cat::Event, vec![], then().into_iter().chain(params.iter().map(|p| pin(p, "float"))).collect());
        }
        match k {
            "event.hotkey" => s(&format!("On Key {}", n.get("key")), Cat::Event, vec![], then()),
            "event.widget" => {
                let title = match self.widget(n.get("form"), n.get("widget")) {
                    Some((f, w)) if w.kind == "button" => format!("On {} clicked ({})", w.label, f.title),
                    Some((f, w)) => format!("On {} changed ({})", w.label, f.title),
                    None => "On widget (deleted)".into(),
                };
                s(&title, Cat::Event, vec![], then())
            }
            "branch" => s("Branch", Cat::Flow, [exec_in(), vec![pin("condition", "bool")]].concat(), vec![labeled("true", EXEC, "true"), labeled("false", EXEC, "false")]),
            "sequence" => {
                let count = n.get("count").parse::<usize>().unwrap_or(2).clamp(1, 16);
                s("Sequence", Cat::Flow, exec_in(), (0..count).map(|i| labeled(&format!("then{i}"), EXEC, &format!("then {i}"))).collect())
            }
            "for" => s(
                "For Loop",
                Cat::Flow,
                [exec_in(), vec![pin("first", "int"), pin("last", "int")]].concat(),
                vec![labeled("body", EXEC, "loop body"), pin("index", "int"), labeled("done", EXEC, "completed")],
            ),
            "for_each" => s(
                "For Each",
                Cat::Flow,
                [exec_in(), vec![pin("array", "array")]].concat(),
                vec![labeled("body", EXEC, "loop body"), pin("item", "any"), pin("index", "int"), labeled("done", EXEC, "completed")],
            ),
            "while" => s("While", Cat::Flow, [exec_in(), vec![pin("condition", "bool")]].concat(), vec![labeled("body", EXEC, "loop body"), labeled("done", EXEC, "completed")]),
            "break" => s("Break", Cat::Flow, exec_in(), vec![]),
            "print" => s("Print", Cat::Call, [exec_in(), vec![pin("text", "any")]].concat(), then()),
            "custom" => s("Rhai Code", Cat::Custom, exec_in(), then()),
            "expr" => s("Rhai Expression", Cat::Custom, vec![], vec![pin("value", "any")]),
            "script" => s("Script Code", Cat::Custom, vec![], vec![]),
            "literal" => {
                let ty = n.get("ty");
                s(&format!("{} value", if ty.is_empty() { "float" } else { ty }), Cat::Pure, vec![], vec![pin("value", if ty.is_empty() { "float" } else { ty })])
            }
            "op" => {
                let (op, ty) = (n.get("op"), n.get("ty"));
                let ret = OPS.iter().find(|o| o.0 == op && o.1 == ty).map_or("any", |o| o.2);
                let ins = if op == "!" { vec![pin("a", ty)] } else { vec![pin("a", ty), pin("b", ty)] };
                let title = match op {
                    "!" => "NOT".to_string(),
                    "&&" => "AND".into(),
                    "||" => "OR".into(),
                    _ => format!("{op}  ({ty})"),
                };
                s(&title, Cat::Pure, ins, vec![pin("result", ret)])
            }
            "convert" => {
                let to = n.get("to");
                s(&format!("To {to}"), Cat::Pure, vec![pin("value", "any")], vec![pin("result", to)])
            }
            "cast" => {
                let ty = n.get("ty");
                s(&format!("Cast to {ty}"), Cat::Pure, vec![pin("object", "Obj")], vec![pin("as", if ty.is_empty() { "Obj" } else { ty })])
            }
            "var.get" | "var.set" => {
                let name = n.get("name");
                let ty = self.vars.iter().find(|v| v.name == name).map_or("any", |v| v.ty.as_str());
                if k == "var.get" {
                    s(&format!("Get {name}"), Cat::Var, vec![], vec![labeled("value", ty, name)])
                } else {
                    s(&format!("Set {name}"), Cat::Var, [exec_in(), vec![labeled("value", ty, name)]].concat(), then())
                }
            }
            "widget.get" | "widget.set" => {
                let (label, ty) = match self.widget(n.get("form"), n.get("widget")) {
                    Some((_, w)) => (w.label.clone(), widget_type(&w.kind)),
                    None => ("(deleted)".into(), "any"),
                };
                if k == "widget.get" {
                    s(&format!("Get {label}"), Cat::Form, vec![], vec![pin("value", ty)])
                } else {
                    s(&format!("Set {label}"), Cat::Form, [exec_in(), vec![pin("value", ty)]].concat(), then())
                }
            }
            "form.open" => {
                let title = self.form(n.get("form")).map_or("(deleted)", |f| f.title.as_str());
                s(&format!("Show {title}"), Cat::Form, [exec_in(), vec![pin("shown", "bool")]].concat(), then())
            }
            "form.is_open" => {
                let title = self.form(n.get("form")).map_or("(deleted)", |f| f.title.as_str());
                s(&format!("Is {title} shown"), Cat::Form, vec![], vec![pin("shown", "bool")])
            }
            "fn.entry" => {
                let sh = &self.sheets[sheet.min(self.sheets.len() - 1)];
                s(&sh.name, Cat::Function, vec![], then().into_iter().chain(sh.params.iter().map(|p| pin(&p.name, &p.ty))).collect())
            }
            "fn.return" => {
                let sh = &self.sheets[sheet.min(self.sheets.len() - 1)];
                s("Return", Cat::Function, exec_in().into_iter().chain(sh.outputs.iter().map(|p| pin(&p.name, &p.ty))).collect(), vec![])
            }
            "fn.call" => {
                let name = n.get("fn");
                let (ins, outs) = self.function(name).map(|f| (f.params.clone(), f.outputs.clone())).unwrap_or_default();
                s(name, Cat::Function, exec_in().into_iter().chain(ins.iter().map(|p| pin(&p.name, &p.ty))).collect(), then().into_iter().chain(outs.iter().map(|p| pin(&p.name, &p.ty))).collect())
            }
            "reroute" => {
                let ty = self.reroute_type(sheet, n.id, 0);
                s("", Cat::Pure, vec![labeled("in", &ty, "")], vec![labeled("out", &ty, "")])
            }
            "api" => {
                let (params, ret) = parse_sig(n.get("sig"));
                let name = n.get("path").rsplit("::").next().unwrap_or("").to_string();
                let target = || labeled("target", "Obj", "target");
                let pure = n.get("pure") == "1";
                let flow = |ins: Vec<Pin>| if pure { ins } else { [exec_in(), ins].concat() };
                let out = |ret: &str| {
                    let mut o = if pure { vec![] } else { then() };
                    if !ret.is_empty() {
                        o.push(labeled("ret", ret, "return value"));
                    }
                    o
                };
                let params: Vec<Pin> = params.iter().map(|p| pin(&p.name, &p.ty)).collect();
                match n.get("member") {
                    "get" => s(&format!("Get .{name}"), Cat::Pure, vec![target()], vec![labeled("ret", &ret, &name)]),
                    "set" => s(&format!("Set .{name}"), Cat::Call, [exec_in(), vec![target(), labeled("value", &ret, &name)]].concat(), then()),
                    "method" => s(&format!(".{name}"), if pure { Cat::Pure } else { Cat::Call }, flow([vec![target()], params].concat()), out(&ret)),
                    _ => {
                        let path = n.get("path");
                        let title = path.rsplitn(3, "::").take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("::");
                        s(&title, if pure { Cat::Pure } else { Cat::Call }, flow(params), out(&ret))
                    }
                }
            }
            _ => s(&format!("Unknown node {k}"), Cat::Custom, vec![], vec![]),
        }
    }

    /// A reroute takes the type of what is wired into it.
    fn reroute_type(&self, sheet: usize, id: u32, depth: u32) -> String {
        let Some(s) = self.sheets.get(sheet) else { return WILD.into() };
        let Some(l) = s.links.iter().find(|l| l.to == id && l.inp == "in") else { return WILD.into() };
        let Some(src) = s.nodes.iter().find(|n| n.id == l.from) else { return WILD.into() };
        if src.kind == "reroute" {
            return if depth > 32 { WILD.into() } else { self.reroute_type(sheet, src.id, depth + 1) };
        }
        self.shape(sheet, src).output(&l.out).map_or(WILD.into(), |p| p.ty.clone())
    }
}

// ---- code ----

/// A compiled graph: the code, which node each statement came from, and what is wrong.
#[derive(Default, Debug)]
pub struct Compiled {
    pub code: String,
    /// (line, 1-based; sheet; node)
    pub lines: Vec<(usize, usize, u32)>,
    pub errors: Vec<(usize, u32, String)>,
}

impl Compiled {
    pub fn line_of(&self, sheet: usize, node: u32) -> Option<usize> {
        self.lines.iter().find(|l| l.1 == sheet && l.2 == node).map(|l| l.0)
    }

    /// The node a line belongs to: the statement starting there, or the nearest one above.
    pub fn node_at(&self, line: usize) -> Option<(usize, u32)> {
        self.lines.iter().filter(|l| l.0 <= line).max_by_key(|l| l.0).map(|l| (l.1, l.2))
    }
}

struct Gen<'a> {
    g: &'a Graph,
    /// What `this` keys of forms and widgets start with: the file's name, so two graphs of a mod don't share them.
    stem: String,
    sheet: usize,
    out: Vec<(String, Option<(usize, u32)>)>,
    locals: BTreeSet<String>,
    errors: Vec<(usize, u32, String)>,
    /// Exec chain being written (a node that comes back is a loop) and expressions being written.
    chain: Vec<u32>,
    exprs: Vec<u32>,
}

fn ident(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The name a variable, parameter or function gets in code: letters, digits and '_'.
pub fn code_name(s: &str) -> String {
    ident(s)
}

impl Gen<'_> {
    fn nodes(&self) -> &[Node] {
        &self.g.sheets[self.sheet].nodes
    }

    fn node(&self, id: u32) -> Option<&Node> {
        self.nodes().iter().find(|n| n.id == id)
    }

    fn links(&self) -> &[Link] {
        &self.g.sheets[self.sheet].links
    }

    fn line(&mut self, ind: usize, s: &str, node: Option<u32>) {
        self.out.push((format!("{}{s}", "    ".repeat(ind)), node.map(|n| (self.sheet, n))));
    }

    fn err(&mut self, node: u32, msg: &str) {
        if !self.errors.iter().any(|e| e.0 == self.sheet && e.1 == node && e.2 == msg) {
            self.errors.push((self.sheet, node, msg.into()));
        }
    }

    fn probe(&self, key: String, e: String) -> String {
        if self.g.live { format!("graph_probe({}, {e})", serde_json::to_string(&key).unwrap_or_default()) } else { e }
    }

    fn form_key(&self, form: &str) -> String {
        format!("this.__{}_f{form}", self.stem)
    }

    fn widget_key(&self, widget: &str) -> String {
        format!("this.__{}_w{widget}", self.stem)
    }

    fn var_default(&self, name: &str) -> String {
        self.g.vars.iter().find(|v| v.name == name).map_or("()".into(), |v| literal(&v.ty, &v.default))
    }

    fn widget_default(&self, w: &Widget) -> String {
        let ty = widget_type(&w.kind);
        match w.kind.as_str() {
            "text" if w.get("default").is_empty() => literal("string", &w.label),
            "color" if w.get("default").is_empty() => "[1.0, 1.0, 1.0, 1.0]".into(),
            "combo" if w.get("default").is_empty() => "0".into(),
            _ => literal(ty, w.get("default")),
        }
    }

    /// The code of an input: what is wired into it, else its literal.
    fn input(&mut self, n: &Node, sh: &Shape, name: &str) -> String {
        let Some(p) = sh.input(name) else { return "()".into() };
        let to = p.ty.clone();
        match self.links().iter().find(|l| l.to == n.id && l.inp == name).cloned() {
            Some(l) => {
                let (e, from) = self.output(l.from, &l.out);
                if from == "int" && to == "float" { format!("({e}).to_float()") } else { e }
            }
            None => literal(&to, n.get(&format!("in.{name}"))),
        }
    }

    /// The code of a node's output, and its type.
    fn output(&mut self, id: u32, out: &str) -> (String, String) {
        let Some(n) = self.node(id).cloned() else { return ("()".into(), "any".into()) };
        let sh = self.g.shape(self.sheet, &n);
        let ty = sh.output(out).map_or("any".to_string(), |p| p.ty.clone());
        if !sh.pure() || n.kind.starts_with("event.") || n.kind == "fn.entry" {
            let local = format!("n{id}_{out}");
            self.locals.insert(local.clone());
            return (local, ty);
        }
        if self.exprs.contains(&id) {
            self.err(id, "its value depends on itself");
            return ("()".into(), ty);
        }
        self.exprs.push(id);
        let e = self.pure(&n, &sh, out);
        self.exprs.pop();
        let e = if n.kind == "reroute" || n.kind == "literal" { e } else { self.probe(format!("{id}.{out}"), e) };
        (e, ty)
    }

    fn args(&mut self, n: &Node, sh: &Shape, skip: usize) -> String {
        let names: Vec<String> = sh.ins.iter().filter(|p| p.ty != EXEC).skip(skip).map(|p| p.name.clone()).collect();
        names.iter().map(|p| self.input(n, sh, p)).collect::<Vec<_>>().join(", ")
    }

    /// A call to a function of the game's API, Oxidizer's or Rhai's.
    fn call(&mut self, n: &Node, sh: &Shape) -> String {
        let path = n.get("path").to_string();
        match n.get("member") {
            "get" => format!("{}.{}", self.input(n, sh, "target"), path),
            "method" => {
                let target = self.input(n, sh, "target");
                format!("{target}.{path}({})", self.args(n, sh, 1))
            }
            _ => format!("{path}({})", self.args(n, sh, 0)),
        }
    }

    fn pure(&mut self, n: &Node, sh: &Shape, out: &str) -> String {
        match n.kind.as_str() {
            "literal" => literal(if n.get("ty").is_empty() { "float" } else { n.get("ty") }, n.get("value")),
            "op" => {
                let op = n.get("op");
                let a = self.input(n, sh, "a");
                if op == "!" {
                    return format!("!({a})");
                }
                let b = self.input(n, sh, "b");
                format!("({a} {op} {b})")
            }
            "convert" => {
                let v = self.input(n, sh, "value");
                match n.get("to") {
                    "string" => format!("({v}).to_string()"),
                    "int" => format!("({v}).to_int()"),
                    _ => format!("({v}).to_float()"),
                }
            }
            "cast" => self.input(n, sh, "object"),
            "reroute" => self.input(n, sh, "in"),
            "expr" => format!("({})", n.get("code").trim()),
            "var.get" => {
                let name = n.get("name");
                format!("(this.{} ?? {})", ident(name), self.var_default(name))
            }
            "widget.get" => match self.g.widget(n.get("form"), n.get("widget")) {
                Some((_, w)) => format!("({} ?? {})", self.widget_key(&w.id.to_string()), self.widget_default(w)),
                None => {
                    self.err(n.id, "its widget was deleted");
                    "()".into()
                }
            },
            "form.is_open" => match self.g.form(n.get("form")) {
                Some(f) => format!("({} ?? {})", self.form_key(&f.id.to_string()), f.open),
                None => "false".into(),
            },
            "api" => self.call(n, sh),
            k => {
                self.err(n.id, &format!("can't use {k} as a value ({out})"));
                "()".into()
            }
        }
    }

    /// Follows the exec wires out of a pin, writing each node they reach.
    fn follow(&mut self, id: u32, pin: &str, ind: usize) {
        let next: Vec<u32> = self.links().iter().filter(|l| l.from == id && l.out == pin).map(|l| l.to).collect();
        for to in next {
            self.stmt(to, ind);
        }
    }

    fn stmt(&mut self, id: u32, ind: usize) {
        let Some(n) = self.node(id).cloned() else { return };
        if self.chain.contains(&id) {
            self.err(id, "the exec wires loop back here: use a While or For Loop node to repeat");
            return;
        }
        self.chain.push(id);
        let sh = self.g.shape(self.sheet, &n);
        let at = Some(id);
        match n.kind.as_str() {
            "branch" => {
                let c = self.input(&n, &sh, "condition");
                self.line(ind, &format!("if {c} {{"), at);
                self.follow(id, "true", ind + 1);
                if self.links().iter().any(|l| l.from == id && l.out == "false") {
                    self.line(ind, "} else {", None);
                    self.follow(id, "false", ind + 1);
                }
                self.line(ind, "}", None);
            }
            "sequence" => {
                for p in &sh.outs {
                    self.follow(id, &p.name, ind);
                }
            }
            "for" | "for_each" => {
                let head = if n.kind == "for" {
                    let (a, b) = (self.input(&n, &sh, "first"), self.input(&n, &sh, "last"));
                    format!("for _i in {a}..={b} {{")
                } else {
                    format!("for (_v, _i) in {} {{", self.input(&n, &sh, "array"))
                };
                self.line(ind, &head, at);
                if n.kind == "for_each" {
                    self.locals.insert(format!("n{id}_item"));
                    let v = self.probe(format!("{id}.item"), "_v".into());
                    self.line(ind + 1, &format!("n{id}_item = {v};"), None);
                }
                self.locals.insert(format!("n{id}_index"));
                let i = self.probe(format!("{id}.index"), "_i".into());
                self.line(ind + 1, &format!("n{id}_index = {i};"), None);
                self.follow(id, "body", ind + 1);
                self.line(ind, "}", None);
                self.follow(id, "done", ind);
            }
            "while" => {
                let c = self.input(&n, &sh, "condition");
                self.line(ind, &format!("while {c} {{"), at);
                self.follow(id, "body", ind + 1);
                self.line(ind, "}", None);
                self.follow(id, "done", ind);
            }
            "break" => self.line(ind, "break;", at),
            "print" => {
                let t = self.input(&n, &sh, "text");
                self.line(ind, &format!("print({t});"), at);
                self.follow(id, "then", ind);
            }
            "custom" => {
                let code = n.get("code").to_string();
                let mut first = true;
                for l in code.lines().filter(|l| !l.trim().is_empty()) {
                    self.line(ind, l.trim_end(), if first { at } else { None });
                    first = false;
                }
                self.follow(id, "then", ind);
            }
            "var.set" => {
                let v = self.input(&n, &sh, "value");
                self.line(ind, &format!("this.{} = {v};", ident(n.get("name"))), at);
                self.follow(id, "then", ind);
            }
            "widget.set" => {
                let v = self.input(&n, &sh, "value");
                match self.g.widget(n.get("form"), n.get("widget")) {
                    Some((_, w)) => {
                        let key = self.widget_key(&w.id.to_string());
                        self.line(ind, &format!("{key} = {v};"), at);
                    }
                    None => self.err(id, "its widget was deleted"),
                }
                self.follow(id, "then", ind);
            }
            "form.open" => {
                let v = self.input(&n, &sh, "shown");
                let key = self.form_key(n.get("form"));
                self.line(ind, &format!("{key} = {v};"), at);
                self.follow(id, "then", ind);
            }
            "fn.call" => {
                let name = n.get("fn").to_string();
                let args = self.args(&n, &sh, 0);
                let outs: Vec<String> = sh.outs.iter().filter(|p| p.ty != EXEC).map(|p| p.name.clone()).collect();
                if self.g.function(&name).is_none() {
                    self.err(id, "its function was deleted");
                } else if outs.is_empty() {
                    self.line(ind, &format!("this.{}({args});", ident(&name)), at);
                } else {
                    self.line(ind, &format!("let _r = this.{}({args});", ident(&name)), at);
                    for o in outs {
                        self.locals.insert(format!("n{id}_{o}"));
                        let v = self.probe(format!("{id}.{o}"), format!("_r.{}", ident(&o)));
                        self.line(ind, &format!("n{id}_{o} = {v};"), None);
                    }
                }
                self.follow(id, "then", ind);
            }
            "fn.return" => {
                let fields: Vec<String> = sh.ins.iter().filter(|p| p.ty != EXEC).map(|p| p.name.clone()).collect();
                let fields: Vec<String> = fields.iter().map(|f| format!("{}: {}", ident(f), self.input(&n, &sh, f))).collect();
                self.line(ind, &if fields.is_empty() { "return;".into() } else { format!("return #{{{}}};", fields.join(", ")) }, at);
            }
            "reroute" => self.follow(id, "out", ind),
            "api" => {
                let call = self.call(&n, &sh);
                if n.get("member") == "set" {
                    let v = self.input(&n, &sh, "value");
                    self.line(ind, &format!("{call} = {v};"), at);
                } else if sh.output("ret").is_some() {
                    self.locals.insert(format!("n{id}_ret"));
                    let v = self.probe(format!("{id}.ret"), call);
                    self.line(ind, &format!("n{id}_ret = {v};"), at);
                } else {
                    self.line(ind, &format!("{call};"), at);
                }
                self.follow(id, "then", ind);
            }
            k if k.starts_with("event.") || k == "fn.entry" => self.err(id, "an event starts a chain: nothing can run it"),
            k => self.err(id, &format!("{k} doesn't run (it has no exec input)")),
        }
        self.chain.pop();
    }

    /// An event node's chain: its outputs (the callback's parameters) into locals, then what it runs.
    fn event(&mut self, n: &Node, params: &[&str], ind: usize) {
        for p in params {
            if self.links().iter().any(|l| l.from == n.id && l.out == *p) {
                self.locals.insert(format!("n{}_{p}", n.id));
                let v = self.probe(format!("{}.{p}", n.id), p.to_string());
                self.line(ind, &format!("n{}_{p} = {v};", n.id), Some(n.id));
            }
        }
        self.follow(n.id, "then", ind);
    }

    fn events(&self, kind: &str) -> Vec<Node> {
        self.nodes().iter().filter(|n| n.kind == kind).cloned().collect()
    }

    /// Writes `fn head {`, the locals the body uses, the body and `}`; nothing when the body is empty.
    fn function(&mut self, head: &str, body: impl FnOnce(&mut Self)) {
        let (outer, locals) = (std::mem::take(&mut self.out), std::mem::take(&mut self.locals));
        body(self);
        let (inner, used) = (std::mem::replace(&mut self.out, outer), std::mem::replace(&mut self.locals, locals));
        if inner.is_empty() {
            return;
        }
        self.out.push((String::new(), None));
        self.line(0, &format!("fn {head} {{"), None);
        for l in &used {
            self.line(1, &format!("let {l} = ();"), None);
        }
        self.out.extend(inner);
        self.line(0, "}", None);
    }

    /// A form's window, in gui(): its widgets where the designer put them, and the chains of their events.
    fn form(&mut self, f: &Form) {
        let key = self.form_key(&f.id.to_string());
        self.line(1, &format!("if {key} ?? {} {{", f.open), None);
        self.line(2, &format!("set_next_window_size({}, {});", literal("float", &f.size[0].to_string()), literal("float", &f.size[1].to_string())), None);
        let title = serde_json::to_string(&format!("{}##{}_{}", f.title, self.stem, f.id)).unwrap_or_default();
        self.line(2, &format!("if begin({title}) {{"), None);
        let mut widgets = f.widgets.clone();
        widgets.sort_by_key(|w| (w.pos[1], w.pos[0]));
        for w in &widgets {
            let fl = |v: i32| literal("float", &v.to_string());
            self.line(3, &format!("set_cursor_pos({}, {});", fl(w.pos[0]), fl(w.pos[1])), None);
            self.line(3, &format!("set_next_item_width({});", fl(w.width)), None);
            let wkey = self.widget_key(&w.id.to_string());
            let label = serde_json::to_string(&format!("{}##w{}", w.label, w.id)).unwrap_or_default();
            let (cur, d) = (wkey.clone(), self.widget_default(w));
            let widget_call = match w.kind.as_str() {
                "button" => {
                    self.line(3, &format!("if button({label}) {{"), None);
                    self.widget_events(f, w, 4);
                    self.line(3, "}", None);
                    continue;
                }
                "text" => {
                    self.line(3, &format!("text(({cur} ?? {d}).to_string());"), None);
                    continue;
                }
                "progress" => {
                    self.line(3, &format!("progress_bar({cur} ?? {d}, \"\");"), None);
                    continue;
                }
                "separator" => {
                    self.line(3, "separator();", None);
                    continue;
                }
                "checkbox" => format!("checkbox({label}, _old)"),
                "slider" => {
                    let (min, max) = (literal("float", w.get("min")), literal("float", if w.get("max").is_empty() { "1" } else { w.get("max") }));
                    format!("slider({label}, _old, {min}, {max})")
                }
                "input_text" => format!("input_text({label}, _old)"),
                "input_float" => format!("input_float({label}, _old)"),
                "input_int" => format!("input_int({label}, _old)"),
                "combo" => {
                    let items: Vec<String> = w.get("items").lines().map(|i| serde_json::to_string(i.trim()).unwrap_or_default()).collect();
                    format!("combo({label}, _old, [{}])", items.join(", "))
                }
                "color" => format!("color_edit({label}, _old)"),
                _ => continue,
            };
            self.line(3, "{", None);
            self.line(4, &format!("let _old = {cur} ?? {d};"), None);
            self.line(4, &format!("{cur} = {widget_call};"), None);
            let has_events = self.nodes().iter().any(|n| n.kind == "event.widget" && n.get("widget") == w.id.to_string());
            if has_events {
                self.line(4, &format!("if {cur} != _old {{"), None);
                self.widget_events(f, w, 5);
                self.line(4, "}", None);
            }
            self.line(3, "}", None);
        }
        self.line(2, "}", None);
        self.line(2, "end();", None);
        self.line(1, "}", None);
    }

    fn widget_events(&mut self, f: &Form, w: &Widget, ind: usize) {
        let (fid, wid) = (f.id.to_string(), w.id.to_string());
        for n in self.events("event.widget").into_iter().filter(|n| n.get("form") == fid && n.get("widget") == wid) {
            self.event(&n, &[], ind);
        }
    }
}

/// Compiles a graph to Rhai. `stem` is the file's name without `.rhai`.
pub fn compile(g: &Graph, stem: &str) -> Compiled {
    let mut cg = Gen { g, stem: ident(stem), sheet: 0, out: vec![], locals: BTreeSet::new(), errors: vec![], chain: vec![], exprs: vec![] };
    cg.line(0, "// Made with Rhai Studio's graph editor: open it there to change it. Edits to this code are lost when the graph is saved.", None);
    for n in cg.events("script") {
        cg.out.push((String::new(), None));
        for (i, l) in n.get("code").lines().enumerate() {
            cg.line(0, l.trim_end(), (i == 0).then_some(n.id));
        }
    }
    let reserved: Vec<&str> = EVENTS.iter().map(|e| e.2).collect();
    for (kind, _, name, params) in EVENTS {
        let head = format!("{name}({})", params.join(", "));
        cg.function(&head, |cg| {
            if *name == "init" {
                for v in &g.vars {
                    let (k, d) = (ident(&v.name), literal(&v.ty, &v.default));
                    cg.line(1, &format!("if this.{k} == () {{ this.{k} = {d}; }}"), None);
                }
            }
            if *name == "frame" {
                for n in cg.events("event.hotkey") {
                    let key = serde_json::to_string(n.get("key")).unwrap_or_default();
                    cg.line(1, &format!("if key_pressed({key}) {{"), Some(n.id));
                    cg.event(&n, &[], 2);
                    cg.line(1, "}", None);
                }
            }
            if *name == "menu" {
                for f in g.forms.iter().filter(|f| f.menu) {
                    let key = cg.form_key(&f.id.to_string());
                    let title = serde_json::to_string(&f.title).unwrap_or_default();
                    cg.line(1, &format!("if menu_item({title}, {key} ?? {}) {{ {key} = !({key} ?? {}); }}", f.open, f.open), None);
                }
            }
            for n in cg.events(kind) {
                cg.event(&n, params, 1);
            }
            if *name == "gui" {
                for f in &g.forms {
                    cg.form(f);
                }
            }
        });
    }
    for i in 1..g.sheets.len() {
        cg.sheet = i;
        let sh = &g.sheets[i];
        if reserved.contains(&ident(&sh.name).as_str()) {
            cg.errors.push((i, 0, format!("a function can't be called {}: Oxidizer calls that one itself", sh.name)));
        }
        let head = format!("{}({})", ident(&sh.name), sh.params.iter().map(|p| ident(&p.name)).collect::<Vec<_>>().join(", "));
        let entry = cg.events("fn.entry");
        cg.function(&head, |cg| {
            for n in &entry {
                for p in &sh.params {
                    if cg.links().iter().any(|l| l.from == n.id && l.out == p.name) {
                        cg.locals.insert(format!("n{}_{}", n.id, p.name));
                        let v = cg.probe(format!("{}.{}", n.id, p.name), ident(&p.name));
                        cg.line(1, &format!("n{}_{} = {v};", n.id, p.name), Some(n.id));
                    }
                }
                cg.follow(n.id, "then", 1);
            }
            if cg.out.is_empty() {
                cg.line(1, "// (empty)", None); // still defined, so calls to it work
            }
        });
    }
    let mut c = Compiled { errors: cg.errors, ..Default::default() };
    for (i, (text, node)) in cg.out.into_iter().enumerate() {
        c.code.push_str(&text);
        c.code.push('\n');
        if let Some((sheet, id)) = node {
            c.lines.push((i + 1, sheet, id));
        }
    }
    c
}

// ---- the file ----

/// FNV-1a: a hash that stays the same across builds and platforms.
fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// The whole file: the code, then the graph in a comment.
pub fn to_file(g: &Graph, stem: &str) -> (String, Compiled) {
    let c = compile(g, stem);
    // '/' escaped (JSON allows "\/") so the comment can't be closed, or nested open, by text in the graph.
    let json = serde_json::to_string_pretty(g).unwrap_or_default().replace('/', "\\/");
    (format!("{}{MARK}{:016x}\n{json}\n*/\n", c.code, hash(&c.code)), c)
}

/// What a file holds.
pub enum Loaded {
    /// A plain script.
    Script,
    /// A graph; `edited` when its code no longer matches (someone changed it by hand).
    Graph { graph: Graph, edited: bool },
    /// It has a graph that can't be read.
    Broken(String),
}

pub fn from_file(text: &str) -> Loaded {
    let Some(at) = text.rfind(MARK).filter(|&i| i == 0 || text[..i].ends_with('\n')) else { return Loaded::Script };
    let (code, rest) = (&text[..at], &text[at + MARK.len()..]);
    let Some((stored, json)) = rest.split_once('\n') else { return Loaded::Broken("the graph's comment is cut short".into()) };
    let json = json.trim_end().trim_end_matches("*/");
    match serde_json::from_str::<Graph>(json) {
        Ok(graph) if !graph.sheets.is_empty() => Loaded::Graph { graph, edited: u64::from_str_radix(stored.trim(), 16).ok() != Some(hash(code)) },
        Ok(_) => Loaded::Broken("the graph has no sheets".into()),
        Err(e) => Loaded::Broken(format!("the graph can't be read: {e}")),
    }
}

/// The code without the graph (when the hand-edited code is kept).
pub fn strip(text: &str) -> &str {
    match text.rfind(MARK) {
        Some(at) => &text[..at],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{CallFnOptions, Dynamic, Engine, Map, Scope};
    use std::sync::{Arc, Mutex};

    /// An engine with stand-ins for Oxidizer's functions; what scripts print and draw goes to the log.
    fn engine(log: Arc<Mutex<Vec<String>>>) -> Engine {
        let mut e = Engine::new();
        e.set_max_expr_depths(128, 128); // as Oxidizer 0.14 does: graphs nest blocks deeply
        let l = log.clone();
        e.on_print(move |s| l.lock().unwrap().push(s.to_string()));
        let l = log.clone();
        e.register_fn("text", move |s: &str| l.lock().unwrap().push(format!("text {s}")));
        e.register_fn("begin", |_: &str| true);
        e.register_fn("end", || ());
        e.register_fn("separator", || ());
        e.register_fn("set_next_window_size", |_: f64, _: f64| ());
        e.register_fn("set_cursor_pos", |_: f64, _: f64| ());
        e.register_fn("set_next_item_width", |_: f64| ());
        e.register_fn("button", |l: &str| l.starts_with("Say hi"));
        e.register_fn("checkbox", |_: &str, v: bool| !v);
        e.register_fn("slider", |_: &str, v: f64, _: f64, max: f64| (v + 0.5).min(max));
        e.register_fn("input_text", |_: &str, s: &str| s.to_string());
        e.register_fn("input_float", |_: &str, v: f64| v);
        e.register_fn("input_int", |_: &str, v: i64| v);
        e.register_fn("combo", |_: &str, i: i64, _: rhai::Array| i);
        e.register_fn("color_edit", |_: &str, c: rhai::Array| c);
        e.register_fn("progress_bar", |_: f64, _: &str| ());
        e.register_fn("menu_item", |_: &str, _: bool| false);
        e.register_fn("key_pressed", |k: &str| k == "K");
        e.register_fn("graph_probe", |_: &str, v: Dynamic| v);
        e
    }

    fn run(code: &str, calls: &[&str]) -> (Vec<String>, Dynamic) {
        let log = Arc::new(Mutex::new(vec![]));
        let e = engine(log.clone());
        let ast = e.compile(code).unwrap_or_else(|err| panic!("{err}\n{code}"));
        let mut this: Dynamic = Map::new().into();
        for f in calls {
            let args: Vec<Dynamic> = match *f {
                "frame" => vec![1.0_f64.into(), 0.5_f64.into()],
                "gui" => vec![0.5_f64.into()],
                _ => vec![],
            };
            let opts = CallFnOptions::new().eval_ast(false).bind_this_ptr(&mut this);
            let _ = e.call_fn_with_options::<Dynamic>(opts, &mut Scope::new(), &ast, f, args).unwrap_or_else(|err| panic!("{f}: {err}\n{code}"));
        }
        let out = log.lock().unwrap().clone();
        (out, this)
    }

    fn link(s: &mut Sheet, from: u32, out: &str, to: u32, inp: &str) {
        s.links.push(Link { from, out: out.into(), to, inp: inp.into() });
    }

    #[test]
    fn starter_runs() {
        let g = Graph::starter("demo");
        let (file, c) = to_file(&g, "main");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        let (log, _) = run(&file, &["init", "gui", "menu"]);
        assert_eq!(log, ["demo: hello!", "text Hello from a graph!", "hi!"]);
    }

    #[test]
    fn file_round_trip_and_hand_edits() {
        let mut g = Graph::starter("x");
        g.sheets[0].nodes[1].data.insert("in.text".into(), "a */ b /* c".into());
        let (file, _) = to_file(&g, "main");
        let Loaded::Graph { graph, edited } = from_file(&file) else { panic!("not read back") };
        assert!(!edited);
        assert_eq!(graph, g);
        let Loaded::Graph { edited, .. } = from_file(&file.replacen("print(", "print(1); print(", 1)) else { panic!() };
        assert!(edited);
        assert!(matches!(from_file("fn init() {}\n"), Loaded::Script));
        assert!(!strip(&file).contains("rhai-studio-graph"));
    }

    #[test]
    fn flow_math_and_variables() {
        let mut g = Graph::empty();
        g.vars.push(Var { name: "total".into(), ty: "int".into(), default: "0".into() });
        let ids: Vec<u32> = (0..8).map(|_| g.new_id()).collect();
        let [ev, for_, get, add, set, br, cmp, print] = ids[..] else { unreachable!() };
        let s = &mut g.sheets[0];
        s.nodes.push(Node::new(ev, "event.init", [0.0, 0.0], &[]));
        s.nodes.push(Node::new(for_, "for", [0.0, 0.0], &[("in.first", "1"), ("in.last", "4")]));
        s.nodes.push(Node::new(get, "var.get", [0.0, 0.0], &[("name", "total")]));
        s.nodes.push(Node::new(add, "op", [0.0, 0.0], &[("op", "+"), ("ty", "int")]));
        s.nodes.push(Node::new(set, "var.set", [0.0, 0.0], &[("name", "total")]));
        s.nodes.push(Node::new(br, "branch", [0.0, 0.0], &[]));
        s.nodes.push(Node::new(cmp, "op", [0.0, 0.0], &[("op", ">"), ("ty", "int"), ("in.b", "5")]));
        s.nodes.push(Node::new(print, "print", [0.0, 0.0], &[]));
        link(s, ev, "then", for_, EXEC);
        link(s, for_, "body", set, EXEC);
        link(s, get, "value", add, "a");
        link(s, for_, "index", add, "b");
        link(s, add, "result", set, "value");
        link(s, for_, "done", br, EXEC);
        link(s, get, "value", cmp, "a");
        link(s, cmp, "result", br, "condition");
        link(s, br, "true", print, EXEC);
        link(s, get, "value", print, "text");
        let c = compile(&g, "main");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        let (log, this) = run(&c.code, &["init"]);
        assert_eq!(log, ["10"]); // 1 + 2 + 3 + 4
        assert_eq!(this.cast::<Map>()["total"].as_int().unwrap(), 10);
        assert!(c.line_of(0, br).is_some());
        assert_eq!(c.node_at(c.line_of(0, set).unwrap()), Some((0, set)));
    }

    #[test]
    fn functions_hotkeys_and_forms() {
        let mut g = Graph::empty();
        g.sheets.push(Sheet {
            name: "double".into(),
            params: vec![Param { name: "x".into(), ty: "float".into() }],
            outputs: vec![Param { name: "y".into(), ty: "float".into() }],
            ..Default::default()
        });
        let (entry, mul, ret) = (g.new_id(), g.new_id(), g.new_id());
        let s = &mut g.sheets[1];
        s.nodes.push(Node::new(entry, "fn.entry", [0.0, 0.0], &[]));
        s.nodes.push(Node::new(mul, "op", [0.0, 0.0], &[("op", "*"), ("ty", "float"), ("in.b", "2")]));
        s.nodes.push(Node::new(ret, "fn.return", [0.0, 0.0], &[]));
        link(s, entry, "then", ret, EXEC);
        link(s, entry, "x", mul, "a");
        link(s, mul, "result", ret, "y");

        let (form, slider, label) = (g.new_id(), g.new_id(), g.new_id());
        let w = |id, kind: &str, label: &str| Widget { id, kind: kind.into(), label: label.into(), pos: [8, 40], width: 100, data: BTreeMap::new() };
        g.forms.push(Form { id: form, title: "F".into(), size: [200, 100], open: true, menu: false, widgets: vec![w(slider, "slider", "S"), w(label, "text", "L")] });
        let (key, call, set_text, changed, conv) = (g.new_id(), g.new_id(), g.new_id(), g.new_id(), g.new_id());
        let (f, sl, lb) = (form.to_string(), slider.to_string(), label.to_string());
        let s = &mut g.sheets[0];
        s.nodes.push(Node::new(key, "event.hotkey", [0.0, 0.0], &[("key", "K")]));
        s.nodes.push(Node::new(call, "fn.call", [0.0, 0.0], &[("fn", "double"), ("in.x", "1.5")]));
        s.nodes.push(Node::new(set_text, "widget.set", [0.0, 0.0], &[("form", &f), ("widget", &lb)]));
        s.nodes.push(Node::new(changed, "event.widget", [0.0, 0.0], &[("form", &f), ("widget", &sl)]));
        s.nodes.push(Node::new(conv, "convert", [0.0, 0.0], &[("to", "string")]));
        link(s, key, "then", call, EXEC);
        link(s, call, "then", set_text, EXEC);
        link(s, call, "y", conv, "value");
        link(s, conv, "result", set_text, "value");
        link(s, changed, "then", call, EXEC);
        let c = compile(&g, "my file");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        // The hotkey sets the label; drawing moves the slider (the stand-in adds 0.5), which runs the chain too.
        let (log, this) = run(&c.code, &["frame", "gui"]);
        assert_eq!(log, ["text 3.0"]);
        let this = this.cast::<Map>();
        assert_eq!(this[format!("__my_file_w{slider}").as_str()].as_float().unwrap(), 0.5);
    }

    #[test]
    fn api_calls_and_errors() {
        let mut g = Graph::empty();
        let (ev, call, get, loop_a, loop_b) = (g.new_id(), g.new_id(), g.new_id(), g.new_id(), g.new_id());
        let s = &mut g.sheets[0];
        s.nodes.push(Node::new(ev, "event.frame", [0.0, 0.0], &[]));
        s.nodes.push(Node::new(call, "api", [0.0, 0.0], &[("path", "key_pressed"), ("sig", "(key: string) -> bool"), ("member", "fn"), ("in.key", "K")]));
        s.nodes.push(Node::new(get, "api", [0.0, 0.0], &[("path", "len"), ("sig", "() -> int"), ("member", "method"), ("pure", "1"), ("in.target", "[1, 2, 3]")]));
        s.nodes.push(Node::new(loop_a, "print", [0.0, 0.0], &[]));
        s.nodes.push(Node::new(loop_b, "print", [0.0, 0.0], &[]));
        link(s, ev, "then", call, EXEC);
        link(s, call, "then", loop_a, EXEC);
        link(s, get, "ret", loop_a, "text");
        link(s, loop_a, "then", loop_b, EXEC);
        link(s, loop_b, "then", loop_a, EXEC);
        let c = compile(&g, "main");
        assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
        assert_eq!(c.errors[0].1, loop_a);
        let (log, _) = run(&c.code, &["frame"]);
        assert_eq!(log, ["3", ""]);
        assert_eq!(literal("float", "3"), "3.0");
        assert_eq!(literal("float", "1e20"), "1.0e20");
        assert_eq!(literal("string", "a\"b"), "\"a\\\"b\"");
        assert!(compatible("int", "float") && !compatible("float", "int") && compatible("Vehicle", "Obj") && !compatible("Vehicle", "Part"));
        assert_eq!(parse_sig("(speed: float, paused: bool) -> ()").0.len(), 2);
    }
}
