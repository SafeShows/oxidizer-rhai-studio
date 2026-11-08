//! What scripts can call, for autocomplete and the API explorer: read from the definition files Oxidizer writes
//! for editors (`<documents>/oxidizer/definitions/*.d.rhai`): the game as `ksa::` and `brutal::` modules, the
//! game objects' properties and methods, Oxidizer's functions and Rhai's own.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Module,
    Function,
    /// A property of game objects: `obj.name`.
    Property,
    /// A method of game objects (or of a Rhai value): `obj.name(...)`.
    Method,
    Keyword,
}

pub struct Item {
    /// "ksa::Universe::get_elapsed_seconds", or just the name for globals and members.
    pub path: String,
    pub name: String,
    pub kind: Kind,
    /// "(speed: float, paused: bool) -> ()", ": float" for a property, "" for a module.
    pub sig: String,
    pub doc: String,
}

impl Item {
    /// How it shows in lists: `ksa::Universe::get_elapsed_seconds() -> float`, `.altitude: float`.
    pub fn label(&self) -> String {
        match self.kind {
            Kind::Property | Kind::Method => format!(".{}{}", self.name, self.sig),
            _ => format!("{}{}", self.path, self.sig),
        }
    }

    /// What INSERT puts in the script: a call with its parameter names, or the name.
    pub fn snippet(&self) -> String {
        let args = |sig: &str| {
            let inner = sig.strip_prefix('(').and_then(|s| s.split(')').next()).unwrap_or("");
            inner.split(", ").filter(|p| !p.is_empty()).map(|p| p.split(':').next().unwrap_or(p).trim()).collect::<Vec<_>>().join(", ")
        };
        match self.kind {
            Kind::Function => format!("{}({})", self.path, args(&self.sig)),
            Kind::Method => format!(".{}({})", self.name, args(&self.sig)),
            Kind::Property => format!(".{}", self.name),
            _ => self.path.clone(),
        }
    }
}

pub struct Index {
    pub items: Vec<Item>,
    /// Items by their module's path, lowercased ("" for globals).
    children: HashMap<String, Vec<usize>>,
    /// What can follow a dot: game object members and Rhai's methods, one per name.
    members: Vec<usize>,
}

const KEYWORDS: &[&str] = &[
    "let", "const", "fn", "if", "else", "while", "loop", "for", "in", "do", "until", "return", "break", "continue", "true",
    "false", "this", "try", "catch", "throw", "switch", "import", "export", "as", "private", "global",
];

static INDEX: OnceLock<Index> = OnceLock::new();

/// The index once it is built (`load` runs on a thread at startup), or None.
pub fn index() -> Option<&'static Index> {
    INDEX.get()
}

/// Builds the index from the definition files (about 100,000 lines: done once, off the game's thread).
pub fn load() {
    let dir = oxidizer_api::config_path("rhai_studio").with_file_name("definitions");
    let mut items = vec![];
    for k in KEYWORDS {
        items.push(Item { path: k.to_string(), name: k.to_string(), kind: Kind::Keyword, sig: String::new(), doc: String::new() });
    }
    // Rhai's operators are left out: nobody completes `+`.
    for file in ["__static__.d.rhai", "__builtin__.d.rhai", "ksa.d.rhai", "brutal.d.rhai"] {
        if let Ok(text) = std::fs::read_to_string(dir.join(file)) {
            parse(&text, &mut items);
        }
    }
    if items.len() == KEYWORDS.len() {
        oxidizer_api::log(&format!("rhai_studio: no definition files in {} yet: autocomplete knows keywords only", dir.display()));
    }
    let _ = INDEX.set(Index::new(items));
}

fn parse(text: &str, items: &mut Vec<Item>) {
    let mut stack: Vec<String> = vec![];
    let mut doc = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(d) = line.strip_prefix("///") {
            if !doc.is_empty() {
                doc.push('\n');
            }
            doc.push_str(d.strip_prefix(' ').unwrap_or(d));
            continue;
        }
        if let Some(rest) = line.strip_prefix("module ") {
            match (rest.strip_suffix(';'), rest.strip_suffix('{')) {
                (Some("static"), _) => stack.clear(), // globals
                (Some(root), _) => {
                    items.push(module(&[], root));
                    stack = vec![root.to_string()];
                }
                (_, Some(name)) => {
                    items.push(module(&stack, name.trim()));
                    stack.push(name.trim().to_string());
                }
                _ => {}
            }
            doc.clear();
            continue;
        }
        let closes = line.bytes().rev().take_while(|&b| b == b'}').count();
        let body = &line[..line.len() - closes];
        if let Some(f) = body.strip_prefix("fn ").or_else(|| body.strip_prefix("op ")) {
            items.extend(function(&stack, f, std::mem::take(&mut doc)));
        }
        for _ in 0..closes {
            stack.pop();
        }
        if !line.is_empty() {
            doc.clear();
        }
    }
}

fn module(stack: &[String], name: &str) -> Item {
    let path = stack.iter().map(String::as_str).chain([name]).collect::<Vec<_>>().join("::");
    Item { path, name: name.into(), kind: Kind::Module, sig: String::new(), doc: String::new() }
}

/// `get altitude(obj: Obj) -> float;`, `create(vehicle: Vehicle) -> AbandonPopup;`, `text(_: string) -> ;`
fn function(stack: &[String], f: &str, doc: String) -> Option<Item> {
    let f = f.trim_end_matches(';');
    if f.starts_with("set ") {
        return None; // the getter already names it
    }
    let (getter, f) = match f.strip_prefix("get ") {
        Some(rest) => (true, rest),
        None => (false, f),
    };
    let open = f.find('(')?;
    let close = open + f[open..].find(')')?;
    let name = f[..open].trim().to_string();
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return None; // an operator (`!`, `+`, ...)
    }
    let mut ret = f[close + 1..].trim().trim_start_matches("->").trim();
    if ret.contains("EvalAltResult") {
        ret = ""; // Oxidizer functions that can throw: Rhai writes their result type garbled
    }
    let mut params: Vec<&str> = f[open + 1..close].split(", ").filter(|p| !p.is_empty()).map(|p| p.strip_prefix("_: ").unwrap_or(p)).collect();
    let member = params.first().is_some_and(|p| p.starts_with("obj: Obj") || p.starts_with("this:"));
    if member {
        params.remove(0);
    }
    let ret = if ret.is_empty() || ret == "()" { String::new() } else { format!(" -> {ret}") };
    let (kind, sig) = match (getter, member) {
        (true, _) => (Kind::Property, ret.replacen(" ->", ":", 1)),
        (false, true) => (Kind::Method, format!("({}){ret}", params.join(", "))),
        (false, false) => (Kind::Function, format!("({}){ret}", params.join(", "))),
    };
    let path = if member || stack.is_empty() { name.clone() } else { format!("{}::{name}", stack.join("::")) };
    Some(Item { path, name, kind, sig, doc })
}

impl Index {
    fn new(items: Vec<Item>) -> Index {
        let mut children: HashMap<String, Vec<usize>> = HashMap::new();
        let mut members = vec![];
        let mut seen = HashSet::new();
        for (i, it) in items.iter().enumerate() {
            let is_member = matches!(it.kind, Kind::Property | Kind::Method)
                || (it.kind == Kind::Function && !it.path.contains("::") && it.sig.starts_with("(") && !it.sig.starts_with("()"));
            if is_member && seen.insert(it.name.clone()) {
                members.push(i); // Rhai's functions with an argument can be called as methods too: s.len()
            }
            if !matches!(it.kind, Kind::Property | Kind::Method) {
                let parent = it.path.rsplit_once("::").map(|(p, _)| p.to_lowercase()).unwrap_or_default();
                children.entry(parent).or_default().push(i);
            }
        }
        Index { items, children, members }
    }

    /// Suggestions for the code before the cursor on its line: (length of the word being completed, items).
    /// After `a::b::` the module's contents, after `.` members, otherwise globals, keywords and `locals`.
    pub fn complete(&self, before: &str, locals: &[String]) -> (usize, Vec<Suggestion>) {
        let word = before.len() - before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_').len();
        let (rest, prefix) = before.split_at(before.len() - word);
        let pool: Vec<usize> = if let Some(path) = rest.strip_suffix("::") {
            let start = path.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_' || c == ':').len();
            self.children.get(&path[start..].to_lowercase()).cloned().unwrap_or_default()
        } else if rest.ends_with('.') && !prefix.is_empty() && !rest.ends_with("..") {
            self.members.clone()
        } else if !prefix.is_empty() && !rest.ends_with('.') && !prefix.starts_with(|c: char| c.is_ascii_digit()) {
            self.children.get("").cloned().unwrap_or_default()
        } else {
            return (word, vec![]);
        };
        let p = prefix.to_lowercase();
        let mut out: Vec<Suggestion> = pool
            .into_iter()
            .map(|i| &self.items[i])
            .filter(|it| it.name.to_lowercase().starts_with(&p) && it.name != prefix)
            .map(|it| Suggestion { name: it.name.clone(), sig: it.sig.clone(), doc: first_line(&it.doc) })
            .collect();
        if !rest.ends_with("::") && !rest.ends_with('.') {
            for l in locals.iter().filter(|l| l.to_lowercase().starts_with(&p) && *l != prefix) {
                out.push(Suggestion { name: l.clone(), sig: String::new(), doc: "in this script".into() });
            }
        }
        out.sort_by(|a, b| a.name.len().cmp(&b.name.len()).then_with(|| a.name.cmp(&b.name)));
        out.dedup_by(|a, b| a.name == b.name);
        out.truncate(60);
        (word, out)
    }

    /// The explorer's results: a module's contents for "ksa::Vehicle::", paths starting with a query that has
    /// "::", otherwise names containing the query (best matches first). Empty: the top level.
    pub fn search(&self, q: &str) -> Vec<usize> {
        let q = q.trim().to_lowercase();
        if q.is_empty() || q.ends_with("::") {
            // A module's contents (or the top level): modules first, then by name.
            let mut list: Vec<usize> = self.children.get(q.trim_end_matches("::")).cloned().unwrap_or_default();
            list.retain(|&i| self.items[i].kind != Kind::Keyword);
            list.sort_by_key(|&i| (self.items[i].kind != Kind::Module, self.items[i].name.to_lowercase()));
            return list;
        }
        let mut hits: Vec<(u8, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.kind != Kind::Keyword)
            .filter_map(|(i, it)| {
                let (name, path) = (it.name.to_lowercase(), it.path.to_lowercase());
                let rank = if q.contains("::") {
                    path.starts_with(&q).then_some(1)
                } else if name == q {
                    Some(0)
                } else if name.starts_with(&q) {
                    Some(1)
                } else {
                    path.contains(&q).then_some(2)
                };
                rank.map(|r| (r, i))
            })
            .collect();
        hits.sort_by_key(|&(r, i)| (r, self.items[i].path.len()));
        hits.truncate(500);
        hits.into_iter().map(|(_, i)| i).collect()
    }
}

pub struct Suggestion {
    pub name: String,
    pub sig: String,
    pub doc: String,
}

fn first_line(doc: &str) -> String {
    let l = doc.lines().next().unwrap_or("").replace('`', "");
    if l.chars().count() > 90 { format!("{}...", l.chars().take(90).collect::<String>()) } else { l }
}

/// Names the script itself defines: `let x`, `const X`, `fn name`.
pub fn locals(text: &str) -> Vec<String> {
    let mut out = vec![];
    for kw in ["let ", "const ", "fn "] {
        for (i, _) in text.match_indices(kw) {
            if i > 0 && text[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let name: String = text[i + kw.len()..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFS: &str = "module ksa;\n\nmodule Universe {\n/// `KSA.Universe.GetElapsedSeconds`\nfn get_elapsed_seconds() -> float;\n\n/// `KSA.Universe.SetSimulationSpeed`\nfn set_simulation_speed(speed: float, paused: bool) -> ();}\n\nmodule Vehicle {\nop new() -> Vehicle;}\n";
    const STATIC: &str = "module static;\n\nfn text(_: string) -> ;\n\nfn subscribe(_: string, _: string) -> EvalAltResult>>;\n\n/// the game property or field `Altitude` on Vehicle\nfn get altitude(obj: Obj) -> float;\n\nfn set altitude(obj: Obj, value: float) -> ();\n\n/// the game method `Stage` on Vehicle\nfn stage(obj: Obj, n: int) -> bool;\n";

    fn index() -> Index {
        let mut items = vec![];
        parse(STATIC, &mut items);
        parse(DEFS, &mut items);
        Index::new(items)
    }

    #[test]
    fn reads_definitions() {
        let ix = index();
        let find = |p: &str| ix.items.iter().find(|i| i.path == p).unwrap();
        assert_eq!(find("ksa::Universe::set_simulation_speed").sig, "(speed: float, paused: bool)");
        assert_eq!(find("ksa::Universe::get_elapsed_seconds").doc, "`KSA.Universe.GetElapsedSeconds`");
        assert_eq!(find("ksa::Vehicle::new").sig, "() -> Vehicle");
        assert_eq!(find("subscribe").sig, "(string, string)");
        assert_eq!(find("altitude").sig, ": float");
        assert_eq!(find("stage").snippet(), ".stage(n)");
        assert_eq!(find("ksa::Universe::set_simulation_speed").snippet(), "ksa::Universe::set_simulation_speed(speed, paused)");
        assert_eq!(ix.items.iter().filter(|i| i.name == "altitude").count(), 1); // no setter
    }

    #[test]
    fn completes_by_context() {
        let ix = index();
        let names = |before: &str| ix.complete(before, &["my_var".into()]).1.into_iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(names("let x = ksa::Uni"), ["Universe"]);
        assert_eq!(names("ksa::Universe::"), ["get_elapsed_seconds", "set_simulation_speed"]);
        assert_eq!(names("ksa::universe::set"), ["set_simulation_speed"]);
        assert_eq!(names("v.al"), ["altitude"]);
        assert_eq!(names("    my"), ["my_var"]);
        assert_eq!(names("sub"), ["subscribe"]);
        assert!(names("v.").is_empty()); // waits for a letter after a dot
        assert!(names("x = 1").is_empty());
        assert_eq!(ix.complete("ksa::Uni", &[]).0, 3);
    }

    #[test]
    fn searches() {
        let ix = index();
        let paths = |q: &str| ix.search(q).into_iter().map(|i| ix.items[i].path.clone()).collect::<Vec<_>>();
        assert_eq!(paths("ksa::Universe::"), ["ksa::Universe::get_elapsed_seconds", "ksa::Universe::set_simulation_speed"]);
        assert_eq!(paths("altitude"), ["altitude"]);
        assert!(paths("").contains(&"ksa".to_string()));
        assert_eq!(locals("let a = 1;\nfn go(x) {}\nconst B = 2; outlet c"), ["a", "B", "go"]);
    }
}

#[cfg(test)]
mod real {
    #[test]
    #[ignore]
    fn real_files() {
        let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join("Documents/My Games/Kitten Space Agency/oxidizer/definitions");
        let t = std::time::Instant::now();
        let mut items = vec![];
        for f in ["__static__.d.rhai", "__builtin__.d.rhai", "ksa.d.rhai", "brutal.d.rhai"] {
            super::parse(&std::fs::read_to_string(dir.join(f)).unwrap(), &mut items);
        }
        let ix = super::Index::new(items);
        eprintln!("{} items in {:?}, {} members", ix.items.len(), t.elapsed(), ix.members.len());
        for q in ["ksa::Program::con", "v.get_bar", "static_", "pri", "ksa::Universe::", "brutal::im_gui_api::"] {
            let (w, s) = ix.complete(q, &[]);
            eprintln!("{q:?} word {w}: {:?}", s.iter().take(6).map(|s| format!("{}{}", s.name, s.sig)).collect::<Vec<_>>());
        }
        for q in ["controlled", "ksa::Program::"] {
            eprintln!("search {q:?}: {:?}", ix.search(q).iter().take(6).map(|&i| ix.items[i].label()).collect::<Vec<_>>());
        }
    }
}
