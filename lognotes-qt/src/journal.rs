//! The bridge between the journal column and `lognotes-core`.
//!
//! The store lives on one worker thread, which owns the connection for its whole life. QML calls
//! invokables on the GUI thread; those send commands to the worker, and the worker queues the resulting
//! rows back onto the GUI thread before touching the model. Nothing here holds a `Connection`, so the
//! view cannot reach the database even by accident.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread;

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QModelIndex, QString, QVariant};
use lognotes_core::markdown::{self, LinkKind};
use lognotes_core::{
    AgendaRow, BlockRow, Marker, ReferenceKind, ReferenceRow, SearchResults, Store,
};

use crate::diagram::{Diagrams, Drawn};
use qobject::{Journal, QHash_i32_QByteArray, Role};

/// Where the database lives, set by `main` before the window loads.
static DATABASE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn set_database_path(path: impl Into<PathBuf>) {
    let _ = DATABASE_PATH.set(path.into());
}

/// How many days each lazy chunk of the column covers.
const CHUNK_DAYS: u32 = 14;

/// How far ahead the agenda looks. The rule itself lives in `lognotes`.
const AGENDA_HORIZON_DAYS: u32 = 7;

/// The colours the rendered HTML uses — fence tokens and links: the app's own palette, and the only place
/// HTML colour is chosen. They are passed into the renderer because Qt's rich text takes colours inline
/// only, and because Qt's default link colour is a stock blue that belongs to neither theme.
const INK_LIGHT: markdown::Ink<'_> = markdown::Ink {
    link: "#3a6ea5",
    keyword: "#3a6ea5",
    string: "#4a7c59",
    comment: "#a39c90",
    number: "#a4443c",
};

const INK_DARK: markdown::Ink<'_> = markdown::Ink {
    link: "#8fb6e8",
    keyword: "#8fb6e8",
    string: "#8fc79c",
    comment: "#8f8a80",
    number: "#e0948a",
};

fn ink(theme: &str) -> &'static markdown::Ink<'static> {
    if theme == "dark" { &INK_DARK } else { &INK_LIGHT }
}

/// How many recently opened pages the sidebar remembers.
const SIDEBAR_RECENT: usize = 8;

/// How many page names and how many blocks the quick switcher previews for one query.
const SEARCH_PAGES: usize = 5;
const SEARCH_BLOCKS: usize = 20;

/// How many the full results view asks for: the preview's caps are what it exists to lift.
const ALL_PAGES: usize = 50;
const ALL_BLOCKS: usize = 500;

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
        Marker,
        Scheduled,
        Source,
        Overdue,
        /// A read-only row's text as escaped HTML: styled, with the markdown's own marks dropped.
        Html,
        /// `file://` URL of a block's drawn mermaid diagram, empty when it has none.
        Diagram,
        /// One line explaining why a diagram could not be drawn.
        DiagramError,
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
        #[qproperty(QString, hits)]
        #[qproperty(QString, theme)]
        #[qproperty(QString, view)]
        #[qproperty(i32, loaded_days)]
        #[qproperty(QString, sidebar)]
        #[qproperty(bool, page_favourite)]
        #[qproperty(i32, focus_row)]
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

        /// Cycle a block's marker: none → TODO → DOING → DONE → none.
        #[qinvokable]
        fn cycle_marker(self: Pin<&mut Self>, row: i32);

        /// Set or clear a block's scheduled date. An empty string clears it.
        #[qinvokable]
        fn set_scheduled(self: Pin<&mut Self>, row: i32, date: &QString);

        /// Set a block's marker directly: "TODO", "DOING", "DONE", or "" to clear it.
        #[qinvokable]
        fn set_marker(self: Pin<&mut Self>, row: i32, marker: &QString);

        /// Set a block's marker and the text a slash command rewrote, as one undo step.
        #[qinvokable]
        fn set_marker_with_content(
            self: Pin<&mut Self>,
            row: i32,
            marker: &QString,
            text: &QString,
        );

        /// The same, for a scheduled date with the text the command rewrote.
        #[qinvokable]
        fn set_scheduled_with_content(
            self: Pin<&mut Self>,
            row: i32,
            date: &QString,
            text: &QString,
        );

        /// Show the journal column again.
        #[qinvokable]
        fn show_journal(self: Pin<&mut Self>);

        /// Open a named page: its blocks and its references. Creates the page if it does not exist.
        #[qinvokable]
        fn open_page(self: Pin<&mut Self>, name: &QString);

        /// The link under `position` in `text`, as `page:<name>` for a page link or a tag and the URL
        /// itself for an external one. Empty when the caret is not on a link.
        ///
        /// `text` is the editor's live text, not the stored one: a link that has not been saved yet must
        /// still be clickable.
        #[qinvokable]
        fn link_at(self: Pin<&mut Self>, text: &QString, position: i32) -> QString;

        /// Whether `position` in `text` sits inside a code fence, which is what makes Enter add a line
        /// to the block instead of starting a new one.
        #[qinvokable]
        fn in_fence(self: Pin<&mut Self>, text: &QString, position: i32) -> bool;

        /// Where a caret in the *rendered* text of `text` belongs in the raw text: a block at rest shows
        /// the marks removed, so a click there has to be translated back before the editor takes it.
        #[qinvokable]
        fn raw_position(self: Pin<&mut Self>, text: &QString, shown: i32) -> i32;

        /// Run the quick switcher's query: pages by name, then blocks by their text. The answer arrives
        /// in `search_results` with `search_revision` bumped, rather than as a return value, because the
        /// store lives on the worker thread.
        #[qinvokable]
        fn search(self: Pin<&mut Self>, query: &QString);

        /// Show the place a search hit names and put the caret in that block. `journal_date` is empty for
        /// a block on a named page; `page_name` is the page to open otherwise.
        #[qinvokable]
        fn open_block(
            self: Pin<&mut Self>,
            block_id: i32,
            page_name: &QString,
            journal_date: &QString,
        );

        /// Consumed once the view has moved the caret to `focus_row`.
        #[qinvokable]
        fn clear_focus_row(self: Pin<&mut Self>);

        /// Switch the window between light and dark, and remember it.
        #[qinvokable]
        fn apply_theme(self: Pin<&mut Self>, name: &QString);

        /// Show every match for `query`, in the column area — what the preview's "all results" row
        /// opens.
        #[qinvokable]
        fn open_search(self: Pin<&mut Self>, query: &QString);

        /// Back to the column from anywhere.
        #[qinvokable]
        fn back(self: Pin<&mut Self>);

        /// Star or unstar a page: what the page header's star does, and what fills the
        /// sidebar's Favourites.
        #[qinvokable]
        fn toggle_favourite(self: Pin<&mut Self>, name: &QString, favourite: bool);

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

/// What a column row is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Heading,
    Block,
    Placeholder,
    AgendaHeader,
    AgendaRow,
    PageHeader,
    ReferenceHeader,
    ReferenceRow,
    UnlinkedHeader,
    UnlinkedRow,
    /// A page whose name matched a query, in the full results view.
    SearchPage,
}

/// One row of the column: a date heading, a block under it, or a row of the agenda.
#[derive(Clone)]
struct Row {
    kind: Kind,
    date: String,
    block_id: i32,
    depth: u32,
    text: String,
    marker: Option<Marker>,
    scheduled: Option<String>,
    source: String,
    overdue: bool,
    /// A read-only row's text, rendered from the same scan the editor styles with.
    html: String,
    /// A block's drawn diagram, as a `file://` URL.
    diagram: String,
    diagram_error: String,
}

impl Row {
    fn is_editable_block(&self) -> bool {
        self.kind == Kind::Block || self.kind == Kind::Placeholder
    }
}

/// What the column is showing.
#[derive(Clone, Default, PartialEq)]
enum Mode {
    #[default]
    Journal,
    Page(String),
    /// The full results of a query: every match, in the column area.
    Search(String),
}

#[derive(Default)]
pub struct JournalRust {
    rows: Vec<Row>,
    mode: Mode,
    dates: Vec<String>,
    worker: Option<Sender<Job>>,
    error: QString,
    ready: bool,
    can_undo: bool,
    can_redo: bool,
    /// Bumped on every applied snapshot. QML restores the caret from this: the row count alone does not
    /// change when a block is merely edited, so `countChanged` cannot be the trigger.
    revision: u64,
    /// The last search answer, as `{"error": "…", "hits": […]}`. Its own signal is what tells the view
    /// to rebuild, so no counter is needed.
    hits: QString,
    /// "light" or "dark", from the store's settings.
    theme: QString,
    /// Which view the rows describe: "journal", "page" or "search".
    view: QString,
    /// How many journal days are materialised in the column, empty ones included.
    loaded_days: i32,
    /// The sidebar as `{"favourites": […], "recent": […]}`, refreshed with every snapshot.
    sidebar: QString,
    /// Whether the page in view is starred, for the star in its header.
    page_favourite: bool,
    /// A block a search asked for, to be turned into a row once a snapshot containing it arrives.
    pending_focus: i32,
    /// The row the view should put the caret in, or -1.
    focus_row: i32,
}

/// Where a block that does not exist yet has to be created: a journal day, or a named page.
///
/// The placeholder row on a fresh page carries the page's *name* in its `date` field, so treating every
/// placeholder as a day tried to create a journal page called "My Project" and failed.
#[derive(Clone, Debug, PartialEq)]
enum Destination {
    Day(String),
    Page(String),
}

fn destination(mode: &Mode, row: &Row) -> Destination {
    match mode {
        Mode::Journal | Mode::Search(_) => Destination::Day(row.date.clone()),
        Mode::Page(name) => Destination::Page(name.clone()),
    }
}

enum Command {
    Update { block_id: i32, text: String },
    Materialize { destination: Destination, text: String },
    Submit { destination: Destination, block_id: i32, text: String, offset: usize },
    /// Create a page if it does not exist and record the visit — what opening one does, whether the
    /// name came from a link or the sidebar.
    OpenPage { name: String },
    SetTheme { name: String },
    SetFavourite { name: String, favourite: bool },
    Indent { block_id: i32 },
    Outdent { block_id: i32 },
    Delete { block_id: i32 },
    CycleMarker { block_id: i32 },
    SetMarker { block_id: i32, marker: Option<Marker> },
    /// A slash command's marker with the text it rewrote, so one undo takes back both halves.
    SetMarkerWithContent { block_id: i32, marker: Option<Marker>, text: String },
    SetScheduled { block_id: i32, date: Option<String> },
    SetScheduledWithContent { block_id: i32, date: Option<String>, text: String },
    Undo,
    Redo,
}

/// The marker a slash command or a checkbox names, or `None` for "no marker".
fn parse_marker(raw: &str) -> Option<Marker> {
    match raw {
        "TODO" => Some(Marker::Todo),
        "DOING" => Some(Marker::Doing),
        "DONE" => Some(Marker::Done),
        _ => None,
    }
}

enum Job {
    /// The quick switcher's query: no snapshot, just an answer.
    Search { query: String, queue: CxxQtThread<Journal> },
    Start {
        dates: Vec<String>,
        mode: Mode,
        queue: CxxQtThread<Journal>,
    },
    /// Run an optional command, then hand back a fresh snapshot of `dates`.
    Run {
        command: Option<Command>,
        dates: Vec<String>,
        mode: Mode,
        queue: CxxQtThread<Journal>,
    },
    /// Run a command and signal completion, so the caller can wait for the write to land.
    Flush {
        command: Command,
        done: Sender<()>,
    },
}

/// The page in view, when the column is showing one instead of the journal.
struct PageView {
    name: String,
    /// Set when this page *is* a journal day — a date is a name a block can link to. The header renders
    /// it as a day heading, and the star is hidden: journal days never appear in the sidebar.
    journal_date: Option<String>,
    blocks: Vec<BlockRow>,
    references: Vec<ReferenceRow>,
}

/// What the worker sends back to the GUI thread.
struct Outcome {
    error: Option<String>,
    days: Vec<(String, Vec<BlockRow>)>,
    agenda: Vec<AgendaRow>,
    /// Diagrams drawn for blocks on the worker thread, by block id.
    diagrams: HashMap<i64, Drawn>,
    /// Set when the column is showing a named page: what it is called, and what it holds.
    page: Option<PageView>,
    /// The sidebar as `{"favourites": […], "recent": […]}`.
    sidebar: String,
    /// "light" or "dark", from the store's settings.
    theme: String,
    /// Which view these rows describe: "journal", "page" or "search".
    view: &'static str,
    /// The rows of the results view, which are built where the query is answered.
    search_rows: Vec<Row>,
    /// Whether the page in view is starred.
    page_favourite: bool,

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
        if row.is_editable_block() || row.kind == Kind::AgendaRow {
            Some(row.block_id)
        } else {
            None
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
                let kind = match row.kind {
                    Kind::Heading => "heading",
                    Kind::Block => "block",
                    Kind::Placeholder => "placeholder",
                    Kind::AgendaHeader => "agenda-header",
                    Kind::AgendaRow => "agenda-row",
                    Kind::PageHeader => "page-header",
                    Kind::ReferenceHeader => "reference-header",
                    Kind::ReferenceRow => "reference-row",
                    Kind::UnlinkedHeader => "unlinked-header",
                    Kind::UnlinkedRow => "unlinked-row",
                    Kind::SearchPage => "search-page",
                };
                QVariant::from(&QString::from(kind))
            }
            Role::Text => QVariant::from(&QString::from(row.text.as_str())),
            Role::Depth => QVariant::from(&(row.depth as i32)),
            Role::Date => QVariant::from(&QString::from(row.date.as_str())),
            Role::BlockId => QVariant::from(&row.block_id),
            Role::Placeholder => QVariant::from(&(row.kind == Kind::Placeholder)),
            Role::Marker => QVariant::from(&QString::from(
                row.marker.map(Marker::label).unwrap_or(""),
            )),
            Role::Scheduled => QVariant::from(&QString::from(
                row.scheduled.as_deref().unwrap_or(""),
            )),
            Role::Source => QVariant::from(&QString::from(row.source.as_str())),
            Role::Overdue => QVariant::from(&row.overdue),
            Role::Html => QVariant::from(&QString::from(row.html.as_str())),
            Role::Diagram => QVariant::from(&QString::from(row.diagram.as_str())),
            Role::DiagramError => QVariant::from(&QString::from(row.diagram_error.as_str())),
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
        hash.insert(Role::Marker.repr, QByteArray::from("marker"));
        hash.insert(Role::Scheduled.repr, QByteArray::from("scheduled"));
        hash.insert(Role::Source.repr, QByteArray::from("source"));
        hash.insert(Role::Overdue.repr, QByteArray::from("overdue"));
        hash.insert(Role::Html.repr, QByteArray::from("html"));
        hash.insert(Role::Diagram.repr, QByteArray::from("diagram"));
        hash.insert(Role::DiagramError.repr, QByteArray::from("diagramError"));
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
        let mode = self.mode.clone();
        self.send(Job::Start { dates, mode, queue });
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
        let mode = self.mode.clone();
        self.send(Job::Run {
            command: None,
            dates,
            mode,
            queue,
        });
    }

    fn edit(mut self: Pin<&mut Self>, row: i32, text: &QString) {
        let Some(target) = self.rows.get(row as usize).cloned() else {
            return;
        };
        let command = if target.kind == Kind::Placeholder {
            Command::Materialize {
                destination: destination(&self.mode, &target),
                text: text.to_string(),
            }
        } else if !target.is_editable_block() {
            return;
        } else {
            Command::Update {
                block_id: target.block_id,
                text: text.to_string(),
            }
        };
        self.as_mut().dispatch(Some(command));
    }

    fn save_now(self: Pin<&mut Self>, row: i32, text: &QString) {
        let Some(target) = self.rows.get(row as usize).cloned() else {
            return;
        };
        if !target.is_editable_block() {
            return;
        }
        let command = if target.kind == Kind::Placeholder {
            Command::Materialize {
                destination: destination(&self.mode, &target),
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
        if !target.is_editable_block() {
            return;
        }
        let command = Command::Submit {
            destination: destination(&self.mode, &target),
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

    fn cycle_marker(mut self: Pin<&mut Self>, row: i32) {
        if let Some(block_id) = self.block_at(row) {
            self.as_mut().dispatch(Some(Command::CycleMarker { block_id }));
        }
    }

    fn set_marker(mut self: Pin<&mut Self>, row: i32, marker: &QString) {
        if let Some(block_id) = self.block_at(row) {
            let marker = parse_marker(&marker.to_string());
            self.as_mut()
                .dispatch(Some(Command::SetMarker { block_id, marker }));
        }
    }

    fn set_marker_with_content(
        mut self: Pin<&mut Self>,
        row: i32,
        marker: &QString,
        text: &QString,
    ) {
        if let Some(block_id) = self.block_at(row) {
            let marker = parse_marker(&marker.to_string());
            let text = text.to_string();
            self.as_mut().dispatch(Some(Command::SetMarkerWithContent {
                block_id,
                marker,
                text,
            }));
        }
    }

    fn set_scheduled_with_content(
        mut self: Pin<&mut Self>,
        row: i32,
        date: &QString,
        text: &QString,
    ) {
        if let Some(block_id) = self.block_at(row) {
            let date = date.to_string();
            let text = text.to_string();
            let date = if date.is_empty() { None } else { Some(date) };
            self.as_mut().dispatch(Some(Command::SetScheduledWithContent {
                block_id,
                date,
                text,
            }));
        }
    }

    fn set_scheduled(mut self: Pin<&mut Self>, row: i32, date: &QString) {
        if let Some(block_id) = self.block_at(row) {
            let date = date.to_string();
            let date = if date.is_empty() { None } else { Some(date) };
            self.as_mut()
                .dispatch(Some(Command::SetScheduled { block_id, date }));
        }
    }

    fn show_journal(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().mode = Mode::Journal;
        self.as_mut().dispatch(None);
    }

    fn open_page(mut self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        if name.trim().is_empty() {
            return;
        }
        self.as_mut().rust_mut().mode = Mode::Page(name.clone());
        // Opening a page from a link can name one that was never written: create it, so the click lands
        // on a page you can type in rather than on an empty view that refuses a keystroke.
        self.as_mut().dispatch(Some(Command::OpenPage { name }));
    }

    fn open_search(mut self: Pin<&mut Self>, query: &QString) {
        let query = query.to_string().trim().to_string();
        if query.is_empty() {
            return;
        }
        self.as_mut().rust_mut().mode = Mode::Search(query);
        self.as_mut().dispatch(None);
    }

    fn back(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().mode = Mode::Journal;
        self.as_mut().dispatch(None);
    }

    fn in_fence(self: Pin<&mut Self>, text: &QString, position: i32) -> bool {
        let text = text.to_string();
        markdown::in_fence(&text, byte_offset(&text, position.max(0) as u32))
    }

    fn raw_position(self: Pin<&mut Self>, text: &QString, shown: i32) -> i32 {
        let text = text.to_string();
        markdown::raw_position(&text, shown.max(0) as usize) as i32
    }

    fn link_at(self: Pin<&mut Self>, text: &QString, position: i32) -> QString {
        let text = text.to_string();
        let Some(link) = markdown::link_at(&text, byte_offset(&text, position.max(0) as u32)) else {
            return QString::from("");
        };
        let target = match link.kind {
            LinkKind::Url => link.target,
            LinkKind::Page | LinkKind::Tag => format!("page:{}", link.target),
        };
        QString::from(target.as_str())
    }

    fn search(mut self: Pin<&mut Self>, query: &QString) {
        if self.worker.is_none() {
            return;
        }
        let queue = self.as_mut().qt_thread();
        self.send(Job::Search {
            query: query.to_string(),
            queue,
        });
    }

    fn open_block(
        mut self: Pin<&mut Self>,
        block_id: i32,
        page_name: &QString,
        journal_date: &QString,
    ) {
        let date = journal_date.to_string();
        let name = page_name.to_string();
        if !date.is_empty() {
            // A journalled block: load the chunk that ends on its day, as if the column had scrolled there.
            let mut rust = self.as_mut().rust_mut();
            rust.mode = Mode::Journal;
            rust.dates = chunk_from(&date, CHUNK_DAYS);
        } else if !name.trim().is_empty() {
            self.as_mut().rust_mut().mode = Mode::Page(name);
        } else {
            return;
        }
        self.as_mut().rust_mut().pending_focus = block_id;
        self.as_mut().dispatch(None);
    }

    fn clear_focus_row(mut self: Pin<&mut Self>) {
        self.as_mut().set_focus_row(-1);
    }

    fn apply_theme(mut self: Pin<&mut Self>, name: &QString) {
        let name = if name.to_string() == "dark" { "dark" } else { "light" };
        // Show it at once; the write only has to reach the store before the next launch.
        self.as_mut().set_theme(QString::from(name));

        let dates = self.dates.clone();
        if dates.is_empty() || self.worker.is_none() {
            return;
        }
        let queue = self.as_mut().qt_thread();
        let mode = self.mode.clone();
        self.send(Job::Run {
            command: Some(Command::SetTheme {
                name: name.to_string(),
            }),
            dates,
            mode,
            queue,
        });
    }

    fn toggle_favourite(mut self: Pin<&mut Self>, name: &QString, favourite: bool) {
        let name = name.to_string();
        if name.trim().is_empty() {
            return;
        }
        self.as_mut()
            .dispatch(Some(Command::SetFavourite { name, favourite }));
    }

    /// Called on the GUI thread when a search answer arrives.
    fn apply_search(mut self: Pin<&mut Self>, json: String) {
        self.as_mut().set_hits(QString::from(json.as_str()));
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
        let mode = self.mode.clone();
        self.send(Job::Run {
            command,
            dates,
            mode,
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
        let pending = self.pending_focus;
        let focus;
        {
            let agenda = agenda_rows(&outcome.agenda, &outcome.theme);
            let mut rows = Vec::new();
            if outcome.view == "search" {
                rows.extend(outcome.search_rows.iter().cloned());
            }
            if let Some(page) = &outcome.page {
                rows.push(page_header(&page.name, page.journal_date.as_deref()));
                rows.extend(
                    day_rows(&page.name, &page.blocks, &outcome.diagrams, &outcome.theme)
                        .into_iter()
                        .skip(1),
                );
                rows.extend(reference_rows(&page.references, &outcome.theme));
                focus = row_of_block(&rows, pending);
                let mut rust = self.as_mut().rust_mut();
                rust.pending_focus = -1;
                rust.rows = rows;
                rust.can_undo = outcome.can_undo;
                rust.can_redo = outcome.can_redo;
                drop(rust);
                self.as_mut().end_reset_model();
                self.as_mut().set_error(QString::from(""));
                self.as_mut().set_ready(true);
                self.as_mut().set_can_undo(outcome.can_undo);
                self.as_mut().set_can_redo(outcome.can_redo);
                self.as_mut().set_sidebar(QString::from(outcome.sidebar.as_str()));
                self.as_mut().set_loaded_days(outcome.days.len() as i32);
                self.as_mut().set_theme(QString::from(outcome.theme.as_str()));
                self.as_mut().set_view(QString::from(outcome.view));
                self.as_mut().set_page_favourite(outcome.page_favourite);
                self.as_mut().set_focus_row(focus);
                self.as_mut().bump_revision();
                return;
            }
            for (index, (date, blocks)) in outcome.days.iter().enumerate() {
                if !shows_day(index, blocks) {
                    continue;
                }
                rows.extend(day_rows(date, blocks, &outcome.diagrams, &outcome.theme));
                if index == 0 && !agenda.is_empty() {
                    // "Right under today's blocks": today is the newest day in the column.
                    rows.extend(agenda.iter().cloned());
                }
            }
            if rows.is_empty() {
                rows.extend(agenda.iter().cloned());
            }

            focus = row_of_block(&rows, pending);
            let mut rust = self.as_mut().rust_mut();
            rust.pending_focus = -1;
            rust.rows = rows;
            rust.can_undo = outcome.can_undo;
            rust.can_redo = outcome.can_redo;
        }
        self.as_mut().end_reset_model();

        self.as_mut().set_error(QString::from(""));
        self.as_mut().set_ready(outcome.ready);
        self.as_mut().set_can_undo(outcome.can_undo);
        self.as_mut().set_can_redo(outcome.can_redo);
        self.as_mut().set_sidebar(QString::from(outcome.sidebar.as_str()));
        self.as_mut().set_loaded_days(outcome.days.len() as i32);
        self.as_mut().set_theme(QString::from(outcome.theme.as_str()));
        self.as_mut().set_view(QString::from(outcome.view));
        self.as_mut().set_page_favourite(outcome.page_favourite);
        self.as_mut().set_focus_row(focus);
        self.as_mut().bump_revision();
    }

    fn bump_revision(mut self: Pin<&mut Self>) {
        let next = self.revision.wrapping_add(1);
        self.as_mut().set_revision(next);
    }
}

fn worker_loop(rx: mpsc::Receiver<Job>) {
    let mut store: Option<Store> = None;
    // Diagrams are drawn here, off the GUI thread, and remembered by content hash.
    let mut diagrams = Diagrams::default();

    while let Ok(job) = rx.recv() {
        match job {
            Job::Start { dates, mode, queue } => {
                let opened = match DATABASE_PATH.get() {
                    // The storage error already names the file, so it is not appended again.
                    Some(path) => Store::open(path).map_err(|error| error.to_string()),
                    None => Err("no database path was configured".to_string()),
                };
                match opened {
                    Ok(opened) => {
                        // A copy before any typing, at most once a day. Failing to write one must not stop
                        // the notes from opening.
                        if let Some(path) = DATABASE_PATH.get() {
                            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                            let _ = crate::backup::daily(&opened, path, &today);
                        }
                        let theme = theme_name(&opened);
                        let outcome = snapshot(&opened, &dates, &mode, &mut diagrams, &theme);
                        store = Some(opened);
                        deliver(queue, outcome);
                    }
                    Err(error) => deliver(
                        queue,
                        Outcome {
                            error: Some(error),
                            days: Vec::new(),
                            agenda: Vec::new(),
                            diagrams: HashMap::new(),
                            page: None,
                            sidebar: "{\"favourites\":[],\"recent\":[]}".to_string(),
                            theme: "light".to_string(),
                            view: "journal",
                            search_rows: Vec::new(),
                            page_favourite: false,
                            ready: false,
                            can_undo: false,
                            can_redo: false,
                        },
                    ),
                }
            }
            Job::Search { query, queue } => {
                let json = match store.as_ref() {
                    Some(store) => match store.search(&query, SEARCH_PAGES, SEARCH_BLOCKS) {
                        Ok(results) => search_json(&results, ink(&theme_name(store))),
                        Err(error) => search_failed(&error.to_string()),
                    },
                    None => search_failed("the database is not open"),
                };
                let _ = queue.queue(move |journal: Pin<&mut Journal>| journal.apply_search(json));
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
                mode,
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
                    Ok(()) => {
                        let theme = theme_name(store);
                        snapshot(store, &dates, &mode, &mut diagrams, &theme)
                    }
                    Err(error) => Outcome {
                        error: Some(error),
                        days: Vec::new(),
                        agenda: Vec::new(),
                        diagrams: HashMap::new(),
                        page: None,
                        sidebar: sidebar_json(store),
                        theme: theme_name(store),
                        view: "journal",
                        search_rows: Vec::new(),
                        page_favourite: false,
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
        Command::OpenPage { name } => {
            // Opening a page from a link can name one that was never written: create it, so the
            // click lands on a page you can type in, and record the visit for the sidebar.
            let page = store.ensure_page(&name).map_err(fail)?;
            store.record_visit(page).map_err(fail)?;
            Ok(())
        }
        Command::SetTheme { name } => store.set_setting("theme", &name).map_err(fail),
        Command::SetFavourite { name, favourite } => {
            let page = store.ensure_page(&name).map_err(fail)?;
            store.set_favourite(page, favourite).map_err(fail)
        }
        Command::Materialize {
            destination,
            text,
        } => {
            let page = page_for(store, &destination)?;
            store.insert_block(page, None, &text).map_err(fail)?;
            Ok(())
        }
        Command::Submit {
            destination,
            block_id,
            text,
            offset,
        } => {
            let page = page_for(store, &destination)?;
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
        Command::CycleMarker { block_id } => store.cycle_marker(block_id as i64).map_err(fail),
        Command::SetMarker { block_id, marker } => store
            .set_marker(block_id as i64, marker)
            .map_err(fail),
        Command::SetMarkerWithContent {
            block_id,
            marker,
            text,
        } => store
            .set_marker_with_content(block_id as i64, marker, &text)
            .map_err(fail),
        Command::SetScheduled { block_id, date } => store
            .set_scheduled(block_id as i64, date.as_deref())
            .map_err(fail),
        Command::SetScheduledWithContent {
            block_id,
            date,
            text,
        } => store
            .set_scheduled_with_content(block_id as i64, date.as_deref(), &text)
            .map_err(fail),
        Command::Undo => store.undo().map(|_| ()).map_err(fail),
        Command::Redo => store.redo().map(|_| ()).map_err(fail),
    }
}

/// The full results of a query, as rows: a heading, then the pages whose name matched and the blocks
/// whose text did. Block rows carry the block and where it lives, so their "from …" label can go there.
fn search_outcome(store: &Store, query: &str) -> Outcome {
    let colours = ink(&theme_name(store));
    let found = store.search(query, ALL_PAGES, ALL_BLOCKS).unwrap_or_default();
    let mut rows = vec![section_row(
        Kind::PageHeader,
        &format!("All results ({}) · {query}", found.total),
    )];

    if !found.pages.is_empty() {
        rows.push(section_row(Kind::ReferenceHeader, "Pages"));
        for page in &found.pages {
            rows.push(Row {
                kind: Kind::SearchPage,
                date: String::new(),
                block_id: -1,
                depth: 0,
                html: markdown::to_html(&page.name, colours),
                text: page.name.clone(),
                marker: None,
                scheduled: None,
                source: String::new(),
                overdue: false,
                diagram: String::new(),
                diagram_error: String::new(),
            });
        }
    }

    if !found.blocks.is_empty() {
        rows.push(section_row(Kind::ReferenceHeader, "Blocks"));
        for hit in &found.blocks {
            let entry = ReferenceRow {
                block: hit.block.clone(),
                page_name: hit.page_name.clone(),
                journal_date: hit.journal_date.clone(),
                kind: ReferenceKind::Mention,
            };
            let mut row = reference_row(&entry, Kind::ReferenceRow, &theme_name(store));
            row.html = highlighted(&hit.snippet, hit.matched, colours);
            rows.push(row);
        }
    }

    Outcome {
        error: None,
        days: Vec::new(),
        agenda: Vec::new(),
        diagrams: HashMap::new(),
        page: None,
        sidebar: sidebar_json(store),
        theme: theme_name(store),
        view: "search",
        search_rows: rows,
        page_favourite: false,
        ready: true,
        can_undo: store.can_undo(),
        can_redo: store.can_redo(),
    }
}

/// A block snippet rendered like any other row, with the words that matched picked out behind them.
fn highlighted(snippet: &str, matched: Option<(usize, usize)>, colours: &markdown::Ink) -> String {
    match matched {
        Some(range) => markdown::to_html_marked(snippet, range, colours),
        None => markdown::to_html(snippet, colours),
    }
}

/// The page a block should be created on, making it — and its day, or its name — exist if it does not.
fn page_for(store: &mut Store, destination: &Destination) -> Result<i64, String> {
    let fail = |error: lognotes_core::Error| error.to_string();
    match destination {
        Destination::Day(date) => store.ensure_journal_page(date).map_err(fail),
        Destination::Page(name) => store.ensure_page(name).map_err(fail),
    }
}

fn snapshot(
    store: &Store,
    dates: &[String],
    mode: &Mode,
    diagrams: &mut Diagrams,
    theme: &str,
) -> Outcome {
    if let Mode::Search(query) = mode {
        return search_outcome(store, query);
    }
    if let Mode::Page(name) = mode {
        let page = store.page_by_name(name).ok().flatten();
        let (blocks, references) = match &page {
            Some(page) => (
                store.page_blocks(page.id).unwrap_or_default(),
                store.references(page.id).unwrap_or_default(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let journal_date = page.as_ref().and_then(|page| page.journal_date.clone());
        let drawn = drawn_for(&blocks, diagrams, theme);
        let view = PageView {
            name: name.clone(),
            journal_date,
            blocks,
            references,
        };
        return Outcome {
            error: None,
            days: Vec::new(),
            agenda: Vec::new(),
            diagrams: drawn,
            view: "page",
            search_rows: Vec::new(),
            // A day is never starred: the sidebar lists named pages, so a star on one would lead
            // nowhere. Everything else follows the store.
            page_favourite: match (&view.journal_date, &page) {
                (Some(_), _) => false,
                (None, Some(page)) => store.is_favourite(page.id).ok().unwrap_or(false),
                (None, None) => false,
            },
            page: Some(view),
            sidebar: sidebar_json(store),
            theme: theme.to_string(),
            ready: true,
            can_undo: store.can_undo(),
            can_redo: store.can_redo(),
        };
    }

    let days: Vec<(String, Vec<BlockRow>)> = dates
        .iter()
        .map(|date| {
            let blocks = store.day(date).map(|day| day.blocks).unwrap_or_default();
            (date.clone(), blocks)
        })
        .collect();

    // The agenda is anchored to the newest day in the column, which is today's page.
    let agenda = dates
        .first()
        .and_then(|today| store.agenda(today, AGENDA_HORIZON_DAYS).ok())
        .unwrap_or_default();

    let drawn = days
        .iter()
        .flat_map(|(_, blocks)| drawn_for(blocks, diagrams, theme))
        .collect();

    Outcome {
        error: None,
        days,
        agenda,
        diagrams: drawn,
        page: None,
        sidebar: sidebar_json(store),
        theme: theme.to_string(),
        view: "journal",
        search_rows: Vec::new(),
        page_favourite: false,
        ready: true,
        can_undo: store.can_undo(),
        can_redo: store.can_redo(),
    }
}

/// Draw the diagrams of every block that has a mermaid fence. Content-hashed, so a snapshot that changes
/// one block elsewhere in the column redraws nothing.
fn drawn_for(
    blocks: &[BlockRow],
    diagrams: &mut Diagrams,
    theme: &str,
) -> HashMap<i64, Drawn> {
    let mut drawn = HashMap::new();
    for block in blocks {
        if let Some(diagram) = diagrams.for_text(&block.content, theme) {
            drawn.insert(block.id, diagram);
        }
    }
    drawn
}

fn heading(date: &str) -> Row {
    Row {
        kind: Kind::Heading,
        date: date.to_string(),
        block_id: -1,
        depth: 0,
        text: String::new(),
        marker: None,
        scheduled: None,
        source: String::new(),
        overdue: false,
        html: String::new(),
        diagram: String::new(),
        diagram_error: String::new(),
    }
}

/// Whether a day at this position in the column is worth a heading. The newest day always is — that is
/// where the cursor goes — and every day after it only if someone wrote in it: the journal is a record
/// of what was written, not of the calendar.
fn shows_day(index: usize, blocks: &[BlockRow]) -> bool {
    index == 0 || !blocks.is_empty()
}

fn day_rows(
    date: &str,
    blocks: &[BlockRow],
    drawn: &HashMap<i64, Drawn>,
    theme: &str,
) -> Vec<Row> {
    let mut rows = vec![heading(date)];

    if blocks.is_empty() {
        // The empty block the cursor lands in. It is never persisted: the first keystroke
        // materialises it, so scrolling a year back cannot fill the database with empty days.
        rows.push(Row {
            kind: Kind::Placeholder,
            marker: None,
            scheduled: None,
            source: String::new(),
            overdue: false,
            ..heading(date)
        });
        return rows;
    }

    rows.extend(blocks.iter().map(|block| {
        let diagram = drawn.get(&block.id).cloned().unwrap_or_default();
        Row {
            kind: Kind::Block,
            date: date.to_string(),
            block_id: block.id as i32,
            depth: block.depth,
            // A block is shown as this until it is clicked, so it keeps its line breaks — a fence has to
            // still read as a block of code.
            html: markdown::to_html_block(&block.content, ink(theme)),
            text: block.content.clone(),
            marker: block.marker,
            scheduled: block.scheduled_date.clone(),
            source: String::new(),
            overdue: false,
            diagram: diagram.source,
            diagram_error: diagram.error,
        }
    }));
    rows
}

fn page_header(name: &str, journal_date: Option<&str>) -> Row {
    Row {
        kind: Kind::PageHeader,
        // A day page's header reads as a day; a named page's header carries no date at all.
        date: journal_date.unwrap_or_default().to_string(),
        block_id: -1,
        depth: 0,
        text: name.to_string(),
        marker: None,
        scheduled: None,
        source: String::new(),
        overdue: false,
        html: String::new(),
        diagram: String::new(),
        diagram_error: String::new(),
    }
}

/// The reference panels: explicit links first, then a collapsed group of plain mentions.
fn reference_rows(references: &[ReferenceRow], theme: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let linked: Vec<&ReferenceRow> = references
        .iter()
        .filter(|entry| entry.kind == ReferenceKind::Linked)
        .collect();
    let mentions: Vec<&ReferenceRow> = references
        .iter()
        .filter(|entry| entry.kind == ReferenceKind::Mention)
        .collect();

    if !linked.is_empty() {
        rows.push(section_row(Kind::ReferenceHeader, "Linked References"));
        rows.extend(linked.into_iter().map(|entry| reference_row(entry, Kind::ReferenceRow, theme)));
    }

    if !mentions.is_empty() {
        rows.push(section_row(
            Kind::UnlinkedHeader,
            &format!("Unlinked mentions ({})", mentions.len()),
        ));
        rows.extend(
            mentions
                .into_iter()
                .map(|entry| reference_row(entry, Kind::UnlinkedRow, theme)),
        );
    }

    rows
}

fn section_row(kind: Kind, text: &str) -> Row {
    Row {
        kind,
        date: String::new(),
        block_id: -1,
        depth: 0,
        text: text.to_string(),
        marker: None,
        scheduled: None,
        source: String::new(),
        overdue: false,
        html: String::new(),
        diagram: String::new(),
        diagram_error: String::new(),
    }
}

fn reference_row(entry: &ReferenceRow, kind: Kind, theme: &str) -> Row {
    Row {
        kind,
        date: String::new(),
        block_id: entry.block.id as i32,
        depth: 0,
        // A summary row has no editor, so its styling comes ready-made as HTML.
        html: markdown::to_html(&entry.block.content, ink(theme)),
        text: entry.block.content.clone(),
        marker: entry.block.marker,
        scheduled: None,
        source: entry
            .journal_date
            .clone()
            .unwrap_or_else(|| entry.page_name.clone()),
        overdue: false,
        diagram: String::new(),
        diagram_error: String::new(),
    }
}

/// The row holding `block_id`, or -1 when this snapshot does not contain it.
fn row_of_block(rows: &[Row], block_id: i32) -> i32 {
    if block_id < 0 {
        return -1;
    }
    rows.iter()
        .position(|row| row.block_id == block_id)
        .map(|index| index as i32)
        .unwrap_or(-1)
}

/// A search answer the view can read: `{"error": "", "hits": […]}`. A failed query says so instead of
/// looking like "nothing found".
fn search_failed(error: &str) -> String {
    format!(
        "{{\"error\":{},\"total\":0,\"hits\":[]}}",
        json_string(error)
    )
}

fn search_json(results: &SearchResults, colours: &markdown::Ink) -> String {
    let mut hits: Vec<String> = Vec::new();

    for page in &results.pages {
        hits.push(format!(
            "{{\"kind\":\"page\",\"title\":{},\"where\":\"page\",\"blockId\":-1,\"page\":{},\"date\":\"\"}}",
            json_string(&page.name),
            json_string(&page.name)
        ));
    }

    for hit in &results.blocks {
        let where_ = hit
            .journal_date
            .clone()
            .unwrap_or_else(|| hit.page_name.clone());
        hits.push(format!(
            "{{\"kind\":\"block\",\"title\":{},\"where\":{},\"blockId\":{},\"page\":{},\"date\":{}}}",
            // The block's text is rendered here as it is everywhere else it is read: marks gone.
            json_string(&markdown::to_html(&hit.snippet, colours)),
            json_string(&where_),
            hit.block.id,
            json_string(&hit.page_name),
            json_string(hit.journal_date.as_deref().unwrap_or(""))
        ));
    }

    format!(
        "{{\"error\":\"\",\"total\":{},\"hits\":[{}]}}",
        results.total,
        hits.join(",")
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn theme_name(store: &Store) -> String {
    store
        .setting("theme")
        .ok()
        .flatten()
        .filter(|name| name == "dark")
        .unwrap_or_else(|| "light".to_string())
}

/// The sidebar as the view reads it: the starred pages, then the most recently opened ones.
fn sidebar_json(store: &Store) -> String {
    let names = |pages: Vec<lognotes_core::Page>| -> String {
        let quoted: Vec<String> = pages
            .iter()
            .map(|page| json_string(&page.name))
            .collect();
        format!("[{}]", quoted.join(","))
    };

    let favourites = names(store.pages_by_favourite().unwrap_or_default());
    let recent = names(store.recent_pages(SIDEBAR_RECENT).unwrap_or_default());
    format!("{{\"favourites\":{favourites},\"recent\":{recent}}}")
}

/// The scheduled section that sits directly under today's blocks. Absent entirely when nothing is due.
fn agenda_rows(agenda: &[AgendaRow], theme: &str) -> Vec<Row> {
    if agenda.is_empty() {
        return Vec::new();
    }

    let mut rows = vec![Row {
        kind: Kind::AgendaHeader,
        date: String::new(),
        block_id: -1,
        depth: 0,
        text: "Scheduled".to_string(),
        marker: None,
        scheduled: None,
        source: String::new(),
        overdue: false,
        html: String::new(),
        diagram: String::new(),
        diagram_error: String::new(),
    }];

    rows.extend(agenda.iter().map(|entry| Row {
        kind: Kind::AgendaRow,
        date: entry.block.scheduled_date.clone().unwrap_or_default(),
        block_id: entry.block.id as i32,
        depth: 0,
        html: markdown::to_html(&entry.block.content, ink(theme)),
        text: entry.block.content.clone(),
        marker: entry.block.marker,
        scheduled: entry.block.scheduled_date.clone(),
        source: entry
            .journal_date
            .clone()
            .unwrap_or_else(|| entry.page_name.clone()),
        overdue: entry.overdue,
        diagram: String::new(),
        diagram_error: String::new(),
    }));
    rows
}

fn today_chunk(count: u32) -> Vec<String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    chunk_from(&today, count)
}

/// A QML caret position counts UTF-16 units; the markdown scanner counts bytes.
fn byte_offset(text: &str, position: u32) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 >= position {
            return byte;
        }
        utf16 += character.len_utf16() as u32;
    }
    text.len()
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
        let rows = day_rows("2026-04-13", &[], &HashMap::new(), "light");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, Kind::Heading);
        assert_eq!(rows[0].date, "2026-04-13");
        assert_eq!(rows[1].kind, Kind::Placeholder);
        assert_eq!(rows[1].block_id, -1, "the placeholder is never a real row");
    }

    #[test]
    fn a_day_renders_its_blocks_under_the_heading_with_depth() {
        let rows = day_rows(
            "2026-04-13",
            &[block(7, 0, "one"), block(8, 1, "two")],
            &HashMap::new(),
            "light",
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, Kind::Heading);
        assert_eq!((rows[1].block_id, rows[1].depth), (7, 0));
        assert_eq!((rows[2].block_id, rows[2].depth), (8, 1));
        assert!(rows[1..].iter().all(|row| row.kind == Kind::Block));
    }

    #[test]
    fn a_past_day_with_nothing_in_it_is_not_shown() {
        assert!(shows_day(0, &[]), "today is shown even when it is empty");
        assert!(!shows_day(1, &[]), "yesterday is not, when nothing was written");
        assert!(shows_day(1, &[block(1, 0, "something")]));
    }

    #[test]
    fn an_agenda_renders_a_section_heading_and_its_rows() {
        let agenda = vec![lognotes_core::AgendaRow {
            block: block(9, 0, "pay the bill"),
            page_name: "2026-04-11".to_string(),
            journal_date: Some("2026-04-11".to_string()),
            overdue: true,
        }];

        let rows = agenda_rows(&agenda, "light");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, Kind::AgendaHeader);
        assert_eq!(rows[0].text, "Scheduled");
        assert_eq!(rows[1].kind, Kind::AgendaRow);
        assert_eq!(rows[1].block_id, 9);
        assert!(rows[1].overdue);
        assert_eq!(rows[1].source, "2026-04-11");
    }

    #[test]
    fn nothing_scheduled_means_no_section_at_all() {
        assert!(agenda_rows(&[], "light").is_empty());
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

    /// A slash command's text and its state arrive as one bridge call, so undo takes back the command as
    /// the user performed it and not one half of it.
    #[test]
    fn a_slash_command_writes_its_text_and_its_state_in_one_command() {
        let mut store = session();
        let day = store.ensure_journal_page("2026-04-13").unwrap();
        let block = store.insert_block(day, None, "order filament /todo").unwrap() as i32;

        run_command(
            &mut store,
            Command::SetMarkerWithContent {
                block_id: block,
                marker: Some(Marker::Todo),
                text: "order filament".into(),
            },
        )
        .unwrap();
        assert_eq!(blocks(&store, "2026-04-13"), ["order filament"]);
        assert_eq!(store.page_blocks(day).unwrap()[0].marker, Some(Marker::Todo));

        run_command(&mut store, Command::Undo).unwrap();
        assert_eq!(blocks(&store, "2026-04-13"), ["order filament /todo"]);
        assert_eq!(store.page_blocks(day).unwrap()[0].marker, None);

        run_command(
            &mut store,
            Command::SetScheduledWithContent {
                block_id: block,
                date: Some("2026-04-14".into()),
                text: "order filament /scheduled".into(),
            },
        )
        .unwrap();
        let row = &store.page_blocks(day).unwrap()[0];
        assert_eq!(row.content, "order filament /scheduled");
        assert_eq!(row.scheduled_date.as_deref(), Some("2026-04-14"));
    }

    /// A date is a name for a journal day, so a page view of one carries the day in the header's `date`
    /// role — that is what makes the header read as a day and hides the star.
    #[test]
    fn a_day_shown_as_a_page_carries_its_date_and_no_star() {
        let mut store = session();
        let day = store.ensure_journal_page("2026-04-13").unwrap();
        store.insert_block(day, None, "written that day").unwrap();
        store.set_favourite(day, true).unwrap();

        let mut diagrams = Diagrams::default();
        let outcome = snapshot(
            &store,
            &[],
            &Mode::Page("2026-04-13".into()),
            &mut diagrams,
            "light",
        );
        let page = outcome.page.expect("a page view");
        assert_eq!(page.journal_date.as_deref(), Some("2026-04-13"));
        assert_eq!(page.blocks.len(), 1);
        assert!(
            !outcome.page_favourite,
            "a journal day takes no star, even one the store says is starred"
        );
        let header = page_header(&page.name, page.journal_date.as_deref());
        assert_eq!(header.kind, Kind::PageHeader);
        assert_eq!(header.date, "2026-04-13");
    }

    #[test]
    fn a_named_page_has_no_date_in_its_header() {
        let mut store = session();
        store.ensure_page("My Project").unwrap();

        let mut diagrams = Diagrams::default();
        let outcome = snapshot(
            &store,
            &[],
            &Mode::Page("My Project".into()),
            &mut diagrams,
            "light",
        );
        let page = outcome.page.expect("a page view");
        assert_eq!(page.journal_date, None);
        assert_eq!(page_header(&page.name, page.journal_date.as_deref()).date, "");
    }

    #[test]
    fn the_first_keystroke_materialises_the_placeholder() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                destination: Destination::Day("2026-04-13".into()),
                text: "hello".into(),
            },
        )
        .unwrap();

        assert_eq!(blocks(&store, "2026-04-13"), ["hello"]);
    }

    #[test]
    fn opening_a_page_that_does_not_exist_creates_it() {
        let mut store = session();
        run_command(
            &mut store,
            Command::OpenPage {
                name: "Fresh Page".into(),
            },
        )
        .unwrap();

        let page = store.page_by_name("Fresh Page").unwrap().expect("created");
        assert_eq!(
            page.kind,
            lognotes_core::PageKind::Page,
            "a page name is not a date"
        );
    }

    #[test]
    fn a_page_that_was_just_created_takes_a_keystroke() {
        let mut store = session();
        run_command(
            &mut store,
            Command::OpenPage {
                name: "Fresh Page".into(),
            },
        )
        .unwrap();
        run_command(
            &mut store,
            Command::Materialize {
                destination: Destination::Page("Fresh Page".into()),
                text: "first note".into(),
            },
        )
        .unwrap();

        let page = store.page_by_name("Fresh Page").unwrap().expect("created");
        let blocks = store.page_blocks(page.id).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "first note");
    }

    #[test]
    fn enter_mid_block_splits_and_keeps_both_halves() {
        let mut store = session();
        run_command(
            &mut store,
            Command::Materialize {
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
                destination: Destination::Day("2026-04-13".into()),
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
