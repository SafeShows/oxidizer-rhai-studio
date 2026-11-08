//! The parts of the game's ImGui that `oxidizer_api::imgui` doesn't wrap (yet): drawing, measuring text, styles,
//! child windows with a width, selectable rows. Called through the game's own C exports, like the API does.

use std::ffi::{CString, c_char, c_void};
use std::sync::OnceLock;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

pub const fn v2(x: f32, y: f32) -> V2 {
    V2 { x, y }
}

#[repr(C)]
struct V4([f32; 4]);

type S = *const c_char;
type DrawList = *mut c_void;

struct Fns {
    get_window_draw_list: unsafe extern "C" fn() -> DrawList,
    add_text: unsafe extern "C" fn(DrawList, *const V2, u32, S, S),
    add_rect_filled: unsafe extern "C" fn(DrawList, *const V2, *const V2, u32, f32, i32),
    cursor_screen_pos: unsafe extern "C" fn() -> V2,
    set_cursor_screen_pos: unsafe extern "C" fn(*const V2),
    calc_text_size: unsafe extern "C" fn(S, S, bool, f32) -> V2,
    text_line_height: unsafe extern "C" fn() -> f32,
    content_region_avail: unsafe extern "C" fn() -> V2,
    window_size: unsafe extern "C" fn() -> V2,
    scroll_y: unsafe extern "C" fn() -> f32,
    push_style_var_f: unsafe extern "C" fn(i32, f32),
    push_style_var_v: unsafe extern "C" fn(i32, *const V2),
    pop_style_var: unsafe extern "C" fn(i32),
    push_style_color: unsafe extern "C" fn(i32, *const V4),
    pop_style_color: unsafe extern "C" fn(i32),
    begin_child: unsafe extern "C" fn(S, *const V2, i32, i32) -> bool,
    end_child: unsafe extern "C" fn(),
    selectable: unsafe extern "C" fn(S, bool, i32, *const V2) -> bool,
    begin: unsafe extern "C" fn(S, *mut bool, i32) -> bool,
    collapsing_header: unsafe extern "C" fn(S, i32) -> bool,
    set_next_item_width: unsafe extern "C" fn(f32),
    window_pos: unsafe extern "C" fn() -> V2,
    button: unsafe extern "C" fn(S, *const V2) -> bool,
    add_line: unsafe extern "C" fn(DrawList, *const V2, *const V2, u32, f32),
}

fn fns() -> &'static Fns {
    static F: OnceLock<Fns> = OnceLock::new();
    F.get_or_init(|| unsafe {
        let lib = libloading::Library::new(libloading::library_filename("imgui")).expect("the game's ImGui library is not loaded");
        macro_rules! sym {
            ($n:literal) => {
                *lib.get(concat!($n, "\0").as_bytes()).expect($n)
            };
        }
        let f = Fns {
            get_window_draw_list: sym!("GetWindowDrawList"),
            add_text: sym!("ImDrawList_AddText_0"),
            add_rect_filled: sym!("ImDrawList_AddRectFilled"),
            cursor_screen_pos: sym!("GetCursorScreenPos"),
            set_cursor_screen_pos: sym!("SetCursorScreenPos"),
            calc_text_size: sym!("CalcTextSize"),
            text_line_height: sym!("GetTextLineHeight"),
            content_region_avail: sym!("GetContentRegionAvail"),
            window_size: sym!("GetWindowSize"),
            scroll_y: sym!("GetScrollY"),
            push_style_var_f: sym!("PushStyleVar_0"),
            push_style_var_v: sym!("PushStyleVar_1"),
            pop_style_var: sym!("PopStyleVar"),
            push_style_color: sym!("PushStyleColor_1"),
            pop_style_color: sym!("PopStyleColor"),
            begin_child: sym!("BeginChild_0"),
            end_child: sym!("EndChild"),
            selectable: sym!("Selectable_0"),
            begin: sym!("Begin"),
            collapsing_header: sym!("CollapsingHeader_0"),
            set_next_item_width: sym!("SetNextItemWidth"),
            window_pos: sym!("GetWindowPos"),
            button: sym!("Button"),
            add_line: sym!("ImDrawList_AddLine"),
        };
        std::mem::forget(lib);
        f
    })
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

/// An RGBA color as ImGui packs it for draw lists.
pub fn rgba(c: [f32; 4]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    b(c[0]) | b(c[1]) << 8 | b(c[2]) << 16 | b(c[3]) << 24
}

/// Draws text at a screen position into the current window, without moving the layout cursor.
pub fn draw_text(pos: V2, color: [f32; 4], s: &str) {
    let p = s.as_ptr() as S;
    unsafe { (fns().add_text)((fns().get_window_draw_list)(), &pos, rgba(color), p, p.add(s.len())) }
}

pub fn draw_rect(min: V2, max: V2, color: [f32; 4], rounding: f32) {
    unsafe { (fns().add_rect_filled)((fns().get_window_draw_list)(), &min, &max, rgba(color), rounding, 0) }
}

pub fn text_width(s: &str) -> f32 {
    let p = s.as_ptr() as S;
    unsafe { (fns().calc_text_size)(p, p.add(s.len()), false, -1.0).x }
}

pub fn line_height() -> f32 {
    unsafe { (fns().text_line_height)() }
}

pub fn cursor_pos() -> V2 {
    unsafe { (fns().cursor_screen_pos)() }
}

/// Moves the layout cursor: the next widget goes here.
pub fn set_cursor_pos(p: V2) {
    unsafe { (fns().set_cursor_screen_pos)(&p) }
}

pub fn avail() -> V2 {
    unsafe { (fns().content_region_avail)() }
}

pub fn window_height() -> f32 {
    unsafe { (fns().window_size)().y }
}

pub fn scroll_y() -> f32 {
    unsafe { (fns().scroll_y)() }
}

// ImGuiStyleVar and ImGuiCol values of the game's ImGui.
pub const VAR_FRAME_PADDING: i32 = 11;
pub const COL_TEXT: i32 = 0;
pub const COL_CHILD_BG: i32 = 3;
pub const COL_FRAME_BG: i32 = 7;
pub const COL_FRAME_BG_HOVERED: i32 = 8;
pub const COL_FRAME_BG_ACTIVE: i32 = 9;
pub const COL_INPUT_TEXT_CURSOR: i32 = 33;
pub const COL_TEXT_SELECTED_BG: i32 = 53;

pub fn push_var(idx: i32, v: f32) {
    unsafe { (fns().push_style_var_f)(idx, v) }
}

pub fn push_var2(idx: i32, v: V2) {
    unsafe { (fns().push_style_var_v)(idx, &v) }
}

pub fn pop_vars(n: i32) {
    unsafe { (fns().pop_style_var)(n) }
}

pub fn push_color(idx: i32, c: [f32; 4]) {
    unsafe { (fns().push_style_color)(idx, &V4(c)) }
}

pub fn pop_colors(n: i32) {
    unsafe { (fns().pop_style_color)(n) }
}

pub const CHILD_BORDERS: i32 = 1;
pub const WINDOW_HORIZONTAL_SCROLLBAR: i32 = 2048;

/// A child window `size` big (0 = fill, negative = fill except that much). Call [`end_child`] regardless.
pub fn begin_child(id: &str, size: V2, child_flags: i32, window_flags: i32) -> bool {
    unsafe { (fns().begin_child)(c(id).as_ptr(), &size, child_flags, window_flags) }
}

pub fn end_child() {
    unsafe { (fns().end_child)() }
}

/// A row that highlights when hovered or `selected`; true when clicked.
pub fn selectable(label: &str, selected: bool) -> bool {
    unsafe { (fns().selectable)(c(label).as_ptr(), selected, 0, &V2::default()) }
}


pub const WINDOW_NO_TITLE_BAR: i32 = 1;
pub const WINDOW_NO_COLLAPSE: i32 = 32;
pub const WINDOW_NO_SCROLLBAR: i32 = 8;
pub const CHILD_AUTO_RESIZE_Y: i32 = 32;
pub const TREE_DEFAULT_OPEN: i32 = 32;

/// A window with flags. Call `imgui::end` regardless.
pub fn begin_window(title: &str, flags: i32) -> bool {
    unsafe { (fns().begin)(c(title).as_ptr(), std::ptr::null_mut(), flags) }
}

/// A section header bar; true while open.
pub fn header(label: &str, default_open: bool) -> bool {
    unsafe { (fns().collapsing_header)(c(label).as_ptr(), if default_open { TREE_DEFAULT_OPEN } else { 0 }) }
}

/// Width of the next widget: negative fills the row except that much (`-f32::MIN_POSITIVE` fills it all).
pub fn next_width(w: f32) {
    unsafe { (fns().set_next_item_width)(w) }
}

pub fn window_pos() -> V2 {
    unsafe { (fns().window_pos)() }
}

pub fn window_size() -> V2 {
    unsafe { (fns().window_size)() }
}

/// A button of a given size (0 = fit the label).
pub fn button_sized(label: &str, size: V2) -> bool {
    unsafe { (fns().button)(c(label).as_ptr(), &size) }
}

pub fn draw_line(a: V2, b: V2, color: [f32; 4]) {
    unsafe { (fns().add_line)((fns().get_window_draw_list)(), &a, &b, rgba(color), 1.0) }
}
