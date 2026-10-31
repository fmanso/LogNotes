//! The bridge between the journal column and `lognotes-core`.
//!
//! The store lives on one worker thread, which owns the connection for its whole life. QML calls
//! invokables on the GUI thread; those send commands to the worker, and the worker queues the resulting
//! rows back onto the GUI thread before touching the model. Nothing here holds a `Connection`, so the
//! view cannot reach the database even by accident.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread;

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QModelIndex, QString, QVariant};
use lognotes_core::{BlockRow, Store};

use qobject::{Journal, QHash_i32_QByteArray, Role};

/// Where the database lives, set by `main` before the window loads.
static DATABASE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn set_database_path(path: impl Into<PathBuf>) {
    let _ = DATABASE_PATH.set(path.into());
}

/// How many days each lazy chunk of the column covers.
const CHUNK_DAYS: u32 = 14;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(< QAbstractListModel >);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    #[qenum(Journal)]
    enum Role {
        Kind,
        Text,
        Depth,
        Date,
        BlockId,
        Placeholder,
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, error)]
        #[qproperty(bool, ready)]
        #[qproperty(bool, can_undo)]
        #[qproperty(bool, can_redo)]
        #[qproperty(u64, revision)]
        type Journal = super::JournalRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        /// Open the database and render the first chunk. Call again to retry after an error.
        #[qinvokable]
        fn start(self: Pin<&mut Self>);

        /// Append the previous chunk of journal days.
        #[qinvokable]
        fn load_more(self: Pin<&mut Self>);

        /// Persist the text of a block (or materialise a day's placeholder block).
        #[qinvokable]
        fn edit(self: Pin<&mut Self>, row: i32, text: &QString);

        /// Enter: persist the text, then split at `offset` into a new block.
        #[qinvokable]
        fn submit_block(self: Pin<&mut Self>, row: i32, offset: i32, text: &QString);

        /// Persist an edit and wait for the worker to commit it — used when the window closes, where a
        /// debounced save would otherwise be lost with the process.
        #[qinvokable]
        fn save_now(self: Pin<&mut Self>, row: i32, text: &QString);

        #[qinvokable]
        fn indent(self: Pin<&mut Self>, row: i32);

        #[qinvokable]
        fn outdent(self: Pin<&mut Self>, row: i32);

        #[qinvokable]
        fn remove(self: Pin<&mut Self>, row: i32);

        #[qinvokable]
        fn undo(self: Pin<&mut Self>);

        #[qinvokable]
        fn redo(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for Journal {}
}

/// One row of the column: a date heading, or a block under it.
#[derive(Clone)]
struct Row {
    heading: bool,
    date: String,
    block_id: i32,
    depth: u32,
    text: String,
    placeholder: bool,
}

#[derive(Default)]
pub struct JournalRust {
    rows: Vec<Row>,
    dates: Vec<String>,
    worker: Option<Sender<Job>>,
    error: QString,
    ready: bool,
    can_undo: bool,
    can_redo: bool,
    /// Bumped on every applied snapshot. QML restores the caret from this: the row count alone does not
    /// change when a block is merely edited, so `countChanged` cannot be the trigger.
    revision: u64,
}

enum Command {
    Update { block_id: i32, text: String },
    Materialize { date: String, text: String },
    Submit { date: String, block_id: i32, text: String, offset: usize },
    Indent { block_id: i32 },
    Outdent { block_id: i32 },
    Delete { block_id: i32 },
    Undo,
    Redo,
}

enum Job {
    Start {
        dates: Vec<String>,
        queue: CxxQtThread<Journal>,
    },
    /// Run an optional command, then hand back a fresh snapshot of `dates`.
    Run {
        command: Option<Command>,
        dates: Vec<String>,
        queue: CxxQtThread<Journal>,
    },
    /// Run a command and signal completion, so the caller can wait for the write to land.
    Flush {
        command: Command,
        done: Sender<()>,
    },
}

/// What the worker sends back to the GUI thread.
struct Outcome {
    error: Option<String>,
    days: Vec<(String, Vec<BlockRow>)>,
    ready: bool,
    can_undo: bool,
    can_redo: bool,
}

impl JournalRust {
    fn send(&self, job: Job) -> bool {
        match &self.worker {
            Some(tx) => tx.send(job).is_ok(),
            None => false,
        }
    }

    /// The block id at `row`, ignoring headings and un-materialised placeholders.
    fn block_at(&self, row: i32) -> Option<i32> {
        let row = self.rows.get(row as usize)?;
        if row.heading || row.placeholder {
            None
        } else {
            Some(row.block_id)
        }
    }
}

impl qobject::Journal {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let role = Role { repr: role };
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            Role::Kind => {
                let kind = if row.heading {
                    "heading"
                } else if row.placeholder {
                    "placeholder"
                } else {
                    "block"
                };
                QVariant::from(&QString::from(kind))
            }
            Role::Text => QVariant::from(&QString::from(row.text.as_str())),
            Role::Depth => QVariant::from(&(row.depth as i32)),
            Role::Date => QVariant::from(&QString::from(row.date.as_str())),
            Role::BlockId => QVariant::from(&row.block_id),
            Role::Placeholder => QVariant::from(&row.placeholder),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash_i32_QByteArray {
        let mut hash = QHash_i32_QByteArray::default();
        hash.insert(Role::Kind.repr, QByteArray::from("kind"));
        hash.insert(Role::Text.repr, QByteArray::from("text"));
        hash.insert(Role::Depth.repr, QByteArray::from("depth"));
        hash.insert(Role::Date.repr, QByteArray::from("date"));
        hash.insert(Role::BlockId.repr, QByteArray::from("blockId"));
        hash.insert(Role::Placeholder.repr, QByteArray::from("placeholder"));
        hash
    }

    fn start(mut self: Pin<&mut Self>) {
        let dates = if self.dates.is_empty() {
            let dates = today_chunk(CHUNK_DAYS);
            self.as_mut().rust_mut().dates = dates.clone();
            dates
        } else {
            self.dates.clone()
        };

        if self.worker.is_none() {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || worker_loop(rx));
            self.as_mut().rust_mut().worker = Some(tx);
        }

        let queue = self.as_mut().qt_thread();
        self.send(Job::Start { dates, queue });
    }

    fn load_more(mut self: Pin<&mut Self>) {
        let Some(oldest) = self.dates.last().cloned() else {
            return;
        };
        if self.worker.is_none() {
            return;
        }
        let mut dates = self.dates.clone();
        dates.extend(older_chunk(&oldest, CHUNK_DAYS));
        self.as_mut().rust_mut().dates = dates.clone();

        let queue = self.as_mut().qt_thread();
        self.send(Job::Run {
            command: None,
            dates,
            queue,
        });
    }

    fn edit(mut self: Pin<&mut Self>, row: i32, text: &QString) {
        let Some(target) = self.rows.get(row as usize).cloned() else {
            return;
        };
        let command = if target.placeholder {
            Command::Materialize {
                date: target.date,
                text: text.to_string(),
            }
        } else if target.heading {
            return;
        } else {
            Command::Update {
                block_id: target.block_id,
                text: text.to_string(),
            }
        };
        self.as_mut().dispatch(Some(command));
    }

    fn save_now(mut self: Pin<&mut Self>, row: i32, text: &QString) {
        let Some(target) = self.rows.get(row as usize).cloned() else {
            return;
        };
        if target.heading {
            return;
        }
        let command = if target.placeholder {
            Command::Materialize {
                date: target.date,
                text: text.to_string(),
            }
        } else {
            Command::Update {
                block_id: target.block_id,
                text: text.to_string(),
            }
        };
        let (done, wait) = mpsc::channel();
        if self.send(Job::Flush { command, done }) {
            // Bounded so a wedged worker cannot hang the close forever.
            let _ = wait.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    fn submit_block(mut self: Pin<&mut Self>, row: i32, offset: i32, text: &QString) {
        let Some(target) = self.rows.get(row as usize).cloned() else {
            return;
        };
        if target.heading {
            return;
        }
        let command = Command::Submit {
            date: target.date,
            block_id: target.block_id,
            text: text.to_string(),
            offset: offset.max(0) as usize,
        };
        self.as_mut().dispatch(Some(command));
    }

    fn indent(mut self: Pin<&mut Self>, row: i32) {
        if let Some(block_id) = self.block_at(row) {
            self.as_mut().dispatch(Some(Command::Indent { block_id }));
        }
    }

    fn outdent(mut self: Pin<&mut Self>, row: i32) {
        if let Some(block_id) = self.block_at(row) {
            self.as_mut().dispatch(Some(Command::Outdent { block_id }));
        }
    }

    fn remove(mut self: Pin<&mut Self>, row: i32) {
        if let Some(block_id) = self.block_at(row) {
            self.as_mut().dispatch(Some(Command::Delete { block_id }));
        }
    }

    fn undo(mut self: Pin<&mut Self>) {
        self.as_mut().dispatch(Some(Command::Undo));
    }

    fn redo(mut self: Pin<&mut Self>) {
        self.as_mut().dispatch(Some(Command::Redo));
    }

    fn dispatch(mut self: Pin<&mut Self>, command: Option<Command>) {
        let dates = self.dates.clone();
        if dates.is_empty() || self.worker.is_none() {
            return;
        }
        let queue = self.as_mut().qt_thread();
        self.send(Job::Run {
            command,
            dates,
            queue,
        });
    }

    /// Called on the GUI thread once the worker has finished: swap in the new rows.
    fn apply(mut self: Pin<&mut Self>, outcome: Outcome) {
        if let Some(error) = &outcome.error {
            self.as_mut().set_error(QString::from(error.as_str()));
            self.as_mut().set_ready(false);
            self.as_mut().bump_revision();
            return;
        }

        self.as_mut().begin_reset_model();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.rows = outcome
                .days
                .iter()
                .flat_map(|(date, blocks)| day_rows(date, blocks))
                .collect();
            rust.can_undo = outcome.can_undo;
            rust.can_redo = outcome.can_redo;
        }
        self.as_mut().end_reset_model();

        self.as_mut().set_error(QString::from(""));
        self.as_mut().set_ready(outcome.ready);
        self.as_mut().set_can_undo(outcome.can_undo);
        self.as_mut().set_can_redo(outcome.can_redo);
        self.as_mut().bump_revision();
    }

    fn bump_revision(mut self: Pin<&mut Self>) {
        let next = self.revision.wrapping_add(1);
        self.as_mut().set_revision(next);
    }
}

fn worker_loop(rx: mpsc::Receiver<Job>) {
    let mut store: Option<Store> = None;

    while let Ok(job) = rx.recv() {
        match job {
            Job::Start { dates, queue } => {
                let opened = match DATABASE_PATH.get() {
                    // The storage error already names the file, so it is not appended again.
                    Some(path) => Store::open(path).map_err(|error| error.to_string()),
                    None => Err("no database path was configured".to_string()),
                };
                match opened {
                    Ok(opened) => {
                        let outcome = snapshot(&opened, &dates);
                        store = Some(opened);
                        deliver(queue, outcome);
                    }
                    Err(error) => deliver(
                        queue,
                        Outcome {
                            error: Some(error),
                            days: Vec::new(),
                            ready: false,
                            can_undo: false,
                            can_redo: false,
                        },
                    ),
                }
            }
            Job::Flush { command, done } => {
                if let Some(store) = store.as_mut() {
                    let _ = run_command(store, command);
                }
                let _ = done.send(());
            }
            Job::Run {
                command,
                dates,
                queue,
            } => {
                let Some(store) = store.as_mut() else {
                    continue;
                };
                let run = match command {
                    Some(command) => run_command(store, command),
                    None => Ok(()),
                };
                let outcome = match run {
                    Ok(()) => snapshot(store, &dates),
                    Err(error) => Outcome {
                        error: Some(error),
                        days: Vec::new(),
                        ready: true,
                        can_undo: store.can_undo(),
                        can_redo: store.can_redo(),
                    },
                };
                deliver(queue, outcome);
            }
        }
    }
}

fn deliver(queue: CxxQtThread<Journal>, outcome: Outcome) {
    let _ = queue.queue(move |journal: Pin<&mut Journal>| journal.apply(outcome));
}

fn run_command(store: &mut Store, command: Command) -> Result<(), String> {
    let fail = |error: lognotes_core::Error| error.to_string();

    match command {
        Command::Update { block_id, text } => store.update_content(block_id as i64, &text).map_err(fail),
        Command::Materialize { date, text } => {
            let page = store.ensure_journal_page(&date).map_err(fail)?;
            store.insert_block(page, None, &text).map_err(fail)?;
            Ok(())
        }
        Command::Submit {
            date,
            block_id,
            text,
            offset,
        } => {
            let page = store.ensure_journal_page(&date).map_err(fail)?;
            let boundary = text
                .char_indices()
                .nth(offset)
                .map(|(index, _)| index)
                .unwrap_or(text.len());
            // Write the whole text first: `split_block` cuts what is stored, so trimming it to the left
            // half beforehand would throw the tail away.
            let target = if block_id < 0 {
                store.insert_block(page, None, &text).map_err(fail)?
            } else {
                store.update_content(block_id as i64, &text).map_err(fail)?;
                block_id as i64
            };
            if boundary >= text.len() {
                store.insert_block(page, Some(target), "").map_err(fail)?;
            } else {
                store.split_block(target, boundary).map_err(fail)?;
            }
            Ok(())
        }
        Command::Indent { block_id } => store.indent(block_id as i64).map_err(fail),
        Command::Outdent { block_id } => store.outdent(block_id as i64).map_err(fail),
        Command::Delete { block_id } => store.delete_block(block_id as i64).map_err(fail),
        Command::Undo => store.undo().map(|_| ()).map_err(fail),
        Command::Redo => store.redo().map(|_| ()).map_err(fail),
    }
}

fn snapshot(store: &Store, dates: &[String]) -> Outcome {
    let days = dates
        .iter()
        .map(|date| {
            let blocks = store.day(date).map(|day| day.blocks).unwrap_or_default();
            (date.clone(), blocks)
        })
        .collect();

    Outcome {
        error: None,
        days,
        ready: true,
        can_undo: store.can_undo(),
        can_redo: store.can_redo(),
    }
}

fn day_rows(date: &str, blocks: &[BlockRow]) -> Vec<Row> {
    let mut rows = vec![Row {
        heading: true,
        date: date.to_string(),
        block_id: -1,
        depth: 0,
        text: String::new(),
        placeholder: false,
    }];

    if blocks.is_empty() {
        // The empty block the cursor lands in. It is never persisted: the first keystroke
        // materialises it, so scrolling a year back cannot fill the database with empty days.
        rows.push(Row {
            heading: false,
            date: date.to_string(),
            block_id: -1,
            depth: 0,
            text: String::new(),
            placeholder: true,
        });
        return rows;
    }

    rows.extend(blocks.iter().map(|block| Row {
        heading: false,
        date: date.to_string(),
        block_id: block.id as i32,
        depth: block.depth,
        text: block.content.clone(),
        placeholder: false,
    }));
    rows
}

fn today_chunk(count: u32) -> Vec<String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    chunk_from(&today, count)
}

fn older_chunk(oldest: &str, count: u32) -> Vec<String> {
    match chrono::NaiveDate::parse_from_str(oldest, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.checked_sub_days(chrono::Days::new(1)))
    {
        Some(previous) => chunk_from(&previous.format("%Y-%m-%d").to_string(), count),
        None => Vec::new(),
    }
}

fn chunk_from(start: &str, count: u32) -> Vec<String> {
    match chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d") {
        Ok(date) => (0..count)
            .filter_map(|back| date.checked_sub_days(chrono::Days::new(back as u64)))
            .map(|day| day.format("%Y-%m-%d").to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lognotes_core::Store;

    fn block(id: i64, depth: u32, content: &str) -> BlockRow {
        BlockRow {
            id,
            parent_id: None,
            depth,
            order_key: String::new(),
            content: content.to_string(),
            marker: None,
            scheduled_date: None,
            collapsed: false,
        }
    }

    #[test]
    fn an_empty_day_renders_a_heading_and_one_placeholder() {
        let rows = day_rows("2026-04-13", &[]);

        assert_eq!(rows.len(), 2);
        assert!(rows[0].heading);
        assert_eq!(rows[0].date, "2026-04-13");
        assert!(!rows[1].heading);
        assert!(rows[1].placeholder);
        assert_eq!(rows[1].block_id, -1, "the placeholder is never a real row");
    }

    #[test]
    fn a_day_renders_its_blocks_under_the_heading_with_depth() {
        let rows = day_rows("2026-04-13", &[block(7, 0, "one"), block(8, 1, "two")]);

        assert_eq!(rows.len(), 3);
        assert!(rows[0].heading);
        assert_eq!((rows[1].block_id, rows[1].depth), (7, 0));
        assert_eq!((rows[2].block_id, rows[2].depth), (8, 1));
        assert!(rows[1..].iter().all(|row| !row.placeholder && !row.heading));
    }

    #[test]
    fn a_chunk_runs_backwards_from_its_start_date() {
        let chunk = chunk_from("2026-04-13", 3);

        assert_eq!(chunk, ["2026-04-13", "2026-04-12", "2026-04-11"]);
    }

    #[test]
    fn the_next_chunk_continues_before_the_oldest_loaded_day() {
        let older = older_chunk("2026-04-11", 2);

        assert_eq!(older, ["2026-04-10", "2026-04-09"]);
    }

    fn session() -> Store {
        Store::open_in_memory().expect("in-memory store")
    }

    fn blocks(store: &Store, date: &str) -> Vec<String> {
        let page = store.journal_page(date).unwrap().expect("day exists");
        store
            .page_blocks(page.id)
            .unwrap()
            .into_iter()
            .map(|block| block.content)
            .collect()
    }

    #[test]
    fn the_first_keystroke_materialises_the_placeholder() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                date: "2026-04-13".into(),
                text: "hello".into(),
            },
        )
        .unwrap();

        assert_eq!(blocks(&store, "2026-04-13"), ["hello"]);
    }

    #[test]
    fn enter_mid_block_splits_and_keeps_both_halves() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                date: "2026-04-13".into(),
                text: "helloworld".into(),
            },
        )
        .unwrap();
        let id = store
            .journal_page("2026-04-13")
            .unwrap()
            .map(|page| store.page_blocks(page.id).unwrap()[0].id as i32)
            .expect("a block exists");

        run_command(
            &mut store,
            Command::Submit {
                date: "2026-04-13".into(),
                block_id: id,
                text: "helloworld".into(),
                offset: 5,
            },
        )
        .unwrap();

        // Pre-setting the left half before splitting would leave an empty tail instead of "world".
        assert_eq!(blocks(&store, "2026-04-13"), ["hello", "world"]);
    }

    #[test]
    fn enter_at_the_end_appends_an_empty_block() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                date: "2026-04-13".into(),
                text: "hello".into(),
            },
        )
        .unwrap();
        let id = store
            .journal_page("2026-04-13")
            .unwrap()
            .map(|page| store.page_blocks(page.id).unwrap()[0].id as i32)
            .expect("a block exists");

        run_command(
            &mut store,
            Command::Submit {
                date: "2026-04-13".into(),
                block_id: id,
                text: "hello".into(),
                offset: 5,
            },
        )
        .unwrap();

        assert_eq!(blocks(&store, "2026-04-13"), ["hello", ""]);
    }

    #[test]
    fn enter_on_a_placeholder_splits_it_too() {
        let mut store = session();

        run_command(
            &mut store,
            Command::Submit {
                date: "2026-04-13".into(),
                block_id: -1,
                text: "helloworld".into(),
                offset: 5,
            },
        )
        .unwrap();

        assert_eq!(blocks(&store, "2026-04-13"), ["hello", "world"]);
    }

    #[test]
    fn undo_steps_back_through_bridge_commands() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                date: "2026-04-13".into(),
                text: "hello".into(),
            },
        )
        .unwrap();
        let id = store
            .journal_page("2026-04-13")
            .unwrap()
            .map(|page| store.page_blocks(page.id).unwrap()[0].id as i32)
            .expect("a block exists");
        run_command(
            &mut store,
            Command::Submit {
                date: "2026-04-13".into(),
                block_id: id,
                text: "helloworld".into(),
                offset: 5,
            },
        )
        .unwrap();

        run_command(&mut store, Command::Undo).unwrap();
        assert_eq!(blocks(&store, "2026-04-13"), ["helloworld"]);

        run_command(&mut store, Command::Redo).unwrap();
        assert_eq!(blocks(&store, "2026-04-13"), ["hello", "world"]);
    }

    /// cxx-qt exposes a Rust identifier in QML verbatim — `submit_block`, not `submitBlock`. A typo
    /// there compiles and only fails when the key is pressed, so the names are checked here instead.
    #[test]
    fn qml_only_calls_invokables_the_bridge_declares() {
        let bridge = include_str!("journal.rs");
        let qml = include_str!("../qml/main.qml");

        let mut invokables: Vec<String> = Vec::new();
        let mut lines = bridge.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "#[qinvokable]"
                && let Some(declaration) = lines.next().map(str::trim)
                && let Some(rest) = declaration.strip_prefix("fn ")
            {
                let name = rest.split('(').next().unwrap_or("").trim();
                invokables.push(name.to_string());
            }
        }
        assert!(
            invokables.len() > 5,
            "expected to find the bridge's invokables, found {invokables:?}"
        );

        let mut calls: Vec<String> = Vec::new();
        let mut rest = qml;
        while let Some(at) = rest.find("journal.") {
            rest = &rest[at + "journal.".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if rest[name.len()..].starts_with('(') {
                calls.push(name);
            }
        }
        assert!(!calls.is_empty(), "expected the QML to call the model");

        for call in calls {
            assert!(
                invokables.contains(&call),
                "QML calls journal.{call}, but the bridge declares only {invokables:?}"
            );
        }
    }

    #[test]
    fn an_unparseable_date_yields_no_chunk() {
        assert!(chunk_from("not-a-date", 3).is_empty());
    }
}
