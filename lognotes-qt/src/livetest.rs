//! A scripted run of the real window: press keys, screenshot each step, then check the database.
//!
//! Reached with `cargo run -p lognotes-qt --features driver -- --livetest`. It loads the same
//! `main.qml` the app loads and drives it through the real input path, so what it exercises is what a
//! person gets. Screenshots land in `$TMPDIR/lognotes-livetest/shots/`.

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use lognotes_core::{BlockRow, Marker, Store};

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
            // Not `settle()`: it reads the model through the attached root, and there is none yet.
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
    // A window nobody can see still screenshots, so its visibility is asserted rather than assumed.
    check(
        "the window is on screen",
        driver::window_visible(),
        "window()->isVisible()".to_string(),
    );
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
    driver::settle();
    shot("02-typed");
    check(
        "typing reached the store",
        blocks(&root) == ["hello world"],
        format!("blocks={:?}", blocks(&root)),
    );

    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
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
    driver::settle();
    shot("04-second-block");
    check(
        "typing continues in the new block",
        blocks(&root) == ["hello world", "second"],
        format!("blocks={:?}", blocks(&root)),
    );

    driver::press_key(key::TAB, modifier::NONE);
    driver::settle();
    shot("05-indent");
    check(
        "tab indented the block",
        depths(&root) == [0, 1],
        format!("depths={:?}", depths(&root)),
    );

    driver::press_key(key::BACKTAB, modifier::SHIFT);
    driver::settle();
    shot("06-outdent");
    check(
        "shift+tab outdented it",
        depths(&root) == [0, 0],
        format!(
            "depths={:?} focus={} rebuilding={}",
            depths(&root),
            driver::focus_name().to_string(),
            driver::journal_property(&QString::from("ready")).to_string()
        ),
    );

    driver::press_key(key::Z, modifier::CTRL);
    driver::settle();
    shot("07-undo");
    check(
        "ctrl+z undid the outdent",
        depths(&root) == [0, 1],
        format!("depths={:?}", depths(&root)),
    );

    // Clicking a block puts the caret in it. Find the editor that shows the first block, click that,
    // and the text must land there — not in whichever block happened to hold the focus.
    let seen: Vec<String> = (0..6).map(|index| driver::editor_text(index).to_string()).collect();
    let first_block = (0..6).find(|index| seen[*index] == "hello world");
    check(
        "the first block is on screen",
        first_block.is_some(),
        format!("rows seen={seen:?}"),
    );
    let first_block = first_block.unwrap_or(0);
    driver::click_block(first_block as i32);
    driver::wait_ms(400);
    // A click leaves the caret where it landed, so jump to the end before appending.
    driver::press_key(key::END, modifier::NONE);
    driver::wait_ms(200);
    driver::type_text(&QString::from("!"));
    driver::settle();
    shot("08-clicked");
    check(
        "clicking a block edits that block",
        blocks(&root) == ["hello world!", "second"],
        format!("blocks={:?}", blocks(&root)),
    );

    // Empty past days are hidden, so a column with nothing older is shorter than the window; the app
    // pulls older days until it fills rather than leaving a dead scroll. That is what grows here.
    for _ in 0..4 {
        driver::scroll_to_end();
        driver::wait_ms(300);
    }
    driver::settle();
    shot("09-scrolled");
    let loaded_after = driver::journal_property(&QString::from("loaded_days")).to_string();
    check(
        "older days are pulled in to fill the column",
        loaded_after != "14",
        format!("loaded days={loaded_after}"),
    );
    check(
        "empty past days take no room in the column",
        driver::row_count() < 10,
        format!("rows={} loaded={loaded_after}", driver::row_count()),
    );

    // ---- markers, scheduling and the agenda ------------------------------------------------
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Ctrl+Enter marks the focused block.
    driver::press_key(key::RETURN, modifier::CTRL);
    driver::settle();
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
    driver::settle();
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
    driver::settle();
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
    driver::settle();
    driver::press_key(key::DOWN, modifier::NONE);
    driver::wait_ms(200);
    driver::press_key(key::END, modifier::NONE);
    driver::type_text(&QString::from(" see [[My Project]]"));
    driver::settle();
    shot("14-links-typed");

    // The page the link created is reachable from the bar's search; the sidebar will list it once it
    // has been opened.
    driver::press_key(key::K, modifier::CTRL);
    driver::wait_ms(300);
    driver::type_text(&QString::from("My Project"));
    driver::settle();
    shot("15-search-for-page");
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    shot("16-page-view");
    check(
        "the page created by the link can be opened from search",
        driver::visible(&QString::from("favouriteStar")),
        format!("rows={}", driver::row_count()),
    );

    // Its reference panel: one linked reference, and the plain mention behind a collapsed group.
    let page_open = driver::row_count();
    check(
        "the page shows blocks and references",
        driver::visible(&QString::from("favouriteStar")) && page_open > 2,
        format!("rows={page_open}"),
    );

    // ---- the sidebar and the theme ---------------------------------------------------------
    // Opening the page put it in the sidebar's Recent list.
    let sidebar = driver::journal_property(&QString::from("sidebar")).to_string();
    check(
        "the page just opened is listed as recent",
        sidebar.contains("\"recent\":[\"My Project\"]"),
        format!("sidebar={sidebar}"),
    );

    // The star in the page header stars it, which fills Favourites.
    let starred = driver::click_named(&QString::from("favouriteStar"), 0);
    driver::settle();
    shot("16a-starred");
    let sidebar = driver::journal_property(&QString::from("sidebar")).to_string();
    check(
        "starring the page puts it in favourites",
        starred
            && driver::journal_property(&QString::from("page_favourite")).to_string() == "true"
            && sidebar.contains("\"favourites\":[\"My Project\"]"),
        format!(
            "starred={starred} page_favourite={} sidebar={sidebar}",
            driver::journal_property(&QString::from("page_favourite")).to_string()
        ),
    );

    // Ctrl+B hides the sidebar and brings it back.
    driver::press_key(key::B, modifier::CTRL);
    driver::wait_ms(400);
    check(
        "ctrl+b hides the sidebar",
        !driver::visible(&QString::from("sidebar")) && !driver::visible(&QString::from("sidebarRow")),
        "sidebar rows are gone".to_string(),
    );
    driver::press_key(key::B, modifier::CTRL);
    driver::wait_ms(400);
    check(
        "and shows it again",
        driver::visible(&QString::from("sidebar")) && driver::visible(&QString::from("sidebarRow")),
        "sidebar rows are back".to_string(),
    );

    // The theme button switches the whole window, and remembers it.
    let toggled = driver::click_named(&QString::from("themeButton"), 0);
    driver::settle();
    shot("16b-dark-theme");
    check(
        "the theme button switches to dark",
        toggled && driver::journal_property(&QString::from("theme")).to_string() == "dark",
        format!("theme={}", driver::journal_property(&QString::from("theme")).to_string()),
    );
    driver::click_named(&QString::from("themeButton"), 0);
    driver::settle();
    check(
        "and back to light",
        driver::journal_property(&QString::from("theme")).to_string() == "light",
        format!("theme={}", driver::journal_property(&QString::from("theme")).to_string()),
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

    let followed = driver::click_named(&QString::from("sourceLabel"), 0);
    driver::settle();
    shot("17a-from-label");
    check(
        "the from-label goes to the block it names",
        followed && !driver::visible(&QString::from("favouriteStar")),
        format!("rows={}", driver::row_count()),
    );
    driver::type_text(&QString::from("Z"));
    driver::settle();
    check(
        "and the caret lands in that block",
        blocks(&root).iter().any(|block| block.contains('Z')),
        format!("blocks={:?}", blocks(&root)),
    );

    let switched_back = driver::click_named(&QString::from("journalButton"), 0);
    driver::settle();
    check(
        "the Journal button returns to the column",
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
    driver::settle();
    shot("18-markdown-typed");

    // Which row is at which index is the view's business, and it moves as rows come and go; every step
    // below finds its block by what is in it.
    let marked = row_with("**bold**");
    let typed = driver::editor_text(marked).to_string();
    check(
        "the editor shows plain text, marks and all",
        typed.contains("**bold**") && typed.contains("`code`"),
        format!("text={typed:?}"),
    );

    // ---- a block at rest renders its markdown ----------------------------------------------
    // Focus another row so this one renders, then read *this* row's HTML.
    let elsewhere = if marked == 0 { 1 } else { 0 };
    // Past the text, away from any link in that row: this click only has to move the caret out.
    driver::click_block_at(elsewhere, 0.98);
    driver::settle();
    shot("19-block-at-rest");
    wait_until(|| !driver::row_html(marked).to_string().is_empty());
    let shown = driver::row_html(marked).to_string();
    check(
        "a block that is not being edited shows the finished text",
        shown.contains("font-weight:700")
            && shown.contains("monospace")
            && !shown.contains("**")
            && !shown.contains('`'),
        format!("html={shown:?}"),
    );

    // And clicking it edits it again, at the caret the click landed on, marks intact.
    let raw_before = driver::editor_text(marked).to_string();
    driver::click_block_at(marked, 0.95);
    driver::wait_ms(300);
    driver::type_text(&QString::from("~"));
    driver::settle();
    shot("20-click-to-edit");
    let raw_after = driver::editor_text(marked).to_string();
    check(
        "clicking a rendered block edits it again with its marks intact",
        raw_after.chars().count() == raw_before.chars().count() + 1 && raw_after.contains("**bold**"),
        format!("{raw_before:?} -> {raw_after:?}"),
    );

    // Typing the instant a rendered block is clicked: a person does not wait for the next frame, so the
    // keystrokes have to land rather than go nowhere. Aimed past the text, away from the row's link.
    let linked = row_with("second");
    let before_instant = driver::editor_text(linked).to_string();
    driver::click_block_at(linked, 0.95);
    driver::type_text(&QString::from("xyz"));
    driver::settle();
    let after_instant = driver::editor_text(linked).to_string();
    check(
        "typing straight after clicking a rendered block is not lost",
        after_instant.contains("xyz") && after_instant.len() == before_instant.len() + 3,
        format!("{before_instant:?} -> {after_instant:?}"),
    );

    // The same hazard one step earlier: Enter starts a new block asynchronously, so the next keystroke
    // has to land in it. 60 ms is a fast typist's gap; the split's own round trip is shorter than that.
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(60);
    driver::type_text(&QString::from("qp"));
    driver::settle();
    let landed = (0..10).find(|index| driver::editor_text(*index).to_string() == "qp");
    check(
        "typing straight after Enter is not lost",
        landed.is_some(),
        format!("rows={:?}", (0..4).map(|i| driver::editor_text(i).to_string()).collect::<Vec<_>>()),
    );

    // ---- starting a code block by typing ``` -----------------------------------------------
    // A new block, then the three backticks typed quickly — each keystroke landing inside the debounce
    // window, which is what a person does.
    driver::click_block_at(linked, 0.98);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    for _ in 0..3 {
        driver::type_text(&QString::from("`"));
        driver::wait_ms(90);
    }
    driver::settle();
    shot("21-typing-backticks");
    let backticks = row_with("```");
    let started = driver::editor_text(backticks).to_string();
    check(
        "typing ``` in a new block keeps the caret in it",
        driver::focus_name().to_string() == "blockEditor" && started == "```",
        format!(
            "focus={} text={started:?}",
            driver::focus_name().to_string()
        ),
    );

    // ---- a fence that spans lines, and the diagram it draws ---------------------------------
    let first = row_with("hello world");
    let blocks_before_fence = blocks(&root).len();
    driver::click_block(first);
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
    driver::settle();
    shot("22-diagram");

    let fenced = blocks(&root);
    check(
        "enter inside a fence kept the whole of it in one block",
        blocks_before_fence == fenced.len()
            && fenced.iter().any(|block| block.contains("```mermaid") && block.contains('\n')),
        format!("blocks={:?}", fenced),
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
    // Appended to the block that holds the link rather than split into a third: this check is about the
    // message, not about splitting. Aimed left of the row's link.
    driver::click_block_at(linked, 0.98);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```mermaid"));
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("this is not a diagram"));
    driver::settle();
    shot("23-diagram-error");

    let error = driver::named_text(&QString::from("diagramError"), 0).to_string();
    check(
        "a fence that does not parse says so in one quiet line",
        error.starts_with("diagram error: ") && !error.contains('\n'),
        format!("error={error:?}"),
    );

    // ---- a python fence is coloured ---------------------------------------------------------
    // A free block gets a fence whose language is known, then what it renders is read back.
    let free = row_with("qp");
    driver::click_block(free);
    driver::wait_ms(300);
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(100);
    driver::press_key(key::RETURN, modifier::SHIFT);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```python"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("def f():"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("    return 1"));
    driver::press_key(key::RETURN, modifier::NONE);
    driver::wait_ms(150);
    driver::type_text(&QString::from("```"));
    driver::settle();

    // Leave the block so it renders, then read its HTML.
    let python = row_with("```python");
    driver::click_block(first);
    driver::settle();
    shot("24-python-fence");
    wait_until(|| !driver::row_html(python).to_string().is_empty());
    let code_html = driver::row_html(python).to_string();
    check(
        "a python fence renders its keywords and numbers coloured",
        code_html.contains("def") && code_html.contains("#3a6ea5") && code_html.contains("#a4443c"),
        format!("html={code_html:?}"),
    );

    // ---- search -----------------------------------------------------------------------------
    // Ctrl+K over the whole graph: pages by name first, then blocks by their text.
    driver::press_key(key::K, modifier::CTRL);
    driver::wait_ms(500);
    check(
        "ctrl+k opens the search field",
        driver::focus_name().to_string() == "searchField",
        format!("focus={}", driver::focus_name().to_string()),
    );

    driver::type_text(&QString::from("Project"));
    driver::settle();
    shot("25-search");
    check(
        "typing finds the page and the blocks that mention it",
        driver::search_count() >= 2,
        format!("hits={}", driver::search_count()),
    );

    // The page ranks first, so Enter opens it.
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    shot("26-search-opened-page");
    let opened_page_rows = driver::row_count();
    check(
        "enter opens the page the first hit names",
        opened_page_rows > 0 && opened_page_rows < 20,
        format!("rows={opened_page_rows}"),
    );

    // A block hit opens the block itself, with the caret in it.
    driver::press_key(key::K, modifier::CTRL);
    driver::wait_ms(400);
    // "world" appears only in block text, so the first hit is a block — and it is still one word, unlike
    // "matters", which an earlier step of this run split with a ~.
    driver::type_text(&QString::from("world"));
    driver::settle();
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    driver::type_text(&QString::from("Q"));
    driver::settle();
    shot("27-search-opened-block");
    check(
        "a block hit opens that block with the caret on the match",
        blocks(&root).iter().any(|block| block.contains("Qworld")),
        format!("blocks={:?}", blocks(&root)),
    );

    // ---- the full results view ---------------------------------------------------------------
    // The preview is capped; Ctrl+Enter in the field asks for everything, in the column area.
    driver::press_key(key::K, modifier::CTRL);
    driver::wait_ms(400);
    driver::type_text(&QString::from("My Project"));
    driver::settle();
    driver::press_key(key::RETURN, modifier::CTRL);
    driver::settle();
    shot("26a-all-results");
    check(
        "ctrl+enter in the field opens the full results",
        driver::journal_property(&QString::from("view")).to_string() == "search",
        format!(
            "view={} rows={}",
            driver::journal_property(&QString::from("view")).to_string(),
            driver::row_count()
        ),
    );
    let result_rows = driver::row_count();
    check(
        "the results view lists headings and matches",
        result_rows > 2,
        format!("rows={result_rows}"),
    );
    driver::press_key(key::ESCAPE, modifier::NONE);
    driver::settle();
    check(
        "escape leaves the results and returns to the column",
        driver::journal_property(&QString::from("view")).to_string() == "journal",
        format!("view={}", driver::journal_property(&QString::from("view")).to_string()),
    );

    // ---- a link in a block at rest opens its page --------------------------------------------
    // Nothing is editing block 1, so its [[My Project]] is a real anchor. Find its pixels with the
    // view's own hit test and click it.
    driver::click_block(0);
    driver::wait_ms(400);
    check(
        "a link in a block at rest can be clicked",
        driver::click_link(1),
        "found a link under the pointer and clicked it".to_string(),
    );
    driver::settle();
    shot("28-link-opened");
    let linked_rows = driver::row_count();
    check(
        "clicking a link opens that page",
        linked_rows > 0 && linked_rows < 20,
        format!("rows={linked_rows}"),
    );

    // And the page it opened takes a keystroke — a page with no blocks used to fail here.
    driver::type_text(&QString::from("R"));
    driver::settle();
    shot("29-link-typed");
    let on_page = page_blocks(&root, "My Project");
    check(
        "the page a link opened takes a keystroke",
        on_page.iter().any(|block| block.contains('R')),
        format!(
            "page blocks={on_page:?} journal={:?} focus={}",
            blocks(&root),
            driver::focus_name().to_string()
        ),
    );

    // ---- slash commands at block level --------------------------------------------------------
    // Back in the journal column: the search steps above left the window on a page, and a slash command
    // belongs to whatever block the caret is in, so start from the column the rest of the run is about.
    driver::click_named(&QString::from("journalButton"), 0);
    driver::settle();

    // Nothing below assumes a row: this scenario follows the app, reading the store once per step and
    // finding what it looks for by content, because *which* block holds the caret is the view's
    // business. Typing "/" opens the menu; Enter then picks the highlighted command rather than
    // splitting the block, and the token does not survive it.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let todo_before = markers(&root)
        .into_iter()
        .filter(|marker| *marker == Some(Marker::Todo))
        .count();
    driver::type_text(&QString::from("/"));
    driver::settle();
    shot("30-slash-menu");
    check(
        "typing a slash opens the command menu",
        driver::visible(&QString::from("slashMenu")),
        format!("focus={}", driver::focus_name().to_string()),
    );
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    shot("31-slash-todo");
    let after_menu = blocks(&root);
    let todos_now = markers(&root)
        .into_iter()
        .filter(|marker| *marker == Some(Marker::Todo))
        .count();
    check(
        "a slash command marks the block and eats the token it replaced",
        todos_now == todo_before + 1 && !after_menu.iter().any(|text| text.contains('/')),
        format!("blocks={after_menu:?} todos {todo_before} -> {todos_now}"),
    );

    // Typing after the "/" narrows the list; Down moves the highlight to today's date command, which
    // writes a link to the journal day instead of setting anything on the block.
    let today_link = format!("[[{today}]]");
    driver::type_text(&QString::from("/t"));
    driver::settle();
    shot("32-slash-filtered");
    check(
        "the menu follows what is typed after the slash",
        driver::visible(&QString::from("slashMenu")),
        format!("focus={}", driver::focus_name().to_string()),
    );
    driver::press_key(key::DOWN, modifier::NONE);
    driver::wait_ms(200);
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    let after_date = blocks(&root);
    let target = after_date.iter().position(|text| text.contains(&today_link));
    check(
        "down then enter picks the next match, and a date command writes a link to the day",
        target.is_some(),
        format!("blocks={after_date:?}"),
    );
    let target = target.expect("the block the date command wrote into");

    // Esc is the way out that changes nothing: the token stays as ordinary text, and is written.
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(200);
    driver::type_text(&QString::from("/tod"));
    driver::settle();
    driver::press_key(key::ESCAPE, modifier::NONE);
    driver::settle();
    let after_esc = blocks(&root);
    check(
        "esc leaves the token as typed instead of running a command",
        after_esc
            .get(target)
            .map(|text| text.contains(&today_link) && text.ends_with("/tod"))
            .unwrap_or(false),
        format!("blocks={after_esc:?}"),
    );

    // The scheduled command goes through the same calendar Ctrl+Shift+S opens — Right, then Enter, is
    // tomorrow — and the date plus the token's removal are one write: one Ctrl+Z takes the whole
    // command back, date and text together. Saving is held while the menu is open, so the text the
    // undo returns to is the block as it stood before the token was typed.
    let text_before = after_esc.get(target).cloned().unwrap_or_default();
    let dates_before = scheduled(&root);
    let tomorrow = (chrono::Local::now() + chrono::Days::new(1))
        .format("%Y-%m-%d")
        .to_string();
    driver::type_text(&QString::from(" /sch"));
    driver::settle();
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    shot("34-slash-scheduled");
    driver::press_key(key::RIGHT, modifier::NONE);
    driver::wait_ms(300);
    driver::press_key(key::RETURN, modifier::NONE);
    driver::settle();
    let after_scheduled = blocks(&root);
    let dates = scheduled(&root);
    check(
        "the scheduled command opens the calendar, and accepting it dates the block",
        dates.get(target) == Some(&Some(tomorrow.clone()))
            && after_scheduled
                .get(target)
                .map(|text| !text.contains("/sch"))
                .unwrap_or(false),
        format!("scheduled={dates:?} blocks={after_scheduled:?}"),
    );

    let dated = after_scheduled.get(target).cloned().unwrap_or_default();
    driver::press_key(key::Z, modifier::CTRL);
    driver::settle();
    let after_undo = blocks(&root);
    let dates_undone = scheduled(&root);
    check(
        "one undo takes back the command's date and the token it replaced",
        after_undo.get(target) == Some(&text_before)
            && after_undo.get(target) != Some(&dated)
            && dates_undone.get(target) == dates_before.get(target),
        format!(
            "blocks={after_undo:?} scheduled={dates_undone:?} was {dates_before:?}"
        ),
    );
    driver::press_key(key::Z, modifier::CTRL | modifier::SHIFT);
    driver::settle();

    // A date link really is a link to the day: clicking it opens that day as a page, under the same day
    // heading the column uses and without the star a named page carries. Down leaves the block first,
    // so it is rendered and its link can be clicked.
    driver::press_key(key::DOWN, modifier::NONE);
    driver::settle();
    check(
        "the date link in a block at rest can be clicked",
        driver::click_link(target as i32),
        format!("row {target} of {:?}", blocks(&root)),
    );
    driver::settle();
    shot("33-day-page");
    let heading = chrono::Local::now().format("%A, %B %-d, %Y").to_string();
    let shown = driver::named_text(&QString::from("pageHeading"), 0).to_string();
    check(
        "clicking a date link opens that day as a page under its day heading",
        shown == heading && !driver::visible(&QString::from("favouriteStar")),
        format!("heading={shown:?} expected {heading:?}"),
    );

    // Back to the journal for the close test.
    driver::click_named(&QString::from("journalButton"), 0);
    driver::settle();

    // Leave the caret where the close test expects it: at the end of the first block. The click goes to
    // the row's left edge, because that block now starts with a date link and a click on a link in a
    // block at rest follows it instead of putting the caret anywhere.
    driver::click_block_at(0, 0.05);
    driver::settle();
    // Ctrl+End, not End: a block can hold several lines now, and End stops at the end of the line.
    driver::press_key(key::END, modifier::CTRL);
    driver::wait_ms(200);

    check(
        "the journal is reachable again from the switcher",
        !driver::visible(&QString::from("favouriteStar")),
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

/// The row whose editor holds `needle`, found by content: a row's index moves as rows come and go, and a
/// rendered row still reports its raw text.
fn row_with(needle: &str) -> i32 {
    (0..12)
        .find(|index| driver::editor_text(*index).to_string().contains(needle))
        .unwrap_or(0)
}

/// Waits until `settled` holds, giving up after a few seconds. Every save rebuilds the model, so a read
/// taken mid-rebuild can see a delegate that is not there yet.
fn wait_until(settled: impl Fn() -> bool) {
    for _ in 0..30 {
        if settled() {
            return;
        }
        driver::wait_ms(100);
    }
}

fn page_blocks(root: &std::path::Path, name: &str) -> Vec<String> {
    let store = Store::open(root.join("lognotes.db")).expect("open the livetest database");
    let Some(page) = store.page_by_name(name).expect("read the page") else {
        return Vec::new();
    };
    store
        .page_blocks(page.id)
        .expect("read the page's blocks")
        .into_iter()
        .map(|block| block.content)
        .collect()
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
            // Not `settle()`: it reads the model through the attached root, and there is none yet.
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
