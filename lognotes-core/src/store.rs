//! The SQLite store: schema, commands, queries, and the session undo history.
//!
//! Every mutation goes through `Store::exec`, which is the single point that records an inverse action.
//! Nothing else may write, so no command can forget to be undoable.

use std::collections::HashMap;
use std::path::Path;

use chrono::{Days, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::model::{
    AgendaRow, BlockRecord, BlockRow, JournalDay, Marker, Page, PageKind, ReferenceKind, ReferenceRow,
};
use crate::order::key_between;
use crate::text::{extract_links, mentions_whole_words};

const SCHEMA_VERSION: i64 = 2;

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
    Composite(Vec<Action>),
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
}

impl Store {
    /// Open (or create) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        Store::from_connection(Connection::open(path)?)
    }

    /// An ephemeral store, used by tests.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Store> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        migrate(&conn)?;
        Ok(Store {
            conn,
            undo: Vec::new(),
            redo: Vec::new(),
        })
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

    /// Every named page, for the page switcher. Journal days are not pages you navigate to.
    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, name_key, kind, journal_date FROM page
             WHERE kind = 'page' ORDER BY name_key",
        )?;
        let pages = stmt
            .query_map([], read_page)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pages)
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
        ensure_date(date)?;
        if let Some(page) = self.journal_page(date)? {
            return Ok(page.id);
        }
        let now = now();
        self.conn.execute(
            "INSERT INTO page (name, name_key, kind, journal_date, created_at, updated_at)
             VALUES (?1, ?1, 'journal', ?1, ?2, ?2)",
            params![date, now],
        )?;
        Ok(self.conn.last_insert_rowid())
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

    /// Schedule a block for `date`, or clear the date with `None`.
    ///
    /// Scheduling a block that is not a task also marks it TODO: the agenda lists tasks, so a date on an
    /// untasked block would silently never appear. Both writes are one action, so one undo steps back over
    /// the whole thing.
    pub fn set_scheduled(&mut self, block_id: i64, date: Option<&str>) -> Result<()> {
        if let Some(date) = date {
            ensure_date(date)?;
        }
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

        if actions.len() == 1 {
            self.exec(actions.remove(0))
        } else {
            self.exec(Action::Composite(actions))
        }
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

    /// Delete a block and everything under it. One undo step restores the whole subtree.
    pub fn delete_block(&mut self, block_id: i64) -> Result<()> {
        self.exec(Action::Delete { block_id })
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
        let mut stmt = self.conn.prepare(
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
        push_rows(&raw, &by_parent, None, 0, &mut out);
        Ok(out)
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
        Ok(self
            .conn
            .query_row("SELECT IFNULL(MAX(id), 0) + 1 FROM block", [], |row| {
                row.get(0)
            })?)
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

fn ensure_page_on(conn: &Connection, name: &str) -> Result<i64> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::BadPageName);
    }
    let key = trimmed.to_lowercase();

    if let Some(id) = conn
        .query_row("SELECT id FROM page WHERE name_key = ?1", [&key], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
    {
        return Ok(id);
    }

    let now = now();
    conn.execute(
        "INSERT INTO page (name, name_key, kind, journal_date, created_at, updated_at)
         VALUES (?1, ?2, 'page', NULL, ?3, ?3)",
        params![trimmed, key, now],
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
    })
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        // A newer build wrote this file. Refuse rather than open it and damage it.
        return Err(Error::SchemaTooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version < 1 {
        conn.execute_batch(SCHEMA)?;
    }
    if version < 2 {
        conn.execute_batch(SCHEMA_V2)?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
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
            parent_id,
            order_key,
        } => {
            let (old_parent, old_key): (Option<i64>, String) = conn
                .query_row(
                    "SELECT parent_id, order_key FROM block WHERE id = ?1",
                    [block_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or(Error::NotFound(*block_id))?;
            conn.execute(
                "UPDATE block SET parent_id = ?2, order_key = ?3, updated_at = ?4 WHERE id = ?1",
                params![block_id, parent_id, order_key, now()],
            )?;
            Ok(Action::SetPosition {
                block_id: *block_id,
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
fn subtree(conn: &Connection, root: i64) -> Result<Vec<BlockRecord>> {
    let mut out = Vec::new();
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

fn push_rows(
    raw: &[RawBlock],
    by_parent: &HashMap<Option<i64>, Vec<usize>>,
    parent: Option<i64>,
    depth: u32,
    out: &mut Vec<BlockRow>,
) {
    let Some(indices) = by_parent.get(&parent) else {
        return;
    };
    for index in indices {
        let row = &raw[*index];
        out.push(BlockRow {
            id: row.0,
            parent_id: row.1,
            depth,
            order_key: row.2.clone(),
            content: row.3.clone(),
            marker: row.4.as_deref().and_then(Marker::parse),
            scheduled_date: row.5.clone(),
            collapsed: row.6,
        });
        push_rows(raw, by_parent, Some(row.0), depth + 1, out);
    }
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
