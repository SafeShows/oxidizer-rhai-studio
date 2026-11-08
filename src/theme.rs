//! The look of the game's own dialogs (like its multiplayer window): an opaque dark panel with a thin light border
//! and rounded corners, grey section bars over bordered boxes, near-black fields with a light outline, outlined
//! buttons and an amber one for the main action, in a darker footer.

use crate::gfx;

pub const PANEL: [f32; 4] = [0.105, 0.11, 0.12, 0.98];
pub const FOOTER: [f32; 4] = [0.075, 0.08, 0.088, 1.0];
pub const LINE: [f32; 4] = [0.30, 0.31, 0.33, 1.0];
pub const SECTION: [f32; 4] = [0.24, 0.25, 0.28, 1.0];
pub const SECTION_HOVER: [f32; 4] = [0.29, 0.30, 0.34, 1.0];
pub const FIELD: [f32; 4] = [0.04, 0.043, 0.048, 1.0];
pub const FIELD_LINE: [f32; 4] = [0.42, 0.43, 0.46, 1.0];
pub const BUTTON: [f32; 4] = [0.12, 0.125, 0.135, 1.0];
pub const BUTTON_HOVER: [f32; 4] = [0.19, 0.20, 0.22, 1.0];
pub const AMBER: [f32; 4] = [0.95, 0.71, 0.24, 1.0];
pub const AMBER_HOVER: [f32; 4] = [1.0, 0.79, 0.36, 1.0];
pub const INK: [f32; 4] = [0.07, 0.07, 0.08, 1.0];
pub const TEXT: [f32; 4] = [0.85, 0.86, 0.88, 1.0];
pub const MUTED: [f32; 4] = [0.55, 0.57, 0.60, 1.0];
pub const GREEN: [f32; 4] = [0.40, 0.82, 0.45, 1.0];
pub const RED: [f32; 4] = [0.95, 0.38, 0.33, 1.0];
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

const COLORS: &[(i32, [f32; 4])] = &[
    (0, TEXT),           // Text
    (2, PANEL),          // WindowBg
    (3, CLEAR),          // ChildBg
    (5, LINE),           // Border
    (7, FIELD),          // FrameBg
    (8, FIELD),          // FrameBgHovered
    (9, FIELD),          // FrameBgActive
    (14, CLEAR),         // ScrollbarBg
    (15, LINE),          // ScrollbarGrab
    (16, FIELD_LINE),    // ScrollbarGrabHovered
    (17, FIELD_LINE),    // ScrollbarGrabActive
    (21, BUTTON),        // Button
    (22, BUTTON_HOVER),  // ButtonHovered
    (23, BUTTON_HOVER),  // ButtonActive
    (24, SECTION),       // Header
    (25, SECTION_HOVER), // HeaderHovered
    (26, SECTION_HOVER), // HeaderActive
    (27, LINE),          // Separator
    (34, SECTION_HOVER), // TabHovered
    (35, BUTTON),        // Tab
    (36, SECTION),       // TabSelected
    (37, AMBER),         // TabSelectedOverline
    (38, BUTTON),        // TabDimmed
    (39, SECTION),       // TabDimmedSelected
];

const VARS: &[(i32, f32)] = &[(3, 6.0), (4, 1.0), (7, 3.0), (8, 1.0), (12, 3.0), (13, 1.0), (18, 10.0), (23, 3.0)];
const VARS2: &[(i32, gfx::V2)] = &[(2, gfx::v2(12.0, 10.0)), (11, gfx::v2(9.0, 5.0)), (14, gfx::v2(8.0, 6.0))];

/// Push before the window's `begin`, pop after its `end`.
pub fn push() {
    for &(idx, c) in COLORS {
        gfx::push_color(idx, c);
    }
    for &(idx, v) in VARS {
        gfx::push_var(idx, v);
    }
    for &(idx, v) in VARS2 {
        gfx::push_var2(idx, v);
    }
}

pub fn pop() {
    gfx::pop_vars((VARS.len() + VARS2.len()) as i32);
    gfx::pop_colors(COLORS.len() as i32);
}

/// The amber button for a window's main action.
pub fn primary_button(label: &str, size: gfx::V2) -> bool {
    gfx::push_color(21, AMBER);
    gfx::push_color(22, AMBER_HOVER);
    gfx::push_color(23, AMBER_HOVER);
    gfx::push_color(5, AMBER);
    gfx::push_color(0, INK);
    let clicked = gfx::button_sized(label, size);
    gfx::pop_colors(5);
    clicked
}

/// An outlined button that looks pressed (lit border and text) when `on`.
pub fn toggle_button(label: &str, on: bool, size: gfx::V2) -> bool {
    if on {
        gfx::push_color(5, AMBER);
        gfx::push_color(0, AMBER);
    }
    let clicked = gfx::button_sized(label, size);
    if on {
        gfx::pop_colors(2);
    }
    clicked
}

/// Width of a button for `label` with the theme's padding.
pub fn button_width(label: &str) -> f32 {
    gfx::text_width(label) + 18.0
}
