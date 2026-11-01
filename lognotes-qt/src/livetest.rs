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

    // Clicking a block puts the caret in it. Find the editor that shows the first block, click that,
    // and the text must land there — not in whichever block happened to hold the focus.
    let first_block = (0..6)
        .find(|index| driver::editor_text(*index).to_string() == "hello world")
        .expect("the first block is on screen");
    driver::click_block(first_block);
    driver::wait_ms(400);
    // A click leaves the caret where it landed, so jump to the end before appending.
    driver::press_key(key::END, modifier::NONE);
    driver::wait_ms(200);
    driver::type_text(&QString::from("!"));
    driver::wait_ms(700);
    shot("08-clicked");
    check(
        "clicking a block edits that block",
        blocks(&root) == ["hello world!", "second"],
        format!("blocks={:?}", blocks(&root)),
    );

    // Scrolling to the end pulls in the next chunk of older days. How many wheel events that takes
    // depends on how tall the rows are, so scroll until the chunk lands rather than counting.
    let rows_before = driver::row_count();
    for _ in 0..4 {
        driver::scroll_to_end();
        driver::wait_ms(400);
        if driver::row_count() > rows_before {
            break;
        }
    }
    driver::wait_ms(900);
    shot("09-scrolled");
    check(
        "scrolling loads an older chunk",
        driver::row_count() > rows_before,
        format!("rows {} -> {}", rows_before, driver::row_count()),
    );

    // ---- markers, scheduling and the agenda ------------------------------------------------
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Ctrl+Enter marks the focused block.
    driver::press_key(key::RETURN, modifier::CTRL);
    driver::wait_ms(700);
    shot("10-todo");
    check(
        "ctrl+enter marks the block TODO",
        markers(&root) == [Some(lognotes_core::Marker::Todo), None],
        format!("markers={:?}", markers(&root)),
    );

    // Ctrl+Shift+S, then Enter accepts today's date.
    let rows_before_agenda = driver::row_count();
    driver::press_key(key::S, modifier::CTRL | modifier::SHIFT);
    driver::wait_ms(400);
    shot("11-date-picker");
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(800);
    shot("12-agenda");
    check(
        "picking today scheduled the block",
        scheduled(&root) == [Some(today.clone()), None],
        format!("scheduled={:?}", scheduled(&root)),
    );
    check(
        "the agenda section appeared under today's blocks",
        driver::row_count() > rows_before_agenda,
        format!("rows {} -> {}", rows_before_agenda, driver::row_count()),
    );

    // Checking it off from the agenda marks the block DONE, and the row leaves.
    let rows_with_agenda = driver::row_count();
    check(
        "the agenda row can be clicked",
        driver::click_named(&QString::from("agendaCheck"), 0),
        "clicked the agenda checkbox".to_string(),
    );
    driver::wait_ms(800);
    shot("13-agenda-done");
    check(
        "checking it off marks the block DONE",
        markers(&root).first().copied().flatten() == Some(lognotes_core::Marker::Done),
        format!("markers={:?}", markers(&root)),
    );
    check(
        "the completed row left the agenda",
        driver::row_count() < rows_with_agenda,
        format!("rows {} -> {}", rows_with_agenda, driver::row_count()),
    );

    // ---- links, pages and references -------------------------------------------------------
    // Mentioning a page by name in prose, then linking it, must show up as two different kinds of
    // reference on that page.
    driver::click_block(0);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::NONE);
    driver::wait_ms(100);
    driver::type_text(&QString::from(" and My Project matters"));
    driver::wait_ms(700);
    driver::press_key(key::DOWN, modifier::NONE);
    driver::wait_ms(200);
    driver::press_key(key::END, modifier::NONE);
    driver::type_text(&QString::from(" see [[My Project]]"));
    driver::wait_ms(800);
    shot("14-links-typed");

    // The page switcher should now list the page the link created.
    driver::click_named(&QString::from("pagesButton"), 0);
    driver::wait_ms(400);
    shot("15-page-switcher");
    let entries = driver::click_named(&QString::from("pageEntry"), 1);
    driver::wait_ms(900);
    shot("16-page-view");
    check(
        "the page created by the link can be opened",
        entries,
        "clicked the second switcher entry".to_string(),
    );

    // Its reference panel: one linked reference, and the plain mention behind a collapsed group.
    let page_open = driver::row_count();
    check(
        "the page shows blocks and references",
        page_open > 2,
        format!("rows={page_open}"),
    );

    let expanded = driver::click_named(&QString::from("unlinkedToggle"), 0);
    driver::wait_ms(500);
    shot("17-unlinked-expanded");
    check(
        "the unlinked mentions group expands",
        expanded,
        "clicked the unlinked mentions heading".to_string(),
    );

    // A reference row is a summary: no syntax marks, and its page link is live.
    let summary = driver::named_text(&QString::from("summaryRow"), 0).to_string();
    check(
        "a summary row renders its markdown with a live page link",
        summary.contains("href=\"page:My Project\"") && !summary.contains("[["),
        format!("html={summary:?}"),
    );

    // The popup closed when we picked a page, so reopen it before choosing Journal.
    let switched_back = driver::click_named(&QString::from("pagesButton"), 0)
        && {
            driver::wait_ms(400);
            driver::click_named(&QString::from("pageEntry"), 0)
        };
    driver::wait_ms(900);
    check(
        "clicking Journal in the switcher returns to the column",
        switched_back,
        "clicked Journal".to_string(),
    );

    // ---- markdown -------------------------------------------------------------------------
    // The stored text stays markdown; what changes is how the editor draws it, so every check here
    // reads the *formats* a character ended up with, not just the picture.
    driver::click_block(0);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::NONE);
    driver::wait_ms(100);
    driver::type_text(&QString::from(" **bold** and `code`"));
    driver::wait_ms(700);
    shot("18-markdown-typed");

    // The block is plain ASCII, so a byte offset from `find` is also the UTF-16 position the editor
    // counts in.
    let typed = driver::editor_text(0).to_string();
    let bold_at = typed.find("bold").expect("the bold word is in the block") as i32;
    let code_at = typed.find("code").expect("the code word is in the block") as i32;
    let bold_format = driver::editor_format(0, bold_at).to_string();
    let code_format = driver::editor_format(0, code_at).to_string();
    check(
        "bold is drawn bold while the marks stay in the text",
        bold_format.contains("weight=700") && typed.contains("**bold**"),
        format!("format={bold_format} text={typed:?}"),
    );
    check(
        "inline code is drawn in a fixed pitch",
        code_format.contains("mono=1"),
        format!("format={code_format}"),
    );

    // ---- a block at rest renders its markdown ----------------------------------------------
    // Moving the caret to another block leaves this one showing its finished text.
    driver::click_block(1);
    driver::wait_ms(700);
    shot("19-block-at-rest");
    let shown = driver::named_text(&QString::from("blockRendered"), 0).to_string();
    check(
        "a block that is not being edited shows the finished text",
        shown.contains("font-weight:700")
            && shown.contains("monospace")
            && !shown.contains("**")
            && !shown.contains('`'),
        format!("html={shown:?}"),
    );

    // And clicking it edits it again, at the caret the click landed on, marks intact.
    let raw_before = driver::editor_text(0).to_string();
    driver::click_block(0);
    driver::wait_ms(300);
    driver::type_text(&QString::from("~"));
    driver::wait_ms(700);
    shot("20-click-to-edit");
    let raw_after = driver::editor_text(0).to_string();
    check(
        "clicking a rendered block edits it again with its marks intact",
        raw_after.chars().count() == raw_before.chars().count() + 1 && raw_after.contains("**bold**"),
        format!("{raw_before:?} -> {raw_after:?}"),
    );

    // ---- a fence that spans lines, and the diagram it draws ---------------------------------
    let blocks_before_fence = blocks(&root).len();
    driver::click_block(0);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    // Shift+Enter starts the fence on a line of its own; from the moment the ``` is open, plain Enter
    // continues it — which is what typing a code block has to feel like.
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```mermaid"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("graph TD"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("  A --> B"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```"));
    driver::wait_ms(1800);
    shot("21-diagram");

    let fenced = blocks(&root);
    check(
        "enter inside a fence kept the whole of it in one block",
        blocks_before_fence == fenced.len() && fenced[0].contains("```mermaid") && fenced[0].contains('\n'),
        format!("blocks={} fence={:?}", fenced.len(), fenced[0]),
    );
    let diagram = driver::named_text(&QString::from("diagram"), 0).to_string();
    let drawn = diagram
        .strip_prefix("file://")
        .map(|path| std::path::Path::new(path).exists())
        .unwrap_or(false);
    check(
        "the mermaid fence drew a diagram next to itself",
        drawn,
        format!("source={diagram}"),
    );

    // ---- a fence that does not parse -------------------------------------------------------
    // Appended to the second block rather than split into a third: a split needs the caret and the row
    // to agree, and this check is about the message, not about splitting.
    driver::click_block(1);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```mermaid"));
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("this is not a diagram"));
    driver::wait_ms(1800);
    shot("22-diagram-error");

    let error = driver::named_text(&QString::from("diagramError"), 0).to_string();
    check(
        "a fence that does not parse says so in one quiet line",
        error.starts_with("diagram error: ") && !error.contains('\n'),
        format!("error={error:?}"),
    );

    // Leave the caret where the close test expects it: at the end of the first block.
    driver::click_block(0);
    driver::wait_ms(300);
    // Ctrl+End, not End: a block can hold several lines now, and End stops at the end of the line.
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(200);

    check(
        "the journal is reachable again from the switcher",
        driver::row_count() > page_open,
        format!("rows={}", driver::row_count()),
    );

    // Quit straight after a keystroke: the debounce has not fired, so only the close handler can
    // save it. This is the difference between losing and keeping the last thing you typed.
    let before_flush = blocks(&root);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    driver::type_text(&QString::from(" now"));
    driver::close_window();
    driver::wait_ms(400);
    let after_flush = blocks(&root);
    check(
        "closing flushed text typed within the debounce window",
        !before_flush.is_empty()
            && after_flush.len() == before_flush.len()
            && after_flush[0] == format!("{} now", before_flush[0]),
        format!("{:?} -> {:?}", before_flush, after_flush),
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

fn markers(root: &std::path::Path) -> Vec<Option<lognotes_core::Marker>> {
    rows(root).into_iter().map(|block| block.marker).collect()
}

fn scheduled(root: &std::path::Path) -> Vec<Option<String>> {
    rows(root)
        .into_iter()
        .map(|block| block.scheduled_date)
        .collect()
}

/// The other half of the contract: a database that will not open must show the error pane, not an
/// empty journal.
pub fn run_error() -> ! {
    let root = std::env::temp_dir().join("lognotes-livetest-error");
    let shots = root.join("shots");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&shots).expect("create the error-mode directory");

    // A directory where the database file should be: SQLite cannot open it, and no amount of
    // scrolling can make it work.
    let broken = root.join("lognotes.db");
    std::fs::create_dir_all(&broken).expect("create the unopenable path");
    crate::journal::set_database_path(broken);

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

    let error = driver::journal_error().to_string();
    let ready = driver::journal_ready();
    let shot = shots.join("error.png");
    driver::save_png(&QString::from(shot.to_string_lossy().as_ref()));

    let mut failures = 0;
    let mut check = |label: &str, ok: bool, detail: String| {
        println!("{} {label}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    check("an unopenable database reports an error", !error.is_empty(), format!("error={error:?}"));
    check("the column stays hidden", !ready, format!("ready={ready}"));
    check(
        "no rows were rendered",
        driver::row_count() == 0,
        format!("rows={}", driver::row_count()),
    );
    println!(
        "{} (screenshot {})",
        if failures == 0 { "error path verified" } else { "error path FAILED" },
        shot.display()
    );
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
