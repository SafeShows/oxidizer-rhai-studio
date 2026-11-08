//! The output pane (Oxidizer's log as it comes in) and the console line under it, which runs Rhai against the
//! live game through `oxidizer_api::rhai_eval`.

use oxidizer_api::imgui::{self, TextBuffer, text_flags};

const KEEP: usize = 500;
const GREY: [f32; 4] = [0.65, 0.65, 0.65, 1.0];
const CYAN: [f32; 4] = [0.45, 0.85, 1.0, 1.0];
const RED: [f32; 4] = [1.0, 0.45, 0.4, 1.0];
const WHITE: [f32; 4] = [0.95, 0.95, 0.95, 1.0];

pub struct Console {
    lines: Vec<([f32; 4], String)>,
    next_log: u64,
    input: TextBuffer,
}

impl Console {
    pub fn new() -> Console {
        // Start from the log's last 50 lines, so loading messages and errors are visible right away.
        let next_log = oxidizer_api::log_next().saturating_sub(50);
        let mut c = Console { lines: vec![], next_log, input: TextBuffer::default() };
        c.lines.push((GREY, "Console: type Rhai and press Enter, e.g. ksa::Program::controlled_vehicle().id".into()));
        c
    }

    fn push(&mut self, color: [f32; 4], text: &str) {
        self.lines.extend(text.lines().map(|l| (color, l.to_string())));
        let over = self.lines.len().saturating_sub(KEEP);
        self.lines.drain(..over);
    }

    /// New lines of Oxidizer's log.
    fn poll_log(&mut self) {
        let next = oxidizer_api::log_next();
        if next > self.next_log {
            for line in oxidizer_api::log_lines(self.next_log) {
                self.push(GREY, &line);
            }
            self.next_log = next;
        }
    }

    pub fn run(&mut self, code: &str) {
        self.push(CYAN, &format!("> {code}"));
        let result = oxidizer_api::rhai_eval(code);
        self.poll_log(); // print() output of the code, before its result
        match result {
            Ok(v) if v.is_empty() => {}
            Ok(v) => self.push(WHITE, &v),
            Err(e) => self.push(RED, &e),
        }
    }

    pub fn draw(&mut self, height: f32) {
        self.poll_log();
        imgui::begin_child("rhai_studio_output", height);
        for (color, line) in &self.lines {
            imgui::text_colored(*color, line);
        }
        imgui::stick_to_bottom();
        imgui::end_child();
        if imgui::input_text("##console", &mut self.input, text_flags::ENTER_RETURNS_TRUE) {
            let code = self.input.as_str().trim().to_string();
            if !code.is_empty() {
                self.run(&code);
            }
            self.input.set("");
            imgui::set_keyboard_focus_here(-1); // keep typing in the console
        }
    }
}
