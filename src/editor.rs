//! A code editor with syntax colors, built on the game's own ImGui text box.
//!
//! The text box does the editing (typing, cursor, selection, undo, clipboard), drawn with invisible text and a
//! transparent background. Under it, this draws the editor's background, line numbers and the colored text at the
//! same positions. The box is as tall as the text, inside a scrolling child, so every line's position is known.
//! (A multi-line box draws in a child window, which ImGui renders after its parent: what is drawn here, in the
//! parent, ends up under the box's selection highlight and cursor.)

use crate::gfx::{self, V2, v2};
use crate::highlight::{self, State};
use oxidizer_api::imgui::{self, TextBuffer, text_flags};

const BG: [f32; 4] = [0.055, 0.065, 0.08, 0.96];
const GUTTER_BG: [f32; 4] = [0.08, 0.09, 0.11, 1.0];
const LINE_NUMBER: [f32; 4] = [0.36, 0.40, 0.46, 1.0];
const CURSOR: [f32; 4] = [1.0, 0.80, 0.30, 1.0];
const SELECTION: [f32; 4] = [0.25, 0.45, 0.75, 0.45];
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const PAD: V2 = v2(8.0, 6.0);
const NO_HORIZONTAL_SCROLL: i32 = 32768;
const VAR_FRAME_BORDER_SIZE: i32 = 13;

/// Draws the editor `size` big (0 = fill, negative = fill except that much). Returns true when the text changed.
pub fn code_editor(id: &str, text: &mut TextBuffer, size: V2) -> bool {
    gfx::push_color(gfx::COL_CHILD_BG, BG);
    gfx::begin_child(&format!("{id}_scroll"), size, gfx::CHILD_BORDERS, gfx::WINDOW_HORIZONTAL_SCROLLBAR);
    gfx::pop_colors(1);

    let source = text.as_str().to_string(); // as drawn; the box below may change `text`
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
    // ponytail: the outer scroll doesn't follow the cursor when typing past the bottom edge; scroll with the wheel.
    let changed = imgui::input_text_multiline(&format!("##{id}"), text, width, height, text_flags::ALLOW_TAB_INPUT | NO_HORIZONTAL_SCROLL);
    gfx::pop_vars(2);
    gfx::pop_colors(6);
    gfx::end_child();
    changed
}
