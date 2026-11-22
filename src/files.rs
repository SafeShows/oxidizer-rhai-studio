//! The Rhai mods in the game's mods folder, open documents, and making a new mod.

use crate::blueprint;
use crate::editor::Code;
use crate::graph::Graph;
use oxidizer_api::mods_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A Rhai mod: a folder in the mods folder whose `mod.toml` names a script in its `[oxidizer]` section.
pub struct RhaiMod {
    pub id: String,
    /// Its `.rhai` files, the main script first.
    pub files: Vec<PathBuf>,
}

pub fn rhai_mods() -> Vec<RhaiMod> {
    let Ok(dirs) = fs::read_dir(mods_dir()) else { return vec![] };
    let mut mods: Vec<RhaiMod> = dirs
        .flatten()
        .map(|e| e.path())
        .filter_map(|dir| {
            let cfg = fs::read_to_string(dir.join("mod.toml")).ok()?.parse::<toml::Table>().ok()?;
            let main = dir.join(cfg.get("oxidizer")?.get("script")?.as_str()?);
            let mut files = vec![main.clone()];
            let mut others: Vec<PathBuf> = fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "rhai") && *p != main)
                .collect();
            others.sort();
            files.extend(others);
            Some(RhaiMod { id: dir.file_name()?.to_string_lossy().into_owned(), files })
        })
        .collect();
    mods.sort_by_key(|a| a.id.to_lowercase());
    mods
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A file open in a tab.
pub struct Doc {
    pub path: PathBuf,
    /// The mod it belongs to (its folder name).
    pub mod_id: String,
    /// "mod/file.rhai", unique per file (tab labels need that).
    pub title: String,
    pub text: Code,
    /// The text as it is on disk.
    saved: String,
    on_disk: Option<SystemTime>,
    pub error: Option<String>,
    /// The graph editor, when the file is a graph script.
    pub graph: Option<Box<blueprint::Editor>>,
}

impl Doc {
    pub fn open(path: &Path, mod_id: &str) -> Doc {
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let mut d = Doc {
            path: path.to_path_buf(),
            mod_id: mod_id.into(),
            title: format!("{mod_id}/{file}"),
            text: Code::default(),
            saved: String::new(),
            on_disk: None,
            error: None,
            graph: None,
        };
        d.revert();
        d
    }

    pub fn dirty(&self) -> bool {
        self.text.as_str() != self.saved
    }

    /// Writes the file; Oxidizer notices the change and reloads the script within a second.
    pub fn save(&mut self) {
        let text = self.text.as_str().to_string();
        match fs::write(&self.path, &text) {
            Ok(()) => {
                self.saved = text;
                self.on_disk = modified(&self.path);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("could not save: {e}")),
        }
    }

    /// Back to the file as it is on disk.
    pub fn revert(&mut self) {
        match fs::read_to_string(&self.path) {
            Ok(t) => {
                self.text.set(&t);
                self.on_disk = modified(&self.path);
                (self.graph, self.error) = match blueprint::Editor::load(&t, &self.stem()) {
                    Ok(g) => (g.map(Box::new), None),
                    Err(e) => (None, Some(format!("{e}: showing it as code"))),
                };
                self.saved = t;
            }
            Err(e) => self.error = Some(format!("could not read: {e}")),
        }
    }

    /// The file's name without `.rhai`.
    pub fn stem(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Turns a script into a graph: its code goes into a Script Code node (save to keep it).
    pub fn convert_to_graph(&mut self) {
        let mut g = Graph::empty();
        let id = g.new_id();
        g.sheets[0].nodes.push(crate::graph::Node::new(id, "script", [40.0, 40.0], &[("code", self.text.as_str())]));
        let ed = blueprint::Editor::new(g, &self.stem());
        self.text.set(&ed.file());
        self.graph = Some(Box::new(ed));
    }

    /// Keeps the hand-edited code of a graph script: it becomes a plain script (save to keep it).
    pub fn drop_graph(&mut self) {
        let code = crate::graph::strip(self.text.as_str()).to_string();
        self.text.set(&code);
        self.graph = None;
    }

    /// Picks up a change made outside the game (in another editor), unless this tab has unsaved edits.
    pub fn follow_disk(&mut self) {
        if !self.dirty() && modified(&self.path) != self.on_disk {
            self.revert();
        }
    }
}

const STARTER: &str = r#"// {id}: a Rhai mod made in Rhai Studio. Save (Ctrl+S) and it reloads within a second.
// API: https://github.com/SafeShows/Oxidizer#rhai

fn init() {
    print("{id}: hello!");
}

// Your windows (ImGui). Always call end() after begin().
fn gui(dt) {
    if begin("{id}") {
        let v = ksa::Program::controlled_vehicle();
        if v == () {
            text("no vehicle");
        } else {
            text(`vehicle: ${v.id}, altitude ${v.get_barometric_altitude().to_int()} m`);
        }
    }
    end();
}
"#;

/// Makes `mods/<id>/` with a `mod.toml` and a starter `main.rhai` (a script, or a graph); returns the script's
/// path. Oxidizer starts new script mods by itself while the game runs.
pub fn new_mod(id: &str, graph: bool) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("Use letters, digits, '_' and '-' only.".into());
    }
    let dir = mods_dir().join(id);
    if dir.exists() {
        return Err(format!("mods/{id} already exists."));
    }
    let main = dir.join("main.rhai");
    let made = fs::create_dir_all(&dir)
        .and_then(|()| fs::write(&main, if graph { blueprint::Editor::new(Graph::starter(id), "main").file() } else { STARTER.replace("{id}", id) }))
        .and_then(|()| fs::write(dir.join("mod.toml"), format!("name = \"{id}\"\n\n[oxidizer]\nscript = \"main.rhai\"\n")));
    made.map(|()| main).map_err(|e| format!("could not create mods/{id}: {e}"))
}

/// Adds a graph script to a mod (`graph.rhai`, `graph2.rhai`, ...), listed in its mod.toml's `scripts` so
/// Oxidizer (0.14+) runs it with the main script. Returns its path.
pub fn new_graph(mod_id: &str) -> Result<PathBuf, String> {
    let dir = mods_dir().join(mod_id);
    let name = (1..).map(|n| if n == 1 { "graph.rhai".to_string() } else { format!("graph{n}.rhai") }).find(|n| !dir.join(n).exists()).unwrap();
    let path = dir.join(&name);
    let toml_path = dir.join("mod.toml");
    let text = fs::read_to_string(&toml_path).map_err(|e| format!("could not read mods/{mod_id}/mod.toml: {e}"))?;
    let updated = add_script(&text, &name).ok_or_else(|| format!("mods/{mod_id}/mod.toml has no [oxidizer] section"))?;
    let stem = name.trim_end_matches(".rhai");
    fs::write(&path, blueprint::Editor::new(Graph::empty(), stem).file()).map_err(|e| format!("could not create {name}: {e}"))?;
    fs::write(&toml_path, updated).map_err(|e| format!("could not update mod.toml: {e}"))?;
    Ok(path)
}

/// mod.toml with `file` added to `[oxidizer] scripts`, changing as little of the text as it can.
fn add_script(text: &str, file: &str) -> Option<String> {
    let mut table: toml::Table = text.parse().ok()?;
    let ox = table.get_mut("oxidizer")?.as_table_mut()?;
    let mut scripts: Vec<String> = ox.get("scripts").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    scripts.push(file.into());
    let line = format!("scripts = [{}]", scripts.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", "));
    let lines: Vec<&str> = text.lines().collect();
    let key = |l: &&str, k: &str| l.trim_start().strip_prefix(k).is_some_and(|r| r.trim_start().starts_with('='));
    if let Some(i) = lines.iter().position(|l| key(l, "scripts")) {
        if lines[i].contains(']') {
            let mut out = lines.clone();
            out[i] = &line;
            return Some(out.join("\n") + "\n");
        }
        ox.insert("scripts".into(), toml::Value::Array(scripts.into_iter().map(toml::Value::from).collect()));
        return Some(table.to_string()); // a list over several lines: written anew
    }
    let at = lines.iter().position(|l| key(l, "script")).or_else(|| lines.iter().position(|l| l.trim() == "[oxidizer]"))?;
    let mut out = lines.clone();
    out.insert(at + 1, &line);
    Some(out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds_scripts_to_mod_toml() {
        let t = "name = \"m\"\n\n[oxidizer]\nscript = \"main.rhai\" # main\n";
        assert_eq!(super::add_script(t, "graph.rhai").unwrap(), "name = \"m\"\n\n[oxidizer]\nscript = \"main.rhai\" # main\nscripts = [\"graph.rhai\"]\n");
        let t2 = super::add_script(t, "graph.rhai").unwrap();
        assert!(super::add_script(&t2, "graph2.rhai").unwrap().contains("scripts = [\"graph.rhai\", \"graph2.rhai\"]"));
        assert!(super::add_script("name = 1\n", "g.rhai").is_none());
    }
}
