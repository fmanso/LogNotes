//! A scripted run of the real window: press keys, screenshot each step, then check the database.
//!
//! Reached with `cargo run -p lognotes-qt --features driver -- --livetest`. It loads the same
//! `main.qml` the app loads and drives it through the real input path, so what it exercises is what a
//! person gets. Screenshots land in `$TMPDIR/lognotes-livetest/shots/`.

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use lognotes_core::{BlockRow, Store};

use crate::driver::{self, key, modifier};
use crate::MAIN_QML;

pub fn run() -> ! {
    let root = std::env::temp_dir().join("lognotes-livetest");
    let shots = root.join("shots");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&shots).expect("create the livetest directory");
    crate::journal::set_database_path(root.join("lognotes.db"));

    let _app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    let attached = match engine.as_mut() {
        Some(mut engine) => {
            engine.as_mut().load(&QUrl::from(MAIN_QML));
            driver::wait_ms(900);
            driver::attach(&engine)
        }
        None => false,
    };
    assert!(attached, "the QML window did not load");

    let mut failures = 0u32;

    let mut check = |label: &str, ok: bool, detail: String| {
        println!(
            "{} {label}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        );
        if !ok {
            failures += 1;
        }
    };

    let shot = |name: &str| {
        let path = shots.join(format!("{name}.png"));
        let saved = driver::save_png(&QString::from(path.to_string_lossy().as_ref()));
        println!(
            "shot {name}: {} ({})",
            if saved { "ok" } else { "FAILED" },
            path.display()
        );
    };

    shot("01-open");
    check(
        "caret is in today's block on launch",
        driver::focus_name().to_string() == "blockEditor",
        format!("focus={}", driver::focus_name().to_string()),
    );
    check(
        "the column loaded",
        driver::row_count() >= 2,
        format!("rows={}", driver::row_count()),
    );

    driver::type_text(&QString::from("hello world"));
    driver::wait_ms(700);
    shot("02-typed");
    check(
        "typing reached the store",
        blocks(&root) == ["hello world"],
        format!("blocks={:?}", blocks(&root)),
    );

    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(700);
    shot("03-enter");
    check(
        "enter split the block and left an empty one",
        blocks(&root) == ["hello world", ""],
        format!("blocks={:?}", blocks(&root)),
    );
    check(
        "the caret followed into the new block",
        driver::focus_name().to_string() == "blockEditor",
        format!("focus={}", driver::focus_name().to_string()),
    );

    driver::type_text(&QString::from("second"));
    driver::wait_ms(700);
    shot("04-second-block");
    check(
        "typing continues in the new block",
        blocks(&root) == ["hello world", "second"],
        format!("blocks={:?}", blocks(&root)),
    );

    driver::press_key(key::TAB, modifier::NONE);
    driver::wait_ms(700);
    shot("05-indent");
    check(
        "tab indented the block",
        depths(&root) == [0, 1],
        format!("depths={:?}", depths(&root)),
    );

    driver::press_key(key::BACKTAB, modifier::SHIFT);
    driver::wait_ms(700);
    shot("06-outdent");
    check(
        "shift+tab outdented it",
        depths(&root) == [0, 0],
        format!("depths={:?}", depths(&root)),
    );

    driver::press_key(key::Z, modifier::CTRL);
    driver::wait_ms(700);
    shot("07-undo");
    check(
        "ctrl+z undid the outdent",
        depths(&root) == [0, 1],
        format!("depths={:?}", depths(&root)),
    );

    println!(
        "{}",
        if failures == 0 {
            format!("all steps passed — screenshots in {}", shots.display())
        } else {
            format!("{failures} step(s) failed — screenshots in {}", shots.display())
        }
    );
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

fn rows(root: &std::path::Path) -> Vec<BlockRow> {
    let store = Store::open(root.join("lognotes.db")).expect("open the livetest database");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    store.day(&today).expect("read today").blocks
}

fn blocks(root: &std::path::Path) -> Vec<String> {
    rows(root).into_iter().map(|block| block.content).collect()
}

fn depths(root: &std::path::Path) -> Vec<u32> {
    rows(root).into_iter().map(|block| block.depth).collect()
}
