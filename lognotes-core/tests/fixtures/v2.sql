-- A version 2 graph: v1 plus the FTS triggers, with the same sample data.
PRAGMA user_version = 2;

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

INSERT INTO page (id, name, name_key, kind, journal_date, created_at, updated_at) VALUES
  (1, 'My Project', 'my project', 'page', NULL, '2026-11-02T08:00:00Z', '2026-11-02T08:00:00Z'),
  (2, '2026-11-02', '2026-11-02', 'journal', '2026-11-02', '2026-11-02T08:00:00Z', '2026-11-02T08:00:00Z'),
  (3, 'journal', 'journal', 'page', NULL, '2026-11-02T08:00:00Z', '2026-11-02T08:00:00Z');

INSERT INTO block (id, page_id, parent_id, order_key, content, marker, scheduled_date, collapsed, created_at, updated_at) VALUES
  (1, 1, NULL, 'a', 'the project home', NULL, NULL, 0, '2026-11-02T08:00:00Z', '2026-11-02T08:00:00Z'),
  (2, 2, NULL, 'a', 'working on [[My Project]] #journal', 'TODO', '2026-11-05', 0, '2026-11-02T08:01:00Z', '2026-11-02T08:01:00Z');

INSERT INTO block_link (block_id, page_id, kind) VALUES
  (2, 1, 'link'),
  (2, 3, 'tag');

INSERT INTO block_fts (block_fts) VALUES ('rebuild');
