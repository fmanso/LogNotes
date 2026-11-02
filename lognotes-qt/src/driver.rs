//! Development-only access to the live window: press keys, look at it, count rows.
//!
//! Exists so a test can drive the real QML instead of asserting about it from the outside. Compiled
//! only under the `driver` feature — the shipped application links neither this nor QtTest.

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!(<lognotes-qt/driver_shim.h>);
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        include!("cxx-qt-lib/qstring.h");

        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;
        type QString = cxx_qt_lib::QString;

        #[rust_name = "attach"]
        fn lognotes_attach(engine: &QQmlApplicationEngine) -> bool;
        #[rust_name = "type_text"]
        fn lognotes_type_text(text: &QString) -> bool;
        #[rust_name = "press_key"]
        fn lognotes_press_key(key: i32, modifiers: i32) -> bool;
        #[rust_name = "save_png"]
        fn lognotes_save_png(path: &QString) -> bool;
        #[rust_name = "wait_ms"]
        fn lognotes_wait_ms(ms: i32);
        #[rust_name = "focus_name"]
        fn lognotes_focus_name() -> QString;
        #[rust_name = "row_count"]
        fn lognotes_row_count() -> i32;
        #[rust_name = "click_block"]
        fn lognotes_click_block(index: i32) -> bool;
        #[rust_name = "editor_text"]
        fn lognotes_editor_text(index: i32) -> QString;
        #[rust_name = "editor_debug"]
        fn lognotes_editor_debug(index: i32) -> QString;
        #[rust_name = "click_named"]
        fn lognotes_click_named(name: &QString, index: i32) -> bool;
        #[rust_name = "named_text"]
        fn lognotes_named_text(name: &QString, index: i32) -> QString;
        #[rust_name = "click_link"]
        fn lognotes_click_link(index: i32) -> bool;
        #[rust_name = "visible"]
        fn lognotes_visible(name: &QString) -> bool;
        #[rust_name = "row_html"]
        fn lognotes_row_html(index: i32) -> QString;
        #[rust_name = "search_count"]
        fn lognotes_search_count() -> i32;
        #[rust_name = "scroll_to_end"]
        fn lognotes_scroll_to_end() -> bool;
        #[rust_name = "close_window"]
        fn lognotes_close() -> bool;
        #[rust_name = "journal_property"]
        fn lognotes_journal_property(name: &QString) -> QString;
        #[rust_name = "journal_error"]
        fn lognotes_journal_error() -> QString;
        #[rust_name = "window_visible"]
        fn lognotes_window_visible() -> bool;
        #[rust_name = "journal_ready"]
        fn lognotes_journal_ready() -> bool;
    }
}

pub use ffi::*;

/// Qt's own key codes, so a scenario reads like the keymap instead of like numbers.
pub mod key {
    pub const RETURN: i32 = 0x0100_0004;
    pub const TAB: i32 = 0x0100_0001;
    pub const BACKTAB: i32 = 0x0100_0002;
    pub const BACKSPACE: i32 = 0x0100_0003;
    pub const UP: i32 = 0x0100_0013;
    pub const DOWN: i32 = 0x0100_0015;
    pub const LEFT: i32 = 0x0100_0012;
    pub const RIGHT: i32 = 0x0100_0014;
    pub const Z: i32 = 0x5A;
    // Qt::Key_Home/Key_End sit at 0x01000010/0x01000011 — 0x01000007 is Delete.
    pub const END: i32 = 0x0100_0011;
    pub const S: i32 = 0x53;
    pub const K: i32 = 0x4B;
    pub const B: i32 = 0x42;
    pub const ESCAPE: i32 = 0x0100_0000;
}

pub mod modifier {
    pub const NONE: i32 = 0x0000_0000;
    pub const CTRL: i32 = 0x0400_0000;
    pub const SHIFT: i32 = 0x0200_0000;
}
