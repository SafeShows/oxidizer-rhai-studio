//! A code editor with syntax colors and autocomplete, built on the game's own ImGui text box.
//!
//! The text box does the editing (typing, cursor, selection, undo, clipboard), drawn with invisible text and a
//! transparent background. Under it, this draws the editor's background, line numbers and the colored text at the
//! same positions. The box is as tall as the text, inside a scrolling child, so every line's position is known.
//! (A multi-line box draws in a child window, which ImGui renders after its parent: what is drawn here, in the
//! parent, ends up under the box's selection highlight and cursor.)
//!
//! Autocomplete: the box's callback sees the cursor every frame. After a letter is typed it asks the API index
//! (api.rs) for suggestions, shown in a tooltip under the word; Up/Down choose, Tab or Enter insert.

use crate::api::{self, Suggestion};
use crate::gfx::{self, TextEdit, V2, v2};
use crate::highlight::{self, State};
use crate::theme;
use oxidizer_api::imgui::{self, text_flags};
use std::ffi::c_void;

const BG: [f32; 4] = [0.055, 0.065, 0.08, 0.96];
const GUTTER_BG: [f32; 4] = [0.08, 0.09, 0.11, 1.0];
const LINE_NUMBER: [f32; 4] = [0.36, 0.40, 0.46, 1.0];
const CURSOR: [f32; 4] = [1.0, 0.80, 0.30, 1.0];
const SELECTION: [f32; 4] = [0.25, 0.45, 0.75, 0.45];
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const PAD: V2 = v2(8.0, 6.0);
const NO_HORIZONTAL_SCROLL: i32 = 32768;
const VAR_FRAME_BORDER_SIZE: i32 = 13;

/// A script's text, with room for ImGui to type into, and where its cursor is.
#[derive(Default)]
pub struct Code {
    /// The text, a 0, then spare room: one paste can add up to 64 KiB.
    buf: Vec<u8>,
    assist: Assist,
}

/// The cursor and the suggestions, kept up to date by the text box's callback.
#[derive(Default)]
struct Assist {
    /// The cursor (a byte offset) and the text's length, as of the last frame the box was active.
    cursor: usize,
    len: usize,
    /// Whether the cursor is known (the box was active since the text was set).
    known: bool,
    /// The box was active last frame / is this frame.
    active: bool,
    seen: bool,
    popup: Vec<Suggestion>,
    selected: usize,
    /// Length of the word before the cursor that a suggestion replaces.
    word: usize,
}

impl Code {
    const SPARE: usize = 64 * 1024;

    pub fn set(&mut self, text: &str) {
        self.buf.clear();
        self.buf.extend_from_slice(text.replace('\0', "").as_bytes());
        self.buf.push(0);
        self.assist = Assist::default();
    }

    pub fn as_str(&self) -> &str {
        let bytes = &self.buf[..self.buf.iter().position(|&b| b == 0).unwrap_or(self.buf.len())];
        std::str::from_utf8(bytes).unwrap_or_else(|e| std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap())
    }

    /// Puts `s` where the cursor was last (at the end if the script wasn't clicked into yet).
    pub fn insert(&mut self, s: &str) {
        let mut text = self.as_str().to_string();
        let mut at = if self.assist.known { self.assist.cursor.min(text.len()) } else { text.len() };
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        text.insert_str(at, s);
        self.set(&text);
        (self.assist.cursor, self.assist.known) = (at + s.len(), true);
    }
}

extern "C" fn on_frame(e: *mut TextEdit) -> i32 {
    let e = unsafe { &mut *e };
    let a = unsafe { &mut *(e.user_data as *mut Assist) };
    a.seen = true;
    let (len, cur) = (e.buf_text_len.max(0) as usize, e.cursor_pos.max(0) as usize);
    if !a.active {
        (a.len, a.cursor, a.known) = (len, cur, true); // just clicked into: nothing typed yet
        return 0;
    }
    let open = !a.popup.is_empty();
    let (up, down) = (gfx::key_pressed(gfx::KEY_UP), gfx::key_pressed(gfx::KEY_DOWN));
    if open && len == a.len && (up || down) {
        let n = a.popup.len();
        a.selected = if up { (a.selected + n - 1) % n } else { (a.selected + 1) % n };
        e.set_cursor(a.cursor); // the box moved the cursor a line: put it back
        return 0;
    }
    let typed = len == a.len + 1 && cur == a.cursor + 1;
    let last = if typed { e.text()[cur - 1] } else { 0 };
    if open && ((last == b'\t' && gfx::key_pressed(gfx::KEY_TAB)) || (last == b'\n' && gfx::key_pressed(gfx::KEY_ENTER))) {
        // The box already typed the Tab or newline: replace it and the word with the suggestion.
        let name = a.popup[a.selected].name.clone();
        let start = a.cursor - a.word.min(a.cursor);
        e.replace(start, cur - start, &name);
        a.popup.clear();
        (a.len, a.cursor) = (e.buf_text_len.max(0) as usize, e.cursor_pos.max(0) as usize);
        return 0;
    }
    let erased = len + 1 == a.len && cur + 1 == a.cursor;
    if typed || (open && erased) {
        a.suggest(e.text(), cur);
    } else if len != a.len || cur != a.cursor {
        a.popup.clear();
    }
    (a.len, a.cursor) = (len, cur);
    0
}

impl Assist {
    fn suggest(&mut self, text: &[u8], cur: usize) {
        self.popup.clear();
        let (Ok(text), Some(ix)) = (std::str::from_utf8(text), api::index()) else { return };
        if !text.is_char_boundary(cur) {
            return;
        }
        let before = &text[text[..cur].rfind('\n').map_or(0, |i| i + 1)..cur];
        if in_string_or_comment(before) {
            return;
        }
        (self.word, self.popup) = ix.complete(before, &api::locals(text));
        self.selected = 0;
    }
}

/// Whether the end of `line` is inside a string or a `//` comment.
fn in_string_or_comment(line: &str) -> bool {
    let (mut quote, mut prev) = (None, ' ');
    for c in line.chars() {
        match quote {
            Some(q) if c == q && prev != '\\' => quote = None,
            Some(_) => {}
            None if c == '/' && prev == '/' => return true,
            None if matches!(c, '"' | '`' | '\'') => quote = Some(c),
            None => {}
        }
        prev = c;
    }
    quote.is_some()
}

/// The suggestions, under the word being typed.
fn draw_popup(a: &Assist, pos: V2) {
    const ROWS: usize = 8;
    if !gfx::begin_tooltip_at(pos) {
        return;
    }
    let first = a.selected.saturating_sub(ROWS - 1).min(a.popup.len().saturating_sub(ROWS));
    for (i, s) in a.popup.iter().enumerate().skip(first).take(ROWS) {
        imgui::text_colored(if i == a.selected { theme::AMBER } else { theme::TEXT }, &s.name);
        if !s.sig.is_empty() {
            imgui::same_line();
            imgui::text_colored(theme::MUTED, &s.sig);
        }
    }
    if a.popup.len() > ROWS {
        imgui::text_colored(theme::MUTED, &format!("{} of {}", a.selected + 1, a.popup.len()));
    }
    let doc = &a.popup[a.selected].doc;
    if !doc.is_empty() {
        imgui::separator();
        imgui::text_colored(theme::MUTED, doc);
    }
    gfx::end_tooltip();
}

/// Draws the editor `size` big (0 = fill, negative = fill except that much). Returns true when the text changed.
pub fn code_editor(id: &str, code: &mut Code, size: V2) -> bool {
    gfx::push_color(gfx::COL_CHILD_BG, BG);
    gfx::begin_child(&format!("{id}_scroll"), size, gfx::CHILD_BORDERS, gfx::WINDOW_HORIZONTAL_SCROLLBAR);
    gfx::pop_colors(1);

    let source = code.as_str().to_string(); // as drawn; the box below may change the text
    let lines: Vec<&str> = source.split('\n').collect();
    let lh = gfx::line_height();
    let gutter = gfx::text_width(&lines.len().to_string()) + 18.0;
    let widest = lines.iter().map(|l| gfx::text_width(l)).fold(0.0, f32::max);
    let room = gfx::avail();
    let width = (widest + PAD.x * 2.0 + 40.0).max(room.x - gutter);
    let height = ((lines.len() + 1) as f32 * lh + PAD.y * 2.0).max(room.y);
    let origin = gfx::cursor_pos();

    // Gutter and line numbers, then colored text, for the visible lines only.
    let scroll = gfx::scroll_y();
    let first = ((scroll - PAD.y) / lh).floor().max(0.0) as usize;
    let last = (((scroll + gfx::window_height()) / lh).ceil() as usize + 1).min(lines.len());
    gfx::draw_rect(origin, v2(origin.x + gutter - 6.0, origin.y + height), GUTTER_BG, 0.0);
    let mut state = State::Code;
    for (i, line) in lines.iter().enumerate().take(last) {
        let (spans, next) = highlight::spans(line, state);
        state = next; // the state carries through lines above the visible ones
        if i < first {
            continue;
        }
        let y = origin.y + PAD.y + i as f32 * lh;
        let n = (i + 1).to_string();
        gfx::draw_text(v2(origin.x + gutter - 12.0 - gfx::text_width(&n), y), LINE_NUMBER, &n);
        let x = origin.x + gutter + PAD.x;
        for (range, color) in spans {
            let piece = &line[range.clone()];
            if !piece.trim().is_empty() {
                // Measured from the line start: ImGui rounds each measurement up to a pixel, so adding up the
                // widths of the pieces would drift right of where the text box puts the characters.
                gfx::draw_text(v2(x + gfx::text_width(&line[..range.start]), y), color, piece);
            }
        }
    }

    // The text box on top: it edits, and draws only the cursor and the selection.
    gfx::set_cursor_pos(v2(origin.x + gutter, origin.y));
    for (idx, color) in [
        (gfx::COL_TEXT, CLEAR),
        (gfx::COL_FRAME_BG, CLEAR),
        (gfx::COL_FRAME_BG_HOVERED, CLEAR),
        (gfx::COL_FRAME_BG_ACTIVE, CLEAR),
        (gfx::COL_INPUT_TEXT_CURSOR, CURSOR),
        (gfx::COL_TEXT_SELECTED_BG, SELECTION),
    ] {
        gfx::push_color(idx, color);
    }
    gfx::push_var2(gfx::VAR_FRAME_PADDING, PAD);
    gfx::push_var(VAR_FRAME_BORDER_SIZE, 0.0); // no outline: the editor is the frame
    let want = source.len() + Code::SPARE;
    if code.buf.len() < want {
        code.buf.resize(want, 0);
    }
    let a = &mut code.assist;
    (a.active, a.seen) = (a.seen, false);
    // ponytail: the outer scroll doesn't follow the cursor when typing past the bottom edge; scroll with the wheel.
    let flags = text_flags::ALLOW_TAB_INPUT | NO_HORIZONTAL_SCROLL | gfx::INPUT_CALLBACK_ALWAYS;
    let changed = gfx::input_text_multiline(&format!("##{id}"), &mut code.buf, v2(width, height), flags, on_frame, a as *mut Assist as *mut c_void);
    gfx::pop_vars(2);
    gfx::pop_colors(6);

    if !code.assist.seen {
        code.assist.popup.clear(); // the box isn't active
    }
    if !code.assist.popup.is_empty() {
        let text = code.as_str();
        let start = code.assist.cursor.saturating_sub(code.assist.word).min(text.len());
        if text.is_char_boundary(start) {
            let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let line = text[..start].matches('\n').count();
            let x = origin.x + gutter + PAD.x + gfx::text_width(&text[line_start..start]);
            draw_popup(&code.assist, v2(x - 8.0, origin.y + PAD.y + (line + 1) as f32 * lh + 2.0));
        }
    }
    gfx::end_child();
    changed
}

#[cfg(test)]
mod tests {
    #[test]
    fn strings_and_comments() {
        use super::in_string_or_comment as f;
        assert!(f(r#"print("ksa::Uni"#));
        assert!(f("let x = 1; // ksa"));
        assert!(!f(r#"print("a"); ksa::Uni"#));
        assert!(!f(r#"let s = "a\"b"; x"#));
    }
}
