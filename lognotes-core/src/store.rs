//! The SQLite store: schema, commands, queries, and the session undo history.
//!
//! Every mutation goes through `Store::exec`, which is the single point that records an inverse action.
//! Nothing else may write, so no command can forget to be undoable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Days, Local, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::export::{ImportMode, Moved, ParsedPage};
use crate::markdown;
use crate::markdown::LinkKind;
use crate::model::{
    AgendaRow, BlockHit, BlockRecord, BlockRow, JournalDay, Marker, Page, PageKind, ReferenceKind,
    ReferenceRow, Renamed, SearchResults,
};
use crate::order::key_between;
use crate::project::{
    self, BlockRef, BriefItem, ConsolidationRequest, ConsolidationResult, DigestInfo,
    ProjectContext, ProjectSource, TaskRef, TaskToCreate, TaskUpdate,
};
use crate::text::{extract_links, mentions_whole_words};

const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = r#"
CREATE TABLE page (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL,
  name_key     TEXT NOT NULL UNIQUE,
  kind         TEXT NOT NULL CHECK (kind IN ('page', 'journal')),
  journal_date TEXT,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  CHECK ((kind = 'journal') = (journal_date IS NOT NULL))
);
CREATE UNIQUE INDEX page_journal_date ON page (journal_date) WHERE journal_date IS NOT NULL;

CREATE TABLE block (
  id             INTEGER PRIMARY KEY,
  page_id        INTEGER NOT NULL REFERENCES page (id) ON DELETE CASCADE,
  parent_id      INTEGER REFERENCES block (id) ON DELETE CASCADE,
  order_key      TEXT NOT NULL,
  content        TEXT NOT NULL DEFAULT '',
  marker         TEXT CHECK (marker IN ('TODO', 'DOING', 'DONE')),
  scheduled_date TEXT,
  collapsed      INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE UNIQUE INDEX block_sibling_order ON block (page_id, IFNULL(parent_id, 0), order_key);
CREATE INDEX block_page ON block (page_id);
CREATE INDEX block_agenda ON block (scheduled_date)
  WHERE scheduled_date IS NOT NULL AND marker IS NOT NULL;

CREATE TABLE block_link (
  block_id INTEGER NOT NULL REFERENCES block (id) ON DELETE CASCADE,
  page_id  INTEGER NOT NULL REFERENCES page (id) ON DELETE CASCADE,
  kind     TEXT NOT NULL CHECK (kind IN ('link', 'tag')),
  PRIMARY KEY (block_id, page_id)
);
CREATE INDEX block_link_page ON block_link (page_id);

CREATE VIRTUAL TABLE block_fts USING fts5 (content, content = 'block', content_rowid = 'id');
"#;

/// v2: the full-text index has to follow block writes. Triggers are the mechanism FTS5 external-content
/// tables expect — keeping it current by hand means every write path has to remember.
const SCHEMA_V2: &str = r#"
CREATE TRIGGER block_fts_insert AFTER INSERT ON block BEGIN
  INSERT INTO block_fts (rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER block_fts_delete AFTER DELETE ON block BEGIN
  INSERT INTO block_fts (block_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;
CREATE TRIGGER block_fts_update AFTER UPDATE OF content ON block BEGIN
  INSERT INTO block_fts (block_fts, rowid, content) VALUES ('delete', old.id, old.content);
  INSERT INTO block_fts (rowid, content) VALUES (new.id, new.content);
END;
INSERT INTO block_fts (block_fts) VALUES ('rebuild');
"#;

/// Schema v3: a page can be starred and remembers when it was last opened, and the app keeps its own
/// settings — the theme, today — in the same file as the notes, so there is nothing to keep in step.
const SCHEMA_V3: &str = r#"
ALTER TABLE page ADD COLUMN favourite INTEGER NOT NULL DEFAULT 0;
ALTER TABLE page ADD COLUMN visit_order INTEGER;
CREATE INDEX page_recent ON page (visit_order DESC) WHERE visit_order IS NOT NULL;
CREATE INDEX page_favourite ON page (name_key) WHERE favourite = 1;
CREATE TABLE setting (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// One reversible mutation. `apply` performs an action and returns the action that undoes it, which is
/// how undo and redo stay symmetric without a second, hand-written undo path per command.
#[derive(Debug, Clone)]
enum Action {
    SetContent {
        block_id: i64,
        content: String,
    },
    Insert {
        record: BlockRecord,
    },
    Delete {
        block_id: i64,
    },
    InsertSubtree {
        records: Vec<BlockRecord>,
    },
    SetPosition {
        block_id: i64,
        page_id: i64,
        parent_id: Option<i64>,
        order_key: String,
    },
    SetMarker {
        block_id: i64,
        marker: Option<Marker>,
    },
    SetScheduled {
        block_id: i64,
        date: Option<String>,
    },
    /// Rename a page and rewrite the block texts that name it, in that order. The inverse carries the
    /// old texts so the same action can undo and redo without a second path.
    RenamePage {
        page_id: i64,
        from_name: String,
        from_key: String,
        to_name: String,
        to_key: String,
        changes: Vec<(i64, String)>,
    },
    DeletePage {
        page: PageRecord,
        blocks: Vec<BlockRecord>,
    },
    RestorePage {
        page: PageRecord,
        blocks: Vec<BlockRecord>,
    },
    Composite(Vec<Action>),
}

#[derive(Debug, Clone)]
struct PageRecord {
    id: i64,
    name: String,
    name_key: String,
    kind: PageKind,
    journal_date: Option<String>,
    favourite: bool,
    visit_order: Option<i64>,
    created_at: String,
    updated_at: String,
}

type RawBlock = (
    i64,
    Option<i64>,
    String,
    String,
    Option<String>,
    Option<String>,
    bool,
);

pub struct Store {
    conn: Connection,
    undo: Vec<Action>,
    redo: Vec<Action>,
    /// The consistent copy written before an older schema was upgraded, if one was needed.
    migration_copy: Option<PathBuf>,
}

impl Store {
    /// Open (or create) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        Store::from_connection(Connection::open(path)?, None)
    }

    /// Open (or create) the database, copying an older one into `backup_dir` before upgrading it.
    ///
    /// The copy is written with `VACUUM INTO` from the open connection, so it includes anything still
    /// in the write-ahead log. If it cannot be written, the upgrade does not run: opening without a
    /// copy is the one risk this path exists to avoid.
    pub fn open_with_migration_backup(
        path: impl AsRef<Path>,
        backup_dir: impl AsRef<Path>,
    ) -> Result<Store> {
        Store::from_connection(Connection::open(path)?, Some(backup_dir.as_ref()))
    }

    /// An ephemeral store, used by tests.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(Connection::open_in_memory()?, None)
    }

    fn from_connection(conn: Connection, migration_backup: Option<&Path>) -> Result<Store> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let migration_copy = match migration_backup {
            Some(backup_dir) => copy_before_migration(&conn, backup_dir)?,
            None => None,
        };
        migrate(&conn)?;
        Ok(Store {
            conn,
            undo: Vec::new(),
            redo: Vec::new(),
            migration_copy,
        })
    }

    /// The copy written before this store upgraded an older database, when one was made.
    pub fn migration_copy(&self) -> Option<&Path> {
        self.migration_copy.as_deref()
    }

    // ----- pages -------------------------------------------------------------------------------

    pub fn page(&self, id: i64) -> Result<Page> {
        self.conn
            .query_row(
                "SELECT id, name, name_key, kind, journal_date FROM page WHERE id = ?1",
                [id],
                read_page,
            )
            .optional()?
            .ok_or(Error::NotFound(id))
    }

    pub fn page_by_name(&self, name: &str) -> Result<Option<Page>> {
        let key = name.trim().to_lowercase();
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, name_key, kind, journal_date FROM page WHERE name_key = ?1",
                [key],
                read_page,
            )
            .optional()?)
    }

    /// Find or create a named page — what a `[[link]]` or `#tag` does when it names something new.
    pub fn ensure_page(&mut self, name: &str) -> Result<i64> {
        ensure_page_on(&self.conn, name)
    }

    /// Every page, for the page switcher — written journal days included, because a date is a name a
    /// block can link to. A day nobody wrote has no row and so cannot be found this way: scrolling the
    /// column is what reaches it.
    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page ORDER BY name_key",
        )?;
        let pages = stmt
            .query_map([], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pages)
    }

    /// Rename a named page and rewrite the explicit references that resolve to it. One undo step.
    pub fn rename_page(&mut self, page_id: i64, new_name: &str) -> Result<Renamed> {
        let page = self.page(page_id)?;
        if page.kind == PageKind::Journal {
            return Err(Error::CannotRenameJournal);
        }

        let name = new_name.trim();
        if name.is_empty() {
            return Err(Error::BadRenameName("a page needs a name".to_string()));
        }
        if name.contains(['[', ']', '\n', '\r']) {
            return Err(Error::BadRenameName(
                "a page name cannot contain brackets or a line break".to_string(),
            ));
        }
        if is_date_name(name) {
            return Err(Error::BadRenameName(
                "a date is a journal day, not a page name".to_string(),
            ));
        }

        let key = page_key(name);
        if let Some(other) = self.page_by_name(&key)? {
            if other.id != page_id {
                return Err(Error::PageExists(name.to_string()));
            }
        }

        let mentions = self
            .references(page_id)?
            .into_iter()
            .filter(|row| row.kind == ReferenceKind::Mention)
            .count();

        let mut changes = Vec::new();
        {
            let mut stmt = self.conn.prepare("SELECT id, content FROM block")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (block_id, content) = row?;
                if let Some(rewritten) = rewrite_page_links(&content, &page.name_key, name) {
                    changes.push((block_id, rewritten));
                }
            }
        }

        let changed = changes.len();
        self.exec(Action::RenamePage {
            page_id,
            from_name: page.name,
            from_key: page.name_key,
            to_name: name.to_string(),
            to_key: key,
            changes,
        })?;
        Ok(Renamed {
            blocks: changed,
            mentions,
        })
    }

    /// Delete a named page and its blocks. Refused while any block explicitly links to it.
    pub fn delete_page(&mut self, page_id: i64) -> Result<()> {
        let page = self.page(page_id)?;
        if page.kind == PageKind::Journal {
            return Err(Error::CannotDeleteJournal);
        }
        let linked: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM block_link WHERE page_id = ?1",
            [page_id],
            |row| row.get(0),
        )?;
        if linked > 0 {
            return Err(Error::PageHasLinks(linked as usize));
        }

        let record = page_record(&self.conn, page_id)?;
        let blocks = page_block_records(&self.conn, page_id)?;
        self.exec(Action::DeletePage {
            page: record,
            blocks,
        })
    }

    /// Look up a journal day without creating it. Reading is not writing.
    pub fn journal_page(&self, date: &str) -> Result<Option<Page>> {
        ensure_date(date)?;
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, name_key, kind, journal_date FROM page WHERE journal_date = ?1",
                [date],
                read_page,
            )
            .optional()?)
    }

    /// Find or create the journal page for `date`.
    pub fn ensure_journal_page(&mut self, date: &str) -> Result<i64> {
        ensure_journal_page_on(&self.conn, date)
    }

    // ----- commands ----------------------------------------------------------------------------

    /// Insert a block after `after` (or at the end of the page's top level) and return its id.
    pub fn insert_block(&mut self, page_id: i64, after: Option<i64>, content: &str) -> Result<i64> {
        self.page(page_id)?;

        let (parent_id, prev_key, next_key) = match after {
            Some(after_id) => {
                let anchor = block_record(&self.conn, after_id)?;
                if anchor.page_id != page_id {
                    return Err(Error::NotFound(after_id));
                }
                let next =
                    self.next_sibling_key(page_id, anchor.parent_id, &anchor.order_key)?;
                (anchor.parent_id, Some(anchor.order_key), next)
            }
            None => {
                let last: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT order_key FROM block
                         WHERE page_id = ?1 AND parent_id IS NULL
                         ORDER BY order_key DESC LIMIT 1",
                        [page_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                (None, last, None)
            }
        };

        let id = self.next_block_id()?;
        let now = now();
        let record = BlockRecord {
            id,
            page_id,
            parent_id,
            order_key: key_between(prev_key.as_deref(), next_key.as_deref())?,
            content: content.to_string(),
            marker: None,
            scheduled_date: None,
            collapsed: false,
            created_at: now.clone(),
            updated_at: now,
        };
        self.exec(Action::Insert { record })?;
        Ok(id)
    }

    pub fn update_content(&mut self, block_id: i64, content: &str) -> Result<()> {
        self.exec(Action::SetContent {
            block_id,
            content: content.to_string(),
        })
    }

    /// Split a block at `offset`: the text before stays, the text after moves into a new sibling.
    /// One command, therefore one undo step.
    pub fn split_block(&mut self, block_id: i64, offset: usize) -> Result<i64> {
        let record = block_record(&self.conn, block_id)?;
        if offset > record.content.len() || !record.content.is_char_boundary(offset) {
            return Err(Error::BadOffset(offset));
        }
        let left = record.content[..offset].to_string();
        let right = record.content[offset..].to_string();
        let next = self.next_sibling_key(record.page_id, record.parent_id, &record.order_key)?;

        let id = self.next_block_id()?;
        let now = now();
        let created = BlockRecord {
            id,
            page_id: record.page_id,
            parent_id: record.parent_id,
            order_key: key_between(Some(&record.order_key), next.as_deref())?,
            content: right,
            marker: None,
            scheduled_date: None,
            collapsed: false,
            created_at: now.clone(),
            updated_at: now,
        };
        self.exec(Action::Composite(vec![
            Action::SetContent {
                block_id,
                content: left,
            },
            Action::Insert { record: created },
        ]))?;
        Ok(id)
    }

    /// Nest a block under its previous sibling. The first sibling has nowhere to go, so this is a no-op.
    pub fn indent(&mut self, block_id: i64) -> Result<()> {
        let record = block_record(&self.conn, block_id)?;
        let previous: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM block
                 WHERE page_id = ?1 AND parent_id IS ?2 AND order_key < ?3
                 ORDER BY order_key DESC LIMIT 1",
                params![record.page_id, record.parent_id, record.order_key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(previous) = previous else {
            return Ok(());
        };
        let new_parent = block_record(&self.conn, previous)?.id;
        let last_child: Option<String> = self
            .conn
            .query_row(
                "SELECT order_key FROM block WHERE parent_id = ?1 ORDER BY order_key DESC LIMIT 1",
                [new_parent],
                |row| row.get(0),
            )
            .optional()?;
        self.exec(Action::SetPosition {
            block_id,
            page_id: record.page_id,
            parent_id: Some(new_parent),
            order_key: key_between(last_child.as_deref(), None)?,
        })
    }

    /// Lift a block out of its parent, placing it just after the parent. A top-level block is a no-op.
    pub fn outdent(&mut self, block_id: i64) -> Result<()> {
        let record = block_record(&self.conn, block_id)?;
        let Some(parent_id) = record.parent_id else {
            return Ok(());
        };
        let parent = block_record(&self.conn, parent_id)?;
        let next = self.next_sibling_key(parent.page_id, parent.parent_id, &parent.order_key)?;
        self.exec(Action::SetPosition {
            block_id,
            page_id: parent.page_id,
            parent_id: parent.parent_id,
            order_key: key_between(Some(&parent.order_key), next.as_deref())?,
        })
    }

    /// Walk the marker cycle: none → TODO → DOING → DONE → none.
    pub fn cycle_marker(&mut self, block_id: i64) -> Result<()> {
        let current = block_record(&self.conn, block_id)?.marker;
        let next = match current {
            Some(marker) => marker.next(),
            None => Some(Marker::first()),
        };
        self.exec(Action::SetMarker {
            block_id,
            marker: next,
        })
    }

    /// Set or clear a block's marker directly.
    pub fn set_marker(&mut self, block_id: i64, marker: Option<Marker>) -> Result<()> {
        self.exec(Action::SetMarker { block_id, marker })
    }

    /// Set a block's marker and its text together, as one undo step: what a slash command does when it
    /// takes the token it replaced out of the text and marks the block in the same gesture. Two commands
    /// would cost two presses of undo for one thing the user did.
    pub fn set_marker_with_content(
        &mut self,
        block_id: i64,
        marker: Option<Marker>,
        content: &str,
    ) -> Result<()> {
        self.exec(Action::Composite(vec![
            Action::SetContent {
                block_id,
                content: content.to_string(),
            },
            Action::SetMarker { block_id, marker },
        ]))
    }

    /// Schedule a block for `date`, or clear the date with `None`.
    ///
    /// Scheduling a block that is not a task also marks it TODO: the agenda lists tasks, so a date on an
    /// untasked block would silently never appear. Both writes are one action, so one undo steps back over
    /// the whole thing.
    pub fn set_scheduled(&mut self, block_id: i64, date: Option<&str>) -> Result<()> {
        if let Some(date) = date {
            ensure_date(date)?;
        }
        let actions = self.schedule_actions(block_id, date)?;
        self.exec(Action::Composite(actions))
    }

    /// The same for a scheduled date plus the text the command rewrote, so one undo takes back the whole
    /// slash command rather than the two halves of it.
    pub fn set_scheduled_with_content(
        &mut self,
        block_id: i64,
        date: Option<&str>,
        content: &str,
    ) -> Result<()> {
        if let Some(date) = date {
            ensure_date(date)?;
        }
        let mut actions = vec![Action::SetContent {
            block_id,
            content: content.to_string(),
        }];
        actions.extend(self.schedule_actions(block_id, date)?);
        self.exec(Action::Composite(actions))
    }

    /// The writes a scheduled date implies, in order: the date itself, then the TODO that makes the block
    /// visible to the agenda.
    fn schedule_actions(&self, block_id: i64, date: Option<&str>) -> Result<Vec<Action>> {
        let record = block_record(&self.conn, block_id)?;
        let mut actions = vec![Action::SetScheduled {
            block_id,
            date: date.map(str::to_string),
        }];
        if date.is_some() && record.marker.is_none() {
            actions.push(Action::SetMarker {
                block_id,
                marker: Some(Marker::first()),
            });
        }
        Ok(actions)
    }

    /// Everything due: marked `TODO`/`DOING`, with a date up to `horizon_days` ahead. Past dates stay,
    /// flagged overdue, until the block is marked `DONE`.
    pub fn agenda(&self, today: &str, horizon_days: u32) -> Result<Vec<AgendaRow>> {
        let today_date = ensure_date(today)?;
        let horizon = today_date
            .checked_add_days(Days::new(horizon_days as u64))
            .ok_or_else(|| Error::BadDate(today.to_string()))?
            .format("%Y-%m-%d")
            .to_string();

        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.parent_id, b.order_key, b.content, b.marker, b.scheduled_date, b.collapsed,
                    p.name, p.journal_date
             FROM block b JOIN page p ON p.id = b.page_id
             WHERE b.marker IN ('TODO', 'DOING')
               AND b.scheduled_date IS NOT NULL
               AND b.scheduled_date <= ?1
             ORDER BY b.scheduled_date, p.id, b.order_key",
        )?;

        let rows = stmt
            .query_map([horizon], |row| {
                let scheduled: Option<String> = row.get(5)?;
                Ok((
                    BlockRow {
                        id: row.get(0)?,
                        parent_id: row.get(1)?,
                        depth: 0,
                        order_key: row.get(2)?,
                        content: row.get(3)?,
                        marker: row.get::<_, Option<String>>(4)?.as_deref().and_then(Marker::parse),
                        scheduled_date: scheduled.clone(),
                        collapsed: row.get(6)?,
                        child_count: 0,
                    },
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows
            .into_iter()
            .map(|(block, page_name, journal_date)| {
                let overdue = block
                    .scheduled_date
                    .as_ref()
                    .map(|date| date.as_str() < today)
                    .unwrap_or(false);
                AgendaRow {
                    block,
                    page_name,
                    journal_date,
                    overdue,
                }
            })
            .collect())
    }

    /// Blocks that reference `page`: explicit links first, then plain mentions.
    ///
    /// Mentions come from the full-text index, minus the blocks that already link, and are matched on
    /// whole words so a page called "Pla" is not "found" inside "Plastic".
    /// The quick switcher's query: pages whose name contains `query`, then blocks whose text matches it.
    ///
    /// Pages rank exact name, then prefix, then anywhere, alphabetical within a rank. Blocks are ranked by
    /// FTS5's bm25 over `block_fts` — the index exists for exactly this — and queried as a phrase with a
    /// prefix on its last word, so a half-typed word already matches. Both are capped: this feeds a list,
    /// not an archive.
    pub fn search(&self, query: &str, page_limit: usize, block_limit: usize) -> Result<SearchResults> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(SearchResults::default());
        }
        let needle = trimmed.to_lowercase();

        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page
             WHERE instr(name_key, ?1) > 0
             ORDER BY CASE WHEN name_key = ?1 THEN 0
                           WHEN instr(name_key, ?1) = 1 THEN 1
                           ELSE 2 END,
                      name_key
             LIMIT ?2",
        )?;
        let pages = stmt
            .query_map(params![needle, page_limit as i64], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Quoted, so the query is a phrase and its punctuation cannot become syntax.
        let fts_query = format!("\"{}\"*", trimmed.replace('"', ""));
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.parent_id, b.order_key, b.content, b.marker, b.scheduled_date, b.collapsed,
                    p.name, p.journal_date
             FROM block_fts f
             JOIN block b ON b.id = f.rowid
             JOIN page p ON p.id = b.page_id
             WHERE block_fts MATCH ?1
             ORDER BY bm25(block_fts)
             LIMIT ?2",
        )?;
        let blocks = stmt
            .query_map(params![fts_query.clone(), block_limit as i64], |row| {
                let block = read_block_row(row, 0)?;
                let (snippet, matched) = snippet(&block.content, &needle);
                Ok(BlockHit {
                    snippet,
                    matched,
                    block,
                    page_name: row.get(7)?,
                    journal_date: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // The caps decide what is shown, not what exists: an "all results (N)" count needs the latter.
        let total = {
            let page_total: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM page WHERE kind = 'page' AND instr(name_key, ?1) > 0",
                params![needle],
                |row| row.get(0),
            )?;
            let block_total: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH ?1",
                params![fts_query],
                |row| row.get(0),
            )?;
            (page_total + block_total) as usize
        };

        Ok(SearchResults {
            pages,
            blocks,
            total,
        })
    }

    /// Every journal day that was ever written, oldest first — what an export walks.
    pub fn journal_pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page
             WHERE kind = 'journal'
             ORDER BY journal_date",
        )?;
        let pages = stmt
            .query_map([], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pages)
    }

    /// Star or unstar a page. Outside the undo history: a note's text is what undo walks, and a star is
    /// not an edit to the note.
    pub fn set_favourite(&mut self, page_id: i64, favourite: bool) -> Result<()> {
        self.conn
            .execute(
                "UPDATE page SET favourite = ?2 WHERE id = ?1",
                params![page_id, i64::from(favourite)],
            )
            .map_err(Error::from)?;
        Ok(())
    }

    /// Whether a page is starred.
    pub fn is_favourite(&self, page_id: i64) -> Result<bool> {
        let favourite: Option<i64> = self
            .conn
            .query_row("SELECT favourite FROM page WHERE id = ?1", [page_id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(favourite.unwrap_or(0) != 0)
    }

    /// Record that a page was opened. Also outside the undo history: opening is not editing.
    ///
    /// The order is a counter, not a clock: two pages opened in the same second — the same millisecond, in
    /// a test — still have a definite order, and a wall clock that jumps backwards cannot reshuffle the
    /// sidebar.
    pub fn record_visit(&mut self, page_id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE page SET visit_order = (SELECT IFNULL(MAX(visit_order), 0) + 1 FROM page)
                 WHERE id = ?1",
                [page_id],
            )
            .map_err(Error::from)?;
        Ok(())
    }

    /// The starred pages, by name — the sidebar's Favourites.
    pub fn pages_by_favourite(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page
             WHERE kind = 'page' AND favourite = 1
             ORDER BY name_key",
        )?;
        let pages = stmt
            .query_map([], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pages)
    }

    /// Named pages opened most recently, newest first — the sidebar's Recent.
    pub fn recent_pages(&self, limit: usize) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page
             WHERE kind = 'page' AND visit_order IS NOT NULL
             ORDER BY visit_order DESC
             LIMIT ?1",
        )?;
        let pages = stmt
            .query_map([limit as i64], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pages)
    }

    /// Write a consistent copy of the whole database to `path`.
    ///
    /// `VACUUM INTO` is the one way to copy a live SQLite database and get a file that is not a torn mix of
    /// pages: it reads under the connection's own snapshot and writes a compacted copy that opens on its
    /// own. The copy is prepared under a temporary name and only swapped in once it is complete, so a
    /// failed backup leaves the previous file exactly where it was.
    pub fn backup_into(&self, path: &Path) -> Result<()> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Backup(format!("{} has no file name", path.display())))?;
        let temp = path.with_file_name(format!("{name}.tmp"));
        let old = path.with_file_name(format!("{name}.old"));
        let failed = |error: std::io::Error| Error::Backup(error.to_string());

        // A leftover temporary or old file is not data; clear it before writing. If one is a directory,
        // this fails and the real target is still untouched.
        if temp.exists() {
            std::fs::remove_file(&temp).map_err(failed)?;
        }
        if old.exists() {
            std::fs::remove_file(&old).map_err(failed)?;
        }

        self.conn
            .execute("VACUUM INTO ?1", params![temp.to_string_lossy()])
            .map_err(Error::from)?;

        let had_target = path.exists();
        if had_target {
            std::fs::rename(path, &old).map_err(failed)?;
        }
        if let Err(error) = std::fs::rename(&temp, path) {
            if had_target {
                let _ = std::fs::rename(&old, path);
            }
            let _ = std::fs::remove_file(&temp);
            return Err(failed(error));
        }
        if had_target {
            let _ = std::fs::remove_file(&old);
        }
        Ok(())
    }

    /// A stored setting, or `None` when it was never written.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Write a setting, replacing any previous value.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(Error::from)?;
        Ok(())
    }

    /// Write parsed pages into the store as one transaction and one undo step.
    ///
    /// `Merge` replaces the blocks of every page or day the folder names; `ReplaceAll` clears every block
    /// first. The page rows are addresses and stay (or are created) outside the history.
    pub(crate) fn import_pages(&mut self, pages: &[ParsedPage], mode: ImportMode) -> Result<Moved> {
        let mut moved = Moved::default();
        let tx = self.conn.transaction()?;
        let mut inverses: Vec<Action> = Vec::new();

        if mode == ImportMode::ReplaceAll {
            for block_id in top_level_blocks(&tx, None)? {
                inverses.push(apply_action(&tx, &Action::Delete { block_id })?);
            }
        }

        for page in pages {
            let page_id = match page.journal_date.as_deref() {
                Some(date) => ensure_journal_page_on(&tx, date)?,
                None => ensure_page_on(&tx, &page.name)?,
            };

            if mode == ImportMode::Merge {
                for block_id in top_level_blocks(&tx, Some(page_id))? {
                    inverses.push(apply_action(&tx, &Action::Delete { block_id })?);
                }
            }

            // The last block at each depth, so a bullet lands as the child of the nearest shallower one.
            let mut last_at_depth: HashMap<usize, i64> = HashMap::new();
            for bullet in &page.bullets {
                let depth = bullet.depth;
                let (parent_id, last_key) = if depth == 0 {
                    (None, last_top_key(&tx, page_id)?)
                } else {
                    match (0..depth).rev().find(|level| last_at_depth.contains_key(level)) {
                        Some(level) => {
                            let parent = last_at_depth[&level];
                            (Some(parent), last_child_key(&tx, parent)?)
                        }
                        None => (None, last_top_key(&tx, page_id)?),
                    }
                };

                let id = next_block_id_on(&tx)?;
                let now = now();
                let record = BlockRecord {
                    id,
                    page_id,
                    parent_id,
                    order_key: key_between(last_key.as_deref(), None)?,
                    content: bullet.content.clone(),
                    marker: bullet.marker,
                    scheduled_date: bullet.scheduled.clone(),
                    collapsed: false,
                    created_at: now.clone(),
                    updated_at: now,
                };
                inverses.push(apply_action(&tx, &Action::Insert { record })?);

                last_at_depth.insert(depth, id);
                for level in last_at_depth.keys().copied().collect::<Vec<_>>() {
                    if level > depth {
                        last_at_depth.remove(&level);
                    }
                }
                moved.blocks += 1;
            }

            if page.journal_date.is_some() {
                moved.days += 1;
            } else {
                moved.pages += 1;
            }
        }

        tx.commit()?;
        inverses.reverse();
        self.undo.push(Action::Composite(inverses));
        self.redo.clear();
        Ok(moved)
    }

    /// The page a block belongs to — what the view needs to show a block a search found.
    pub fn page_of_block(&self, block_id: i64) -> Result<Page> {
        self.conn
            .query_row(
                "SELECT p.id, p.name, p.name_key, p.kind, p.journal_date
                 FROM page p JOIN block b ON b.page_id = p.id
                 WHERE b.id = ?1",
                [block_id],
                read_page,
            )
            .optional()?
            .ok_or(Error::NotFound(block_id))
    }

    pub fn references(&self, page_id: i64) -> Result<Vec<ReferenceRow>> {
        let page = self.page(page_id)?;
        let mut out = Vec::new();

        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.parent_id, b.order_key, b.content, b.marker, b.scheduled_date, b.collapsed,
                    p.name, p.journal_date
             FROM block_link l
             JOIN block b ON b.id = l.block_id
             JOIN page p ON p.id = b.page_id
             WHERE l.page_id = ?1
             ORDER BY p.journal_date IS NULL, p.journal_date DESC, b.order_key",
        )?;
        let linked = stmt
            .query_map([page_id], |row| {
                Ok(ReferenceRow {
                    block: read_block_row(row, 0)?,
                    page_name: row.get(7)?,
                    journal_date: row.get(8)?,
                    kind: ReferenceKind::Linked,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let linked_ids: Vec<i64> = linked.iter().map(|row| row.block.id).collect();
        out.extend(linked);

        let trimmed = page.name.trim();
        if trimmed.is_empty() {
            return Ok(out);
        }

        // Quoted so FTS treats it as a phrase rather than a query.
        let query = format!("\"{}\"", trimmed.replace('"', ""));
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.parent_id, b.order_key, b.content, b.marker, b.scheduled_date, b.collapsed,
                    p.name, p.journal_date
             FROM block_fts f
             JOIN block b ON b.id = f.rowid
             JOIN page p ON p.id = b.page_id
             WHERE block_fts MATCH ?1
             ORDER BY p.journal_date IS NULL, p.journal_date DESC, b.order_key
             LIMIT 200",
        )?;
        let mentions = stmt
            .query_map([query], |row| {
                Ok(ReferenceRow {
                    block: read_block_row(row, 0)?,
                    page_name: row.get(7)?,
                    journal_date: row.get(8)?,
                    kind: ReferenceKind::Mention,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        out.extend(
            mentions
                .into_iter()
                .filter(|row| !linked_ids.contains(&row.block.id))
                .filter(|row| mentions_whole_words(&row.block.content, trimmed)),
        );
        Ok(out)
    }

    /// Context for consolidating one named project page.
    ///
    /// Sources are journal blocks that explicitly link the page. Each source is returned with its
    /// descendants and ancestor framing. The project page itself, and any `[agent]` task subtree, is
    /// excluded. Plain mentions are not sources.
    pub fn project_context(
        &self,
        page_name: &str,
        since: Option<&str>,
        full: bool,
    ) -> Result<ProjectContext> {
        let page = self
            .page_by_name(page_name)?
            .ok_or_else(|| Error::PageNotFound(page_name.to_string()))?;
        if page.kind != PageKind::Page {
            return Err(Error::InvalidConsolidation(
                "a project must be a named page, not a journal day".to_string(),
            ));
        }
        if let Some(since) = since {
            ensure_date(since)?;
        }

        let blocks = self.page_blocks_all(page.id)?;
        let digest_roots: Vec<&BlockRow> = blocks
            .iter()
            .filter(|block| {
                block.parent_id.is_none() && block.content.trim() == project::DIGEST_HEADING
            })
            .collect();
        if digest_roots.len() > 1 {
            return Err(Error::InvalidConsolidation(
                "the project page has more than one agent digest section".to_string(),
            ));
        }
        let digest_root = digest_roots.first().copied();
        let digest_blocks = digest_root
            .map(|root| descendants_of(&blocks, root.id))
            .unwrap_or_default();
        let last_consolidated = digest_blocks
            .iter()
            .find(|block| block.content.trim_start().starts_with(project::LAST_CONSOLIDATED_PREFIX))
            .and_then(|block| project::first_date_link(&block.content));
        let digest_ids: HashSet<i64> = digest_root
            .map(|root| {
                std::iter::once(root.id)
                    .chain(digest_blocks.iter().map(|block| block.id))
                    .collect()
            })
            .unwrap_or_default();
        let human_notes = blocks
            .iter()
            .filter(|block| !digest_ids.contains(&block.id))
            .map(block_ref)
            .collect();

        let digest = DigestInfo {
            exists: digest_root.is_some(),
            block_id: digest_root.map(|block| block.id),
            last_consolidated: last_consolidated.clone(),
            blocks: digest_blocks,
            human_notes,
        };

        let effective_since = if full {
            None
        } else {
            since.map(str::to_string).or_else(|| last_consolidated.clone())
        };
        let all_sources = self.project_sources(page.id)?;
        let latest_day = all_sources.iter().map(|source| source.day.clone()).max();
        let nothing_new = match (effective_since.as_deref(), latest_day.as_deref()) {
            (Some(since), Some(latest)) => latest <= since,
            (Some(_), None) => true,
            (None, None) => true,
            (None, Some(_)) => false,
        };
        let sources = all_sources
            .into_iter()
            .filter(|source| {
                effective_since
                    .as_deref()
                    .is_none_or(|since| source.day.as_str() > since)
            })
            .collect();

        let agent_tasks = self.agent_tasks(page.id)?;
        let human_open_tasks = self.human_open_tasks(page.id)?;

        Ok(ProjectContext {
            project: page,
            digest,
            sources,
            agent_tasks,
            human_open_tasks,
            nothing_new,
        })
    }

    /// Apply one project consolidation as a single undoable action.
    pub fn apply_consolidation(
        &mut self,
        request: &ConsolidationRequest,
    ) -> Result<ConsolidationResult> {
        let page = self
            .page_by_name(&request.page)?
            .ok_or_else(|| Error::PageNotFound(request.page.clone()))?;
        if page.kind != PageKind::Page {
            return Err(Error::InvalidConsolidation(
                "a project must be a named page, not a journal day".to_string(),
            ));
        }

        let through_date = ensure_date(&request.through)?;
        let today = Local::now().date_naive();
        if through_date > today {
            return Err(Error::BadDate(format!(
                "{} is in the future",
                request.through
            )));
        }
        validate_consolidation_text(request, &page)?;

        let blocks = self.page_blocks_all(page.id)?;
        let digest_roots: Vec<&BlockRow> = blocks
            .iter()
            .filter(|block| {
                block.parent_id.is_none() && block.content.trim() == project::DIGEST_HEADING
            })
            .collect();
        if digest_roots.len() > 1 {
            return Err(Error::InvalidConsolidation(
                "the project page has more than one agent digest section".to_string(),
            ));
        }
        let previous_last = digest_roots
            .first()
            .map(|root| descendants_of(&blocks, root.id))
            .unwrap_or_default()
            .iter()
            .find(|block| block.content.trim_start().starts_with(project::LAST_CONSOLIDATED_PREFIX))
            .and_then(|block| project::first_date_link(&block.content));
        let source_days = self.source_days(page.id, previous_last.as_deref(), &request.through)?;

        let new_tasks = self.new_agent_tasks(page.id, &request.tasks_to_create)?;
        let updates = self.closable_agent_tasks(&request.task_updates)?;
        let has_digest_content =
            !request.brief.is_empty() || !request.timeline_entries.is_empty();
        if !has_digest_content && new_tasks.is_empty() && updates.is_empty() {
            return Ok(ConsolidationResult {
                applied: false,
                through: request.through.clone(),
                source_days,
                tasks_created: 0,
                tasks_closed: 0,
                digest_block_id: digest_roots.first().map(|block| block.id),
                task_root_block_id: None,
            });
        }

        let tx = self.conn.transaction()?;
        let mut inverses: Vec<Action> = Vec::new();
        let today_string = today.format("%Y-%m-%d").to_string();

        let digest_id = ensure_digest_root(&tx, page.id, &mut inverses)?;
        let children = direct_children(&tx, digest_id)?;
        let last_id = children
            .iter()
            .find(|(_, content, _)| content.trim_start().starts_with(project::LAST_CONSOLIDATED_PREFIX))
            .map(|(id, _, _)| *id);
        let timeline_id = children
            .iter()
            .find(|(_, content, _)| content.trim() == project::TIMELINE_HEADING)
            .map(|(id, _, _)| *id);

        for (id, content, _) in &children {
            let keep = Some(*id) == last_id
                || Some(*id) == timeline_id
                || content.trim_start().starts_with(project::LAST_CONSOLIDATED_PREFIX)
                || content.trim() == project::TIMELINE_HEADING;
            if !keep {
                inverses.push(apply_action(&tx, &Action::Delete { block_id: *id })?);
            }
        }

        let last_content = format!(
            "{} [[{}]]",
            project::LAST_CONSOLIDATED_PREFIX,
            request.through
        );
        let last_id = match last_id {
            Some(id) => {
                update_block_content(&tx, id, &last_content, &mut inverses)?;
                id
            }
            None => insert_digest_child_at_start(
                &tx,
                page.id,
                digest_id,
                &last_content,
                &mut inverses,
            )?,
        };
        let last_key = block_record(&tx, last_id)?.order_key;

        let timeline_id = match timeline_id {
            Some(id) => id,
            None => insert_digest_child_at_end(
                &tx,
                page.id,
                digest_id,
                project::TIMELINE_HEADING,
                &mut inverses,
            )?,
        };
        let timeline_key = block_record(&tx, timeline_id)?.order_key;

        // Brief sections are direct children between the last-consolidated line and the timeline.
        let mut previous_key = last_key;
        let mut insert_brief_section =
            |heading: &str, items: &[BriefItem], previous_key: &mut String| -> Result<()> {
                let key = key_between(Some(previous_key.as_str()), Some(timeline_key.as_str()))?;
                let id = insert_block_at(
                    &tx,
                    page.id,
                    Some(digest_id),
                    &key,
                    heading,
                    None,
                    None,
                    &mut inverses,
                )?;
                *previous_key = key;
                for item in items {
                    let content = project::format_brief_item(item);
                    append_child(&tx, page.id, id, &content, None, &mut inverses)?;
                }
                Ok(())
            };
        insert_brief_section(
            "### Current state",
            &request.brief.current_state,
            &mut previous_key,
        )?;
        insert_brief_section(
            "### Decisions",
            &request.brief.decisions,
            &mut previous_key,
        )?;
        insert_brief_section(
            "### Open questions",
            &request.brief.open_questions,
            &mut previous_key,
        )?;
        drop(insert_brief_section);

        // Merge timeline entries by date: one child per day.
        for entry in &request.timeline_entries {
            ensure_date(&entry.date)?;
            let content = project::format_timeline_entry(&entry.date, &entry.text);
            let entries = direct_children(&tx, timeline_id)?;
            let mut matches: Vec<i64> = entries
                .iter()
                .filter(|(_, content, _)| {
                    project::timeline_entry_date(content).as_deref() == Some(entry.date.as_str())
                })
                .map(|(id, _, _)| *id)
                .collect();
            if let Some(first) = matches.first().copied() {
                update_block_content(&tx, first, &content, &mut inverses)?;
                for duplicate in matches.drain(1..) {
                    inverses.push(apply_action(
                        &tx,
                        &Action::Delete { block_id: duplicate },
                    )?);
                }
            } else {
                append_child(
                    &tx,
                    page.id,
                    timeline_id,
                    &content,
                    None,
                    &mut inverses,
                )?;
            }
        }

        let mut task_root_block_id = None;
        if !new_tasks.is_empty() {
            let today_page = ensure_journal_page_on(&tx, &today_string)?;
            let root_id = find_or_create_agent_task_root(
                &tx,
                today_page,
                page.id,
                &page.name,
                &mut inverses,
            )?;
            task_root_block_id = Some(root_id);
            for task in &new_tasks {
                let content = project::format_task(&task.text, &task.source_day);
                append_child(
                    &tx,
                    today_page,
                    root_id,
                    &content,
                    Some(Marker::Todo),
                    &mut inverses,
                )?;
            }
        }

        for block_id in &updates {
            inverses.push(apply_action(
                &tx,
                &Action::SetMarker {
                    block_id: *block_id,
                    marker: Some(Marker::Done),
                },
            )?);
        }

        tx.commit()?;
        inverses.reverse();
        self.undo.push(Action::Composite(inverses));
        self.redo.clear();

        Ok(ConsolidationResult {
            applied: true,
            through: request.through.clone(),
            source_days,
            tasks_created: new_tasks.len(),
            tasks_closed: updates.len(),
            digest_block_id: Some(digest_id),
            task_root_block_id,
        })
    }

    fn project_sources(&self, project_id: i64) -> Result<Vec<ProjectSource>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.page_id, p.journal_date
             FROM block_link l
             JOIN block b ON b.id = l.block_id
             JOIN page p ON p.id = b.page_id
             WHERE l.page_id = ?1 AND p.kind = 'journal' AND l.kind IN ('link', 'tag')
             ORDER BY p.journal_date, b.order_key",
        )?;
        let refs = stmt
            .query_map([project_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut by_page: HashMap<i64, Vec<BlockRow>> = HashMap::new();
        for (_, page_id, _) in &refs {
            if !by_page.contains_key(page_id) {
                by_page.insert(*page_id, self.page_blocks_all(*page_id)?);
            }
        }

        let mut sources = Vec::new();
        for day in refs.iter().map(|(_, _, day)| day.clone()).collect::<HashSet<_>>() {
            let day_refs: Vec<(i64, i64)> = refs
                .iter()
                .filter(|(_, _, d)| *d == day)
                .map(|(block_id, page_id, _)| (*block_id, *page_id))
                .collect();
            let ref_ids: HashSet<i64> = day_refs.iter().map(|(id, _)| *id).collect();
            for (block_id, page_id) in day_refs {
                let blocks = &by_page[&page_id];
                let Some(root_id) = top_root_id(blocks, block_id) else {
                    continue;
                };
                let Some(root) = blocks.iter().find(|block| block.id == root_id) else {
                    continue;
                };
                if project::is_agent_root(&root.content) {
                    continue;
                }
                // A descendant reference under a reference root is already in that root's subtree.
                if ancestors_of(blocks, block_id)
                    .iter()
                    .any(|ancestor| ref_ids.contains(&ancestor.id))
                {
                    continue;
                }
                let Some(block) = blocks.iter().find(|block| block.id == block_id) else {
                    continue;
                };
                sources.push(ProjectSource {
                    day: day.clone(),
                    root: block_ref(block),
                    ancestors: ancestors_of(blocks, block_id),
                    children: descendants_of(blocks, block_id),
                });
            }
        }
        sources.sort_by(|left, right| {
            left.day
                .cmp(&right.day)
                .then_with(|| left.root.content.cmp(&right.root.content))
        });
        Ok(sources)
    }

    fn source_days(
        &self,
        project_id: i64,
        since: Option<&str>,
        through: &str,
    ) -> Result<Vec<String>> {
        let mut days: Vec<String> = self
            .project_sources(project_id)?
            .into_iter()
            .map(|source| source.day)
            .filter(|day| day.as_str() <= through)
            .filter(|day| since.is_none_or(|since| day.as_str() > since))
            .collect();
        days.sort();
        days.dedup();
        Ok(days)
    }

    fn agent_tasks(&self, project_id: i64) -> Result<Vec<TaskRef>> {
        let roots = self.agent_task_roots(project_id)?;
        let mut tasks = Vec::new();
        for (root_id, page_id, day) in roots {
            let blocks = self.page_blocks_all(page_id)?;
            for block in descendants_of(&blocks, root_id) {
                if block.marker.is_none() {
                    continue;
                }
                tasks.push(TaskRef {
                    block_id: block.id,
                    text: block.content.clone(),
                    marker: block.marker,
                    source_day: project::first_date_link(&block.content),
                    page_name: day.clone(),
                    journal_date: Some(day.clone()),
                    agent_owned: true,
                });
            }
        }
        Ok(tasks)
    }

    fn human_open_tasks(&self, project_id: i64) -> Result<Vec<TaskRef>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.page_id, p.name, p.journal_date, b.content, b.marker
             FROM block_link l
             JOIN block b ON b.id = l.block_id
             JOIN page p ON p.id = b.page_id
             WHERE l.page_id = ?1 AND b.marker IN ('TODO', 'DOING')
               AND l.kind IN ('link', 'tag')
             ORDER BY p.journal_date IS NULL, p.journal_date DESC, b.order_key",
        )?;
        let rows = stmt
            .query_map([project_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut tasks = Vec::new();
        for (block_id, page_id, page_name, journal_date, content, marker) in rows {
            let blocks = self.page_blocks_all(page_id)?;
            let Some(root_id) = top_root_id(&blocks, block_id) else {
                continue;
            };
            let Some(root) = blocks.iter().find(|block| block.id == root_id) else {
                continue;
            };
            if project::is_agent_root(&root.content) {
                continue;
            }
            tasks.push(TaskRef {
                block_id,
                text: content.clone(),
                marker: marker.as_deref().and_then(Marker::parse),
                source_day: journal_date.clone().or_else(|| project::first_date_link(&content)),
                page_name,
                journal_date,
                agent_owned: false,
            });
        }
        Ok(tasks)
    }

    fn agent_task_roots(&self, project_id: i64) -> Result<Vec<(i64, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.page_id, p.journal_date
             FROM block b
             JOIN page p ON p.id = b.page_id
             WHERE p.kind = 'journal' AND b.parent_id IS NULL
               AND substr(trim(b.content), 1, 7) = ?2
               AND EXISTS (SELECT 1 FROM block_link l
                           WHERE l.block_id = b.id AND l.page_id = ?1)
             ORDER BY p.journal_date, b.order_key",
        )?;
        Ok(stmt
            .query_map(params![project_id, project::AGENT_PREFIX], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn new_agent_tasks(
        &self,
        project_id: i64,
        proposed: &[TaskToCreate],
    ) -> Result<Vec<TaskToCreate>> {
        let mut existing = HashSet::new();
        for (root_id, page_id, _) in self.agent_task_roots(project_id)? {
            let blocks = self.page_blocks_all(page_id)?;
            for block in descendants_of(&blocks, root_id) {
                if block.marker.is_none() {
                    continue;
                }
                if let Some(day) = project::first_date_link(&block.content) {
                    existing.insert((
                        day,
                        project::normalized_task_text(&block.content),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        for task in proposed {
            ensure_date(&task.source_day)?;
            let key = (
                task.source_day.clone(),
                project::normalized_task_text(&task.text),
            );
            if existing.insert(key) {
                out.push(task.clone());
            }
        }
        Ok(out)
    }

    fn closable_agent_tasks(&self, updates: &[TaskUpdate]) -> Result<Vec<i64>> {
        let mut out = Vec::new();
        for update in updates {
            if update.marker != Marker::Done {
                return Err(Error::InvalidConsolidation(
                    "only closing an agent task is supported".to_string(),
                ));
            }
            let page_id: i64 = self
                .conn
                .query_row(
                    "SELECT page_id FROM block WHERE id = ?1",
                    [update.block_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(Error::NotFound(update.block_id))?;
            let blocks = self.page_blocks_all(page_id)?;
            let Some(root_id) = top_root_id(&blocks, update.block_id) else {
                continue;
            };
            let Some(root) = blocks.iter().find(|block| block.id == root_id) else {
                continue;
            };
            if !project::is_agent_root(&root.content) {
                return Err(Error::InvalidConsolidation(
                    "a task outside an [agent] root cannot be closed".to_string(),
                ));
            }
            if root_id == update.block_id {
                return Err(Error::InvalidConsolidation(
                    "the task root itself cannot be closed".to_string(),
                ));
            }
            let Some(block) = blocks.iter().find(|block| block.id == update.block_id) else {
                return Err(Error::NotFound(update.block_id));
            };
            if block.marker.is_none() {
                return Err(Error::InvalidConsolidation(
                    "only a marked task can be closed".to_string(),
                ));
            }
            if block.marker != Some(Marker::Done) {
                out.push(update.block_id);
            }
        }
        Ok(out)
    }


    /// Delete a block and everything under it. One undo step restores the whole subtree.
    pub fn delete_block(&mut self, block_id: i64) -> Result<()> {
        self.exec(Action::Delete { block_id })
    }

    /// Persist whether a block's children are folded. This is view state, like a star or a visit: it
    /// stays outside the undo history, so undo never unfolds something the user folded.
    pub fn set_collapsed(&mut self, block_id: i64, collapsed: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE block SET collapsed = ?2, updated_at = ?3 WHERE id = ?1",
            params![block_id, collapsed, now()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(block_id));
        }
        Ok(())
    }

    // ----- history -----------------------------------------------------------------------------

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last change. Returns `false` when there was nothing to undo.
    pub fn undo(&mut self) -> Result<bool> {
        let Some(action) = self.undo.pop() else {
            return Ok(false);
        };
        let inverse = self.apply(&action)?;
        self.redo.push(inverse);
        Ok(true)
    }

    /// Redo the last undone change. Returns `false` when there was nothing to redo.
    pub fn redo(&mut self) -> Result<bool> {
        let Some(action) = self.redo.pop() else {
            return Ok(false);
        };
        let inverse = self.apply(&action)?;
        self.undo.push(inverse);
        Ok(true)
    }

    // ----- queries -----------------------------------------------------------------------------

    /// Every block of a page, flattened in render order with its depth.
    pub fn page_blocks(&self, page_id: i64) -> Result<Vec<BlockRow>> {
        blocks_of_page(&self.conn, page_id, false)
    }

    /// Every block of a page, including descendants of folded blocks. Agent context must see what a
    /// folded view hides: a project's history is not gone because the user folded it.
    pub fn page_blocks_all(&self, page_id: i64) -> Result<Vec<BlockRow>> {
        blocks_of_page(&self.conn, page_id, true)
    }

    /// One journal day, created or not.
    pub fn day(&self, date: &str) -> Result<JournalDay> {
        match self.journal_page(date)? {
            Some(page) => Ok(JournalDay {
                journal_date: date.to_string(),
                page_id: Some(page.id),
                blocks: self.page_blocks(page.id)?,
            }),
            None => Ok(JournalDay {
                journal_date: date.to_string(),
                page_id: None,
                blocks: Vec::new(),
            }),
        }
    }

    /// `count` dates ending at `date`, newest first — the chunk the journal column loads.
    pub fn journal_days_ending_at(&self, date: &str, count: u32) -> Result<Vec<String>> {
        let end = ensure_date(date)?;
        Ok((0..count)
            .map_while(|back| end.checked_sub_days(Days::new(back as u64)))
            .map(|day| day.format("%Y-%m-%d").to_string())
            .collect())
    }

    // ----- internals ---------------------------------------------------------------------------

    fn exec(&mut self, action: Action) -> Result<()> {
        let inverse = self.apply(&action)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    fn apply(&mut self, action: &Action) -> Result<Action> {
        let tx = self.conn.transaction()?;
        let inverse = apply_action(&tx, action)?;
        tx.commit()?;
        Ok(inverse)
    }

    fn next_block_id(&self) -> Result<i64> {
        next_block_id_on(&self.conn)
    }

    fn next_sibling_key(
        &self,
        page_id: i64,
        parent_id: Option<i64>,
        current: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT order_key FROM block
                 WHERE page_id = ?1 AND parent_id IS ?2 AND order_key > ?3
                 ORDER BY order_key LIMIT 1",
                params![page_id, parent_id, current],
                |row| row.get(0),
            )
            .optional()?)
    }
}

fn next_block_id_on(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT IFNULL(MAX(id), 0) + 1 FROM block", [], |row| {
        row.get(0)
    })?)
}

fn last_top_key(conn: &Connection, page_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT order_key FROM block
             WHERE page_id = ?1 AND parent_id IS NULL
             ORDER BY order_key DESC LIMIT 1",
            [page_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn last_child_key(conn: &Connection, parent_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT order_key FROM block WHERE parent_id = ?1 ORDER BY order_key DESC LIMIT 1",
            [parent_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn top_level_blocks(conn: &Connection, page_id: Option<i64>) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    match page_id {
        Some(page_id) => {
            let mut stmt = conn.prepare(
                "SELECT id FROM block WHERE page_id = ?1 AND parent_id IS NULL ORDER BY order_key",
            )?;
            let rows = stmt.query_map([page_id], |row| row.get(0))?;
            for row in rows {
                out.push(row?);
            }
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id FROM block WHERE parent_id IS NULL ORDER BY page_id, order_key",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            for row in rows {
                out.push(row?);
            }
        }
    }
    Ok(out)
}

/// The identity a page name resolves to: trimmed, lowercased.
fn page_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Whether a name is a strict `YYYY-MM-DD` and therefore a journal day, not a page.
pub fn is_date_name(name: &str) -> bool {
    ensure_date(name).is_ok()
}

/// Whether a page name can be written as a `#tag`: one token, no `#` and no brackets.
fn tag_representable(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '#' | '[' | ']'))
}

/// Rewrite the links and tags that resolve to `old_key`, or return `None` when the block names it in no
/// explicit way. Ranges come from the one markdown scanner, so code is already excluded.
fn rewrite_page_links(content: &str, old_key: &str, new_name: &str) -> Option<String> {
    let scan = markdown::scan(content);
    let matches: Vec<&markdown::LinkRef> = scan
        .links
        .iter()
        .filter(|link| {
            matches!(link.kind, LinkKind::Page | LinkKind::Tag)
                && page_key(&link.target) == old_key
        })
        .collect();
    if matches.is_empty() {
        return None;
    }

    let mut rewritten = content.to_string();
    for link in matches.into_iter().rev() {
        let replacement = match link.kind {
            LinkKind::Page => format!("[[{new_name}]]"),
            LinkKind::Tag if tag_representable(new_name) => format!("#{new_name}"),
            LinkKind::Tag => format!("[[{new_name}]]"),
            LinkKind::Url => continue,
        };
        rewritten.replace_range(link.start..link.end, &replacement);
    }
    Some(rewritten)
}

fn page_record(conn: &Connection, page_id: i64) -> Result<PageRecord> {
    conn.query_row(
        "SELECT id, name, name_key, kind, journal_date, favourite, visit_order, created_at, updated_at
         FROM page WHERE id = ?1",
        [page_id],
        |row| {
            Ok(PageRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                name_key: row.get(2)?,
                kind: PageKind::parse(&row.get::<_, String>(3)?),
                journal_date: row.get(4)?,
                favourite: row.get(5)?,
                visit_order: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::NotFound(page_id))
}

fn page_block_records(conn: &Connection, page_id: i64) -> Result<Vec<BlockRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, page_id, parent_id, order_key, content, marker, scheduled_date, collapsed,
                created_at, updated_at
         FROM block WHERE page_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([page_id], |row| {
        Ok(BlockRecord {
            id: row.get(0)?,
            page_id: row.get(1)?,
            parent_id: row.get(2)?,
            order_key: row.get(3)?,
            content: row.get(4)?,
            marker: row.get::<_, Option<String>>(5)?.as_deref().and_then(Marker::parse),
            scheduled_date: row.get(6)?,
            collapsed: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn ensure_page_on(conn: &Connection, name: &str) -> Result<i64> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::BadPageName);
    }
    let key = page_key(trimmed);

    // An existing row wins, whatever its kind: a page that was already named like a date keeps its
    // blocks, and no link is silently re-pointed at a different page.
    if let Some(id) = conn
        .query_row("SELECT id FROM page WHERE name_key = ?1", [&key], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
    {
        return Ok(id);
    }

    // A date is a name for a journal day: `[[2025-11-02]]` means that day, not a page beside the
    // calendar that merely looks like it. One rule here, and links, `open_page` and the placeholder's
    // materialise path all land on the real day.
    if ensure_date(trimmed).is_ok() {
        return ensure_journal_page_on(conn, trimmed);
    }

    let now = now();
    conn.execute(
        "INSERT INTO page (name, name_key, kind, journal_date, created_at, updated_at)
         VALUES (?1, ?2, 'page', NULL, ?3, ?3)",
        params![trimmed, key, now],
    )?;
    Ok(conn.last_insert_rowid())
}

fn ensure_journal_page_on(conn: &Connection, date: &str) -> Result<i64> {
    ensure_date(date)?;
    if let Some(id) = conn
        .query_row("SELECT id FROM page WHERE journal_date = ?1", [date], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    let now = now();
    conn.execute(
        "INSERT INTO page (name, name_key, kind, journal_date, created_at, updated_at)
         VALUES (?1, ?1, 'journal', ?1, ?2, ?2)",
        params![date, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Rebuild a block's derived link rows from its text. Called on every write, inside the same
/// transaction, so the derived state can never lag the text it came from.
fn reindex(conn: &Connection, block_id: i64) -> Result<()> {
    let content: Option<String> = conn
        .query_row("SELECT content FROM block WHERE id = ?1", [block_id], |row| {
            row.get(0)
        })
        .optional()?;
    let Some(content) = content else {
        return Ok(());
    };

    conn.execute("DELETE FROM block_link WHERE block_id = ?1", [block_id])?;
    for link in extract_links(&content) {
        let page_id = ensure_page_on(conn, &link.target)?;
        conn.execute(
            "INSERT OR IGNORE INTO block_link (block_id, page_id, kind) VALUES (?1, ?2, ?3)",
            params![block_id, page_id, link.kind],
        )?;
    }
    Ok(())
}

fn read_block_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<BlockRow> {
    Ok(BlockRow {
        id: row.get(offset)?,
        parent_id: row.get(offset + 1)?,
        depth: 0,
        order_key: row.get(offset + 2)?,
        content: row.get(offset + 3)?,
        marker: row
            .get::<_, Option<String>>(offset + 4)?
            .as_deref()
            .and_then(Marker::parse),
        scheduled_date: row.get(offset + 5)?,
        collapsed: row.get(offset + 6)?,
        child_count: 0,
    })
}

/// Copy an existing graph before a schema step touches it. A fresh file has nothing to copy.
fn copy_before_migration(conn: &Connection, backup_dir: &Path) -> Result<Option<PathBuf>> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 || version >= SCHEMA_VERSION {
        return Ok(None);
    }

    std::fs::create_dir_all(backup_dir).map_err(|error| Error::Backup(error.to_string()))?;
    let stamp = Utc::now().format("%Y-%m-%dT%H%M%S").to_string();
    let name = format!(
        "pre-migration-v{version}-to-v{SCHEMA_VERSION}-{stamp}-{}.db",
        std::process::id()
    );
    let path = backup_dir.join(name);
    conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])
        .map_err(|error| Error::Backup(error.to_string()))?;
    Ok(Some(path))
}

fn migrate(conn: &Connection) -> Result<()> {
    let mut version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        // A newer build wrote this file. Refuse rather than open it and damage it.
        return Err(Error::SchemaTooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    // Each step and the version it claims commit together. A failure leaves the file at the last
    // version that fully applied, so a retry never meets a half-created schema and an ALTER it has
    // already run.
    for (target, sql) in [(1, SCHEMA), (2, SCHEMA_V2), (3, SCHEMA_V3)] {
        if version < target {
            let step = conn.unchecked_transaction()?;
            step.execute_batch(sql)?;
            step.pragma_update(None, "user_version", target)?;
            step.commit()?;
            version = target;
        }
    }
    Ok(())
}

fn apply_action(conn: &Connection, action: &Action) -> Result<Action> {
    match action {
        Action::SetContent { block_id, content } => {
            let old: String = conn
                .query_row("SELECT content FROM block WHERE id = ?1", [block_id], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or(Error::NotFound(*block_id))?;
            conn.execute(
                "UPDATE block SET content = ?2, updated_at = ?3 WHERE id = ?1",
                params![block_id, content, now()],
            )?;
            reindex(conn, *block_id)?;
            Ok(Action::SetContent {
                block_id: *block_id,
                content: old,
            })
        }
        Action::Insert { record } => {
            insert_row(conn, record)?;
            reindex(conn, record.id)?;
            Ok(Action::Delete {
                block_id: record.id,
            })
        }
        Action::Delete { block_id } => {
            let records = subtree(conn, *block_id)?;
            conn.execute("DELETE FROM block WHERE id = ?1", [block_id])?;
            Ok(Action::InsertSubtree { records })
        }
        Action::InsertSubtree { records } => {
            for record in records {
                insert_row(conn, record)?;
                reindex(conn, record.id)?;
            }
            records
                .first()
                .map(|first| Action::Delete {
                    block_id: first.id,
                })
                .ok_or(Error::NotFound(0))
        }
        Action::SetPosition {
            block_id,
            page_id,
            parent_id,
            order_key,
        } => {
            let (old_page, old_parent, old_key): (i64, Option<i64>, String) = conn
                .query_row(
                    "SELECT page_id, parent_id, order_key FROM block WHERE id = ?1",
                    [block_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
                .ok_or(Error::NotFound(*block_id))?;
            conn.execute(
                "UPDATE block
                 SET page_id = ?2, parent_id = ?3, order_key = ?4, updated_at = ?5
                 WHERE id = ?1",
                params![block_id, page_id, parent_id, order_key, now()],
            )?;
            Ok(Action::SetPosition {
                block_id: *block_id,
                page_id: old_page,
                parent_id: old_parent,
                order_key: old_key,
            })
        }
        Action::SetMarker { block_id, marker } => {
            let old: Option<String> = conn
                .query_row("SELECT marker FROM block WHERE id = ?1", [block_id], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or(Error::NotFound(*block_id))?;
            conn.execute(
                "UPDATE block SET marker = ?2, updated_at = ?3 WHERE id = ?1",
                params![block_id, marker.map(Marker::as_str), now()],
            )?;
            Ok(Action::SetMarker {
                block_id: *block_id,
                marker: old.as_deref().and_then(Marker::parse),
            })
        }
        Action::SetScheduled { block_id, date } => {
            let old: Option<String> = conn
                .query_row(
                    "SELECT scheduled_date FROM block WHERE id = ?1",
                    [block_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(Error::NotFound(*block_id))?;
            conn.execute(
                "UPDATE block SET scheduled_date = ?2, updated_at = ?3 WHERE id = ?1",
                params![block_id, date, now()],
            )?;
            Ok(Action::SetScheduled {
                block_id: *block_id,
                date: old,
            })
        }
        Action::RenamePage {
            page_id,
            from_name,
            from_key,
            to_name,
            to_key,
            changes,
        } => {
            let mut old_contents = Vec::with_capacity(changes.len());
            for (block_id, _) in changes {
                let old: String = conn
                    .query_row("SELECT content FROM block WHERE id = ?1", [block_id], |row| {
                        row.get(0)
                    })
                    .optional()?
                    .ok_or(Error::NotFound(*block_id))?;
                old_contents.push((*block_id, old));
            }

            // The page row changes first so the reindex below finds the renamed page instead of
            // creating a second one for the new name.
            let changed = conn.execute(
                "UPDATE page SET name = ?2, name_key = ?3, updated_at = ?4 WHERE id = ?1",
                params![page_id, to_name, to_key, now()],
            )?;
            if changed == 0 {
                return Err(Error::NotFound(*page_id));
            }
            for (block_id, content) in changes {
                conn.execute(
                    "UPDATE block SET content = ?2, updated_at = ?3 WHERE id = ?1",
                    params![block_id, content, now()],
                )?;
                reindex(conn, *block_id)?;
            }

            Ok(Action::RenamePage {
                page_id: *page_id,
                from_name: to_name.clone(),
                from_key: to_key.clone(),
                to_name: from_name.clone(),
                to_key: from_key.clone(),
                changes: old_contents,
            })
        }
        Action::DeletePage { page, blocks } => {
            conn.execute("DELETE FROM page WHERE id = ?1", [page.id])?;
            Ok(Action::RestorePage {
                page: page.clone(),
                blocks: blocks.clone(),
            })
        }
        Action::RestorePage { page, blocks } => {
            conn.execute(
                "INSERT INTO page
                   (id, name, name_key, kind, journal_date, favourite, visit_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    page.id,
                    page.name,
                    page.name_key,
                    match page.kind {
                        PageKind::Journal => "journal",
                        PageKind::Page => "page",
                    },
                    page.journal_date,
                    page.favourite,
                    page.visit_order,
                    page.created_at,
                    page.updated_at
                ],
            )?;
            let mut ordered = blocks.clone();
            ordered.sort_by_key(|block| block.id);
            for record in &ordered {
                insert_row(conn, record)?;
                reindex(conn, record.id)?;
            }
            Ok(Action::DeletePage {
                page: page.clone(),
                blocks: blocks.clone(),
            })
        }
        Action::Composite(actions) => {
            let mut inverses = Vec::with_capacity(actions.len());
            for action in actions {
                inverses.push(apply_action(conn, action)?);
            }
            inverses.reverse();
            Ok(Action::Composite(inverses))
        }
    }
}

fn insert_row(conn: &Connection, record: &BlockRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO block
           (id, page_id, parent_id, order_key, content, marker, scheduled_date, collapsed,
            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            record.id,
            record.page_id,
            record.parent_id,
            record.order_key,
            record.content,
            record.marker.map(Marker::as_str),
            record.scheduled_date,
            record.collapsed,
            record.created_at,
            record.updated_at,
        ],
    )?;
    Ok(())
}

fn block_record(conn: &Connection, id: i64) -> Result<BlockRecord> {
    conn.query_row(
        "SELECT id, page_id, parent_id, order_key, content, marker, scheduled_date, collapsed,
                created_at, updated_at
         FROM block WHERE id = ?1",
        [id],
        |row| {
            Ok(BlockRecord {
                id: row.get(0)?,
                page_id: row.get(1)?,
                parent_id: row.get(2)?,
                order_key: row.get(3)?,
                content: row.get(4)?,
                marker: row.get::<_, Option<String>>(5)?.as_deref().and_then(Marker::parse),
                scheduled_date: row.get(6)?,
                collapsed: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::NotFound(id))
}

/// A block and its descendants, parents before children so they can be re-inserted in order.
fn subtree(conn: &Connection, root: i64) -> Result<Vec<BlockRecord>> {    let mut out = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        out.push(block_record(conn, id)?);
        let mut stmt =
            conn.prepare("SELECT id FROM block WHERE parent_id = ?1 ORDER BY order_key DESC")?;
        let children = stmt
            .query_map([id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        pending.extend(children);
    }
    Ok(out)
}

fn block_ref(block: &BlockRow) -> BlockRef {
    BlockRef {
        id: block.id,
        parent_id: block.parent_id,
        depth: block.depth,
        content: block.content.clone(),
        marker: block.marker,
        scheduled_date: block.scheduled_date.clone(),
    }
}

fn top_root_id(blocks: &[BlockRow], id: i64) -> Option<i64> {
    let mut current = id;
    loop {
        let block = blocks.iter().find(|block| block.id == current)?;
        match block.parent_id {
            Some(parent) => current = parent,
            None => return Some(current),
        }
    }
}

fn ancestors_of(blocks: &[BlockRow], id: i64) -> Vec<BlockRef> {
    let mut out = Vec::new();
    let mut current = blocks
        .iter()
        .find(|block| block.id == id)
        .and_then(|block| block.parent_id);
    while let Some(parent) = current {
        let Some(block) = blocks.iter().find(|block| block.id == parent) else {
            break;
        };
        out.push(block_ref(block));
        current = block.parent_id;
    }
    out.reverse();
    out
}

fn descendants_of(blocks: &[BlockRow], id: i64) -> Vec<BlockRef> {
    let Some(index) = blocks.iter().position(|block| block.id == id) else {
        return Vec::new();
    };
    let depth = blocks[index].depth;
    blocks[index + 1..]
        .iter()
        .take_while(|block| block.depth > depth)
        .map(block_ref)
        .collect()
}

fn direct_children(conn: &Connection, parent_id: i64) -> Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, order_key FROM block WHERE parent_id = ?1 ORDER BY order_key",
    )?;
    Ok(stmt
        .query_map([parent_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn validate_consolidation_text(request: &ConsolidationRequest, page: &Page) -> Result<()> {
    let mut size = 0usize;
    for item in request
        .brief
        .current_state
        .iter()
        .chain(&request.brief.decisions)
        .chain(&request.brief.open_questions)
    {
        check_consolidation_item(&item.text, &item.sources, page, &mut size)?;
    }
    for entry in &request.timeline_entries {
        ensure_date(&entry.date)?;
        if entry.date.as_str() > request.through.as_str() {
            return Err(Error::InvalidConsolidation(
                "a timeline entry is later than the through date".to_string(),
            ));
        }
        check_consolidation_item(&entry.text, std::slice::from_ref(&entry.date), page, &mut size)?;
    }
    for task in &request.tasks_to_create {
        size += task.text.len() + task.source_day.len();
        if size > project::MAX_CONSOLIDATION_TEXT {
            return Err(Error::InvalidConsolidation(
                "the consolidation payload is too large".to_string(),
            ));
        }
        if task.text.trim().is_empty() {
            return Err(Error::InvalidConsolidation(
                "a task cannot be empty".to_string(),
            ));
        }
        ensure_date(&task.source_day)?;
        if task.source_day.as_str() > request.through.as_str() {
            return Err(Error::InvalidConsolidation(
                "a task source is later than the through date".to_string(),
            ));
        }
    }
    Ok(())
}

fn check_consolidation_item(
    text: &str,
    sources: &[String],
    page: &Page,
    size: &mut usize,
) -> Result<()> {
    *size += text.len() + sources.iter().map(String::len).sum::<usize>();
    if *size > project::MAX_CONSOLIDATION_TEXT {
        return Err(Error::InvalidConsolidation(
            "the consolidation payload is too large".to_string(),
        ));
    }
    if text.trim().is_empty() {
        return Err(Error::InvalidConsolidation(
            "a digest item cannot be empty".to_string(),
        ));
    }
    if contains_self_link(text, page) {
        return Err(Error::InvalidConsolidation(
            "the digest cannot link the page it lives on".to_string(),
        ));
    }
    for source in sources {
        ensure_date(source)?;
    }
    Ok(())
}

fn contains_self_link(text: &str, page: &Page) -> bool {
    markdown::scan(text).links.iter().any(|link| {
        matches!(link.kind, LinkKind::Page | LinkKind::Tag)
            && page_key(&link.target) == page.name_key
    })
}

fn ensure_digest_root(
    conn: &Connection,
    page_id: i64,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT id FROM block
         WHERE page_id = ?1 AND parent_id IS NULL AND trim(content) = ?2
         ORDER BY order_key",
    )?;
    let mut roots = stmt
        .query_map(params![page_id, project::DIGEST_HEADING], |row| {
            row.get::<_, i64>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if roots.len() > 1 {
        return Err(Error::InvalidConsolidation(
            "the project page has more than one agent digest section".to_string(),
        ));
    }
    if let Some(root) = roots.pop() {
        return Ok(root);
    }

    let first: Option<String> = conn
        .query_row(
            "SELECT order_key FROM block
             WHERE page_id = ?1 AND parent_id IS NULL
             ORDER BY order_key LIMIT 1",
            [page_id],
            |row| row.get(0),
        )
        .optional()?;
    let key = key_between(None, first.as_deref())?;
    insert_block_at(
        conn,
        page_id,
        None,
        &key,
        project::DIGEST_HEADING,
        None,
        None,
        inverses,
    )
}

fn insert_digest_child_at_start(
    conn: &Connection,
    page_id: i64,
    digest_id: i64,
    content: &str,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let first = direct_children(conn, digest_id)?
        .first()
        .map(|(_, _, key)| key.clone());
    let key = key_between(None, first.as_deref())?;
    insert_block_at(
        conn,
        page_id,
        Some(digest_id),
        &key,
        content,
        None,
        None,
        inverses,
    )
}

fn insert_digest_child_at_end(
    conn: &Connection,
    page_id: i64,
    digest_id: i64,
    content: &str,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let last = direct_children(conn, digest_id)?
        .last()
        .map(|(_, _, key)| key.clone());
    let key = key_between(last.as_deref(), None)?;
    insert_block_at(
        conn,
        page_id,
        Some(digest_id),
        &key,
        content,
        None,
        None,
        inverses,
    )
}

fn insert_block_at(
    conn: &Connection,
    page_id: i64,
    parent_id: Option<i64>,
    order_key: &str,
    content: &str,
    marker: Option<Marker>,
    scheduled_date: Option<String>,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let id = next_block_id_on(conn)?;
    let now = now();
    let record = BlockRecord {
        id,
        page_id,
        parent_id,
        order_key: order_key.to_string(),
        content: content.to_string(),
        marker,
        scheduled_date,
        collapsed: false,
        created_at: now.clone(),
        updated_at: now,
    };
    inverses.push(apply_action(conn, &Action::Insert { record })?);
    Ok(id)
}

fn append_child(
    conn: &Connection,
    page_id: i64,
    parent_id: i64,
    content: &str,
    marker: Option<Marker>,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let last = last_child_key(conn, parent_id)?;
    let key = key_between(last.as_deref(), None)?;
    insert_block_at(
        conn,
        page_id,
        Some(parent_id),
        &key,
        content,
        marker,
        None,
        inverses,
    )
}

fn update_block_content(
    conn: &Connection,
    block_id: i64,
    content: &str,
    inverses: &mut Vec<Action>,
) -> Result<()> {
    let current: String = conn.query_row(
        "SELECT content FROM block WHERE id = ?1",
        [block_id],
        |row| row.get(0),
    )?;
    if current != content {
        inverses.push(apply_action(
            conn,
            &Action::SetContent {
                block_id,
                content: content.to_string(),
            },
        )?);
    }
    Ok(())
}

fn find_or_create_agent_task_root(
    conn: &Connection,
    page_id: i64,
    project_id: i64,
    project_name: &str,
    inverses: &mut Vec<Action>,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT b.id
         FROM block b
         JOIN block_link l ON l.block_id = b.id
         WHERE b.page_id = ?1 AND b.parent_id IS NULL
           AND substr(trim(b.content), 1, 7) = ?2 AND l.page_id = ?3
         ORDER BY b.order_key",
    )?;
    let mut roots = stmt
        .query_map(
            params![page_id, project::AGENT_PREFIX, project_id],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if roots.len() > 1 {
        return Err(Error::InvalidConsolidation(
            "the journal day has more than one matching agent task root".to_string(),
        ));
    }
    if let Some(root) = roots.pop() {
        return Ok(root);
    }

    let first: Option<String> = conn
        .query_row(
            "SELECT order_key FROM block
             WHERE page_id = ?1 AND parent_id IS NULL
             ORDER BY order_key LIMIT 1",
            [page_id],
            |row| row.get(0),
        )
        .optional()?;
    let key = key_between(None, first.as_deref())?;
    let content = format!("{} [[{project_name}]] tasks", project::AGENT_PREFIX);
    insert_block_at(
        conn,
        page_id,
        None,
        &key,
        &content,
        None,
        None,
        inverses,
    )
}


fn blocks_of_page(conn: &Connection, page_id: i64, include_collapsed: bool) -> Result<Vec<BlockRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, order_key, content, marker, scheduled_date, collapsed
         FROM block WHERE page_id = ?1 ORDER BY order_key",
    )?;
    let raw = stmt
        .query_map([page_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, bool>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<RawBlock>>>()?;

    let mut by_parent: HashMap<Option<i64>, Vec<usize>> = HashMap::new();
    for (index, row) in raw.iter().enumerate() {
        by_parent.entry(row.1).or_default().push(index);
    }

    let mut out = Vec::with_capacity(raw.len());
    push_rows(&raw, &by_parent, None, 0, include_collapsed, &mut out);
    Ok(out)
}

fn push_rows(
    raw: &[RawBlock],
    by_parent: &HashMap<Option<i64>, Vec<usize>>,
    parent: Option<i64>,
    depth: u32,
    include_collapsed: bool,
    out: &mut Vec<BlockRow>,
) {
    let Some(indices) = by_parent.get(&parent) else {
        return;
    };
    for index in indices {
        let row = &raw[*index];
        let child_count = by_parent.get(&Some(row.0)).map_or(0, Vec::len);
        out.push(BlockRow {
            id: row.0,
            parent_id: row.1,
            depth,
            order_key: row.2.clone(),
            content: row.3.clone(),
            marker: row.4.as_deref().and_then(Marker::parse),
            scheduled_date: row.5.clone(),
            collapsed: row.6,
            child_count,
        });
        if include_collapsed || !row.6 {
            push_rows(raw, by_parent, Some(row.0), depth + 1, include_collapsed, out);
        }
    }
}

/// A window of `text` around the first match of `needle`, cut at character boundaries and marked with
/// ellipses where it was cut — with the match's own range inside that window.
fn snippet(text: &str, needle: &str) -> (String, Option<(usize, usize)>) {
    const WINDOW: usize = 60;

    let start = text.to_lowercase().find(needle).unwrap_or(0);
    let mut from = start.saturating_sub(WINDOW / 2);
    while from > 0 && !text.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (start + needle.len() + WINDOW / 2).min(text.len());
    while to < text.len() && !text.is_char_boundary(to) {
        to += 1;
    }

    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let lead = out.len();
    let body = text[from..to].trim();
    // Trimming the window can move the match: find it again in what is actually shown.
    let matched = body
        .to_lowercase()
        .find(needle)
        .map(|at| (lead + at, lead + at + needle.len()));
    out.push_str(body);
    if to < text.len() {
        out.push('…');
    }
    (out, matched)
}

fn read_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<Page> {
    Ok(Page {
        id: row.get(0)?,
        name: row.get(1)?,
        name_key: row.get(2)?,
        kind: PageKind::parse(&row.get::<_, String>(3)?),
        journal_date: row.get(4)?,
    })
}

fn ensure_date(date: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| Error::BadDate(date.to_string()))?;
    // chrono parses "2026-4-3" happily; a date that does not round-trip is not the date we accept.
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err(Error::BadDate(date.to_string()));
    }
    Ok(parsed)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Bullet, ParsedPage};

    #[test]
    fn an_import_that_fails_while_writing_leaves_the_graph_unchanged() {
        let mut store = Store::open_in_memory().expect("a store");
        let page = store.ensure_page("Kept").expect("a page");
        store.insert_block(page, None, "keep me").expect("a block");

        let bad = ParsedPage {
            name: String::new(),
            journal_date: None,
            bullets: vec![Bullet {
                depth: 0,
                marker: None,
                scheduled: None,
                content: "should not land".to_string(),
            }],
        };

        assert!(matches!(
            store.import_pages(&[bad], ImportMode::Merge),
            Err(Error::BadPageName)
        ));

        let blocks = store.page_blocks(page).expect("blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "keep me");
        let total: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM block", [], |row| row.get(0))
            .expect("a count");
        assert_eq!(total, 1, "the failed import wrote nothing");
    }
}
