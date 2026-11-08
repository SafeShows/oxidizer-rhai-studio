//! The API explorer: search everything a script can call, open a module to see what is in it, read a function's
//! signature and docs, and insert it into the script.

use crate::api::{self, Kind};
use crate::gfx::{self, v2};
use crate::theme::{self, MUTED};
use oxidizer_api::imgui::{self, TextBuffer};

const DETAILS_HEIGHT: f32 = 170.0;

#[derive(Default)]
pub struct Explorer {
    query: TextBuffer,
    /// The query the results are for.
    searched: Option<String>,
    results: Vec<usize>,
    selected: Option<usize>,
}

impl Explorer {
    /// Draws the explorer into the current child. Returns the code to insert when INSERT was pressed.
    pub fn draw(&mut self) -> Option<String> {
        let Some(ix) = api::index() else {
            imgui::text_colored(MUTED, "Reading the API...");
            return None;
        };
        gfx::next_width(-f32::MIN_POSITIVE);
        imgui::input_text("##rhai_studio_api_search", &mut self.query, 0);
        let q = self.query.as_str().trim().to_string();
        if self.searched.as_deref() != Some(q.as_str()) {
            self.results = ix.search(&q);
            self.selected = None;
            self.searched = Some(q.clone());
        }
        if let Some(module) = q.strip_suffix("::") {
            if gfx::button_sized("BACK", v2(0.0, 0.0)) {
                self.query.set(&module.rsplit_once("::").map(|(up, _)| format!("{up}::")).unwrap_or_default());
            }
            imgui::same_line();
            imgui::text(module);
        } else if q.is_empty() {
            imgui::text_colored(MUTED, "Search, or open a module below");
        } else {
            imgui::text_colored(MUTED, &format!("{} found", self.results.len()));
        }

        let mut open = None;
        gfx::begin_child("rhai_studio_api_list", v2(0.0, -DETAILS_HEIGHT), gfx::CHILD_BORDERS, 0);
        for &i in &self.results {
            let it = &ix.items[i];
            let label = if it.kind == Kind::Module { format!("{}::", it.path) } else { it.label() };
            if gfx::selectable(&format!("{label}##api{i}"), self.selected == Some(i)) {
                self.selected = Some(i);
                if it.kind == Kind::Module {
                    open = Some(format!("{}::", it.path));
                }
            }
        }
        gfx::end_child();
        if let Some(path) = open {
            self.query.set(&path);
        }

        let mut insert = None;
        gfx::begin_child("rhai_studio_api_details", v2(0.0, 0.0), gfx::CHILD_BORDERS, 0);
        match self.selected.map(|i| &ix.items[i]) {
            Some(it) if it.kind != Kind::Module => {
                imgui::text_colored(theme::AMBER, &it.label());
                if theme::primary_button("INSERT", v2(theme::button_width("INSERT") + 16.0, 0.0)) {
                    insert = Some(it.snippet());
                }
                gfx::text_wrapped(&it.doc.replace('`', ""));
            }
            _ => imgui::text_colored(MUTED, "Pick a function or property to see its details."),
        }
        gfx::end_child();
        insert
    }
}
