//! The rows the store reads and writes.

/// A page is a named node; a journal page is a page pinned to a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Page,
    Journal,
}

impl PageKind {
    pub(crate) fn parse(raw: &str) -> PageKind {
        match raw {
            "journal" => PageKind::Journal,
            _ => PageKind::Page,
        }
    }
}

/// Task markers. Nothing in this build sets one yet; the column exists so the next capability does not
/// need a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Todo,
    Doing,
    Done,
}

impl Marker {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Marker::Todo => "TODO",
            Marker::Doing => "DOING",
            Marker::Done => "DONE",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Marker> {
        match raw {
            "TODO" => Some(Marker::Todo),
            "DOING" => Some(Marker::Doing),
            "DONE" => Some(Marker::Done),
            _ => None,
        }
    }

    /// The cycle the user gets from Ctrl+Enter and from clicking the box: none → TODO → DOING → DONE →
    /// none. It lives here so the checkbox, the shortcut and the agenda cannot drift apart.
    pub fn next(self) -> Option<Marker> {
        match self {
            Marker::Todo => Some(Marker::Doing),
            Marker::Doing => Some(Marker::Done),
            Marker::Done => None,
        }
    }

    /// The marker a block with none gets on the first press.
    pub fn first() -> Marker {
        Marker::Todo
    }

    pub fn label(self) -> &'static str {
        self.as_str()
    }
}

/// How a block was found to reference a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// An explicit `[[link]]` or `#tag`.
    Linked,
    /// The page's name appearing as plain words.
    Mention,
}

/// One block that references a page, with where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceRow {
    pub block: BlockRow,
    pub page_name: String,
    pub journal_date: Option<String>,
    pub kind: ReferenceKind,
}

/// One row of the agenda: the block that is due, where it lives, and whether its date has passed.
#[derive(Debug, Clone, PartialEq)]
pub struct AgendaRow {
    pub block: BlockRow,
    pub page_name: String,
    pub journal_date: Option<String>,
    pub overdue: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub id: i64,
    pub name: String,
    pub name_key: String,
    pub kind: PageKind,
    pub journal_date: Option<String>,
}

/// One row of a rendered page: the block plus where it sits in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub depth: u32,
    pub order_key: String,
    pub content: String,
    pub marker: Option<Marker>,
    pub scheduled_date: Option<String>,
    pub collapsed: bool,
}

/// One block a search matched, with where it lives and the window of text that matched.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockHit {
    pub block: BlockRow,
    pub page_name: String,
    pub journal_date: Option<String>,
    /// A window of the block's text around the match, as stored — marks and all. The view renders it.
    pub snippet: String,
}

/// What the quick switcher found: pages by name first, then blocks by their text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchResults {
    pub pages: Vec<Page>,
    pub blocks: Vec<BlockHit>,
}

/// A journal day as the column renders it. `page_id` is `None` when the day has never been written to —
/// reading a day must not create it, or scrolling into the past would fill the database with empty pages.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalDay {
    pub journal_date: String,
    pub page_id: Option<i64>,
    pub blocks: Vec<BlockRow>,
}

/// A block exactly as stored, used to restore a deleted subtree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockRecord {
    pub id: i64,
    pub page_id: i64,
    pub parent_id: Option<i64>,
    pub order_key: String,
    pub content: String,
    pub marker: Option<Marker>,
    pub scheduled_date: Option<String>,
    pub collapsed: bool,
    pub created_at: String,
    pub updated_at: String,
}
