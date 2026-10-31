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
