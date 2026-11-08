//! The Rhai mods in the game's mods folder, open documents, and making a new mod.

use crate::editor::Code;
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
                self.saved = t;
                self.on_disk = modified(&self.path);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("could not read: {e}")),
        }
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

/// Makes `mods/<id>/` with a `mod.toml` and a starter `main.rhai`; returns the script's path.
/// Oxidizer starts new script mods by itself while the game runs.
pub fn new_mod(id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("Use letters, digits, '_' and '-' only.".into());
    }
    let dir = mods_dir().join(id);
    if dir.exists() {
        return Err(format!("mods/{id} already exists."));
    }
    let main = dir.join("main.rhai");
    let made = fs::create_dir_all(&dir)
        .and_then(|()| fs::write(&main, STARTER.replace("{id}", id)))
        .and_then(|()| fs::write(dir.join("mod.toml"), format!("name = \"{id}\"\n\n[oxidizer]\nscript = \"main.rhai\"\n")));
    made.map(|()| main).map_err(|e| format!("could not create mods/{id}: {e}"))
}
