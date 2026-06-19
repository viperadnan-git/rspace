//! A reusable single-line text input: real caret, selection, mouse, and IME via
//! `EntityInputHandler`, modeled on gpui's input example. Used wherever the app
//! needs typed text (config forms, etc.).

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size, svg, App, Bounds,
    ClickEvent, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, LayoutId,
    MouseDownEvent, MouseMoveEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, Window,
};

use crate::theme::*;

actions!(
    text_input,
    [
        Left,
        Right,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        Backspace,
        Delete,
        DeleteWordBack,
        DeleteWordForward,
        DeleteToStart,
        DeleteToEnd,
        Paste,
        Copy,
        Cut
    ]
);

/// Events a host can subscribe to. Currently just the backspace-on-empty signal
/// used by the multi-stage picker to step back a stage.
pub enum TextInputEvent {
    BackspaceOnEmpty,
}

/// Character class for word motion (mirrors Zed's `CharKind`): word chars,
/// punctuation, and whitespace are distinct, so word motion stops at word⇄
/// punctuation transitions, not only at spaces.
#[derive(PartialEq, Clone, Copy)]
enum CharKind {
    Whitespace,
    Punctuation,
    Word,
}

fn char_kind(c: char) -> CharKind {
    if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else if c.is_whitespace() {
        CharKind::Whitespace
    } else {
        CharKind::Punctuation
    }
}

pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding as K;
    let ctx = Some("TextInput");
    // Portable across OSes. `secondary-` is cmd on macOS, ctrl elsewhere.
    let mut binds = vec![
        K::new("left", Left, ctx),
        K::new("right", Right, ctx),
        K::new("shift-left", SelectLeft, ctx),
        K::new("shift-right", SelectRight, ctx),
        K::new("home", Home, ctx),
        K::new("end", End, ctx),
        K::new("shift-home", SelectToHome, ctx),
        K::new("shift-end", SelectToEnd, ctx),
        K::new("backspace", Backspace, ctx),
        K::new("delete", Delete, ctx),
        K::new("secondary-a", SelectAll, ctx),
        K::new("secondary-c", Copy, ctx),
        K::new("secondary-v", Paste, ctx),
        K::new("secondary-x", Cut, ctx),
    ];
    // Word/line motions use the platform's native modifiers: Option + Cmd on
    // macOS, Ctrl elsewhere.
    #[cfg(target_os = "macos")]
    binds.extend([
        K::new("alt-left", WordLeft, ctx),
        K::new("alt-right", WordRight, ctx),
        K::new("alt-shift-left", SelectWordLeft, ctx),
        K::new("alt-shift-right", SelectWordRight, ctx),
        K::new("alt-backspace", DeleteWordBack, ctx),
        K::new("alt-delete", DeleteWordForward, ctx),
        K::new("cmd-left", Home, ctx),
        K::new("cmd-right", End, ctx),
        K::new("cmd-shift-left", SelectToHome, ctx),
        K::new("cmd-shift-right", SelectToEnd, ctx),
        K::new("cmd-backspace", DeleteToStart, ctx),
        K::new("cmd-delete", DeleteToEnd, ctx),
    ]);
    #[cfg(not(target_os = "macos"))]
    binds.extend([
        K::new("ctrl-left", WordLeft, ctx),
        K::new("ctrl-right", WordRight, ctx),
        K::new("ctrl-shift-left", SelectWordLeft, ctx),
        K::new("ctrl-shift-right", SelectWordRight, ctx),
        K::new("ctrl-backspace", DeleteWordBack, ctx),
        K::new("ctrl-delete", DeleteWordForward, ctx),
    ]);
    cx.bind_keys(binds);
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    masked: bool,
    /// No box chrome (bg/border/padding) — for embedding inline (e.g. a list row).
    bare: bool,
    /// Validation error: red border + message below; cleared on edit.
    error: Option<SharedString>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// Horizontal scroll so the caret stays visible when the text is wider than
    /// the field (single-line scroll, like a native text input).
    scroll_offset: Pixels,
    /// Center the text within the field (for compact values like a stepper).
    centered: bool,
    /// Show an inline clear (×) button when focused and non-empty (search fields).
    clearable: bool,
}

mod element;
mod handler;
mod input;
mod render;
use element::TextElement;
