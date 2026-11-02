//! Carrying notes out of the database and back in, as the folder of Markdown Logseq 0.x writes.
//!
//! One file per page in `pages/`, one per day in `journals/`, blocks as nested bullets with their markers
//! and `SCHEDULED` lines. The point is that the notes stay readable without this app — so the writer emits
//! the shape Logseq itself reads, and the reader is tolerant of the shapes a real Logseq folder holds
//! (properties it does not understand, tabs, deeper indentation, deadlines).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{Marker, PageKind};
use crate::store::Store;

/// What an export or an import did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Moved {
    pub pages: usize,
    pub days: usize,
    pub blocks: usize,
}

/// Write every page and every written day as Markdown under `directory`.
pub fn export_markdown(store: &Store, directory: &Path) -> Result<Moved> {
    let pages_dir = directory.join("pages");
    let journals_dir = directory.join("journals");
    for dir in [&pages_dir, &journals_dir] {
        fs::create_dir_all(dir).map_err(|error| Error::Backup(error.to_string()))?;
    }

    let mut moved = Moved::default();

    for page in store.pages()? {
        if page.kind != PageKind::Page {
            continue;
        }
        let blocks = store.page_blocks(page.id)?;
        let text = render_page(&blocks);
        let path = pages_dir.join(format!("{}.md", file_name(&page.name)));
        fs::write(&path, text).map_err(|error| Error::Backup(error.to_string()))?;
        moved.pages += 1;
        moved.blocks += blocks.len();
    }

    for page in store.journal_pages()? {
        let Some(date) = page.journal_date.as_deref() else {
            continue;
        };
        let blocks = store.page_blocks(page.id)?;
        let text = render_page(&blocks);
        let path = journals_dir.join(format!("{}.md", date.replace('-', "_")));
        fs::write(&path, text).map_err(|error| Error::Backup(error.to_string()))?;
        moved.days += 1;
        moved.blocks += blocks.len();
    }

    Ok(moved)
}

/// Read a Logseq 0.x folder — `journals/` and `pages/` of nested bullets — into the store.
pub fn import_markdown(store: &mut Store, directory: &Path) -> Result<Moved> {
    let mut moved = Moved::default();

    for file in markdown_files(&directory.join("journals"))? {
        let Some(date) = day_of(&file) else {
            continue;
        };
        let page = store.ensure_journal_page(&date)?;
        moved.blocks += write_blocks(store, page, &file)?;
        moved.days += 1;
    }

    for file in markdown_files(&directory.join("pages"))? {
        let Some(name) = file.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let name = name.replace("%2F", "/").replace("%3A", ":");
        if name.trim().is_empty() {
            continue;
        }
        let page = store.ensure_page(&name)?;
        moved.blocks += write_blocks(store, page, &file)?;
        moved.pages += 1;
    }

    Ok(moved)
}

/// The `.md` files in a folder, sorted so a run is repeatable.
fn markdown_files(directory: &Path) -> Result<Vec<PathBuf>> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    files.sort();
    Ok(files)
}

/// `journals/2026_11_02.md` — and Logseq's other spellings of the same day.
fn day_of(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_str()?;
    let normalised = stem.replace('_', "-").replace('.', "-");
    let date = chrono::NaiveDate::parse_from_str(&normalised, "%Y-%m-%d").ok()?;
    Some(date.format("%Y-%m-%d").to_string())
}

/// One page as Markdown: a bullet per block, two spaces per level.
fn render_page(blocks: &[crate::BlockRow]) -> String {
    let mut out = String::new();
    for block in blocks {
        let indent = "  ".repeat(block.depth as usize);
        let marker = block.marker.map(Marker::label).unwrap_or("");
        let mut lines = block.content.split('\n');

        let first = lines.next().unwrap_or("");
        out.push_str(&indent);
        out.push_str("- ");
        if !marker.is_empty() {
            out.push_str(marker);
            out.push(' ');
        }
        out.push_str(first);
        out.push('\n');

        // What follows the first line stays inside the block, indented under the bullet.
        for line in lines {
            out.push_str(&indent);
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }

        if let Some(date) = block.scheduled_date.as_deref() {
            out.push_str(&indent);
            out.push_str("  SCHEDULED: <");
            out.push_str(date);
            out.push_str(">\n");
        }
    }
    out
}

/// One bullet per block, with its children nested under it.
struct Bullet {
    depth: usize,
    marker: Option<Marker>,
    scheduled: Option<String>,
    content: String,
}

/// Read a page's bullets and write them as blocks, returning how many blocks were written.
fn write_blocks(store: &mut Store, page: i64, file: &Path) -> Result<usize> {
    let text = fs::read_to_string(file).map_err(|error| Error::Backup(error.to_string()))?;
    let bullets = parse(&text);

    // The last block written at each depth, so a bullet becomes the next sibling of the one before it at
    // its own level, or the child of the nearest shallower one.
    let mut written = 0;
    let mut last_at_depth: HashMap<usize, i64> = HashMap::new();
    let mut order: Vec<i64> = Vec::new();

    for bullet in bullets {
        let after = if bullet.depth == 0 {
            order.last().copied()
        } else {
            last_at_depth.get(&(bullet.depth - 1)).copied()
        };
        let id = store.insert_block(page, after, &bullet.content)?;
        if bullet.depth == 0 {
            order.push(id);
        } else {
            // `after` places it beside the nearest shallower block; one indent makes it that block's child,
            // which is the depth the file said.
            store.indent(id)?;
        }
        if let Some(marker) = bullet.marker {
            store.set_marker(id, Some(marker))?;
        }
        if let Some(date) = bullet.scheduled.as_deref() {
            store.set_scheduled(id, Some(date))?;
        }
        last_at_depth.insert(bullet.depth, id);
        written += 1;
    }

    Ok(written)
}

/// The bullets in a Markdown page, tolerantly: tabs or spaces, any indent step, properties it does not
/// know, and continuation lines that belong to the bullet above them.
fn parse(text: &str) -> Vec<Bullet> {
    let mut bullets: Vec<Bullet> = Vec::new();
    let mut current: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        let indent = line.len() - trimmed.len();
        let indent = line[..indent].chars().fold(0, |width, character| {
            width + if character == '\t' { 2 } else { 1 }
        });

        // A bullet starts a block; everything else either belongs to it or is a property of it.
        let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        else {
            let Some(index) = current else {
                continue;
            };
            if let Some(date) = scheduled_in(trimmed) {
                bullets[index].scheduled = Some(date);
            } else if !is_property(trimmed) {
                // A continuation line: part of the block above. Only the bullet's own indent is
                // removed — what is left is the line's own indentation, which is what keeps a code
                // sample inside a block readable.
                let own = bullets[index].depth * 2 + 2;
                let keep: String = line.chars().skip(own).collect();
                if !bullets[index].content.is_empty() {
                    bullets[index].content.push('\n');
                }
                bullets[index].content.push_str(keep.trim_end());
            }
            continue;
        };

        let (marker, rest) = split_marker(rest);
        bullets.push(Bullet {
            depth: indent / 2,
            marker,
            scheduled: None,
            content: rest.to_string(),
        });
        current = Some(bullets.len() - 1);
    }

    bullets
}

/// A Logseq task marker at the start of a bullet, mapped onto the three this app keeps.
fn split_marker(text: &str) -> (Option<Marker>, &str) {
    let (word, rest) = text.split_once(' ').unwrap_or((text, ""));
    let marker = match word {
        "TODO" | "LATER" | "WAITING" => Some(Marker::Todo),
        "DOING" | "NOW" => Some(Marker::Doing),
        "DONE" | "CANCELED" | "CANCELLED" => Some(Marker::Done),
        _ => None,
    };
    if marker.is_some() {
        (marker, rest)
    } else {
        (None, text)
    }
}

/// `SCHEDULED: <2026-11-05>` in any of the spellings Logseq writes.
fn scheduled_in(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("SCHEDULED:")
        .or_else(|| line.strip_prefix("scheduled:"))?
        .trim();
    let date = rest.trim_start_matches('<').split('>').next()?.trim();
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(parsed.format("%Y-%m-%d").to_string())
}

/// A block property this app does not keep: Logseq's `key:: value`, and its date properties, which use
/// a single colon.
fn is_property(line: &str) -> bool {
    if line.starts_with("DEADLINE:") || line.starts_with("deadline:") {
        return true;
    }
    let Some((key, _)) = line.split_once("::") else {
        return false;
    };
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_alphanumeric() || character == '-' || character == '_')
}

/// A page name as a file name: the characters a filesystem refuses are escaped the way Logseq escapes them.
fn file_name(name: &str) -> String {
    name.replace('/', "%2F").replace(':', "%3A").replace('\\', "%5C")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a directory");
        path
    }

    #[test]
    fn a_page_is_written_as_nested_bullets_with_markers_and_schedules() {
        let blocks = vec![
            crate::BlockRow {
                id: 1,
                parent_id: None,
                depth: 0,
                order_key: String::new(),
                content: "pay the bill".to_string(),
                marker: Some(Marker::Todo),
                scheduled_date: Some("2026-11-05".to_string()),
                collapsed: false,
            },
            crate::BlockRow {
                id: 2,
                parent_id: Some(1),
                depth: 1,
                order_key: String::new(),
                content: "for the studio".to_string(),
                marker: None,
                scheduled_date: None,
                collapsed: false,
            },
        ];

        let text = render_page(&blocks);

        assert_eq!(
            text,
            "- TODO pay the bill\n  SCHEDULED: <2026-11-05>\n  - for the studio\n"
        );
    }

    #[test]
    fn a_multi_line_block_stays_one_block() {
        let blocks = vec![crate::BlockRow {
            id: 1,
            parent_id: None,
            depth: 0,
            order_key: String::new(),
            content: "```python\ndef f():\n    return 1\n```".to_string(),
            marker: None,
            scheduled_date: None,
            collapsed: false,
        }];

        let text = render_page(&blocks);
        let read_back = parse(&text);

        assert_eq!(read_back.len(), 1, "one bullet, not four");
        assert_eq!(read_back[0].content, "```python\ndef f():\n    return 1\n```");
    }

    #[test]
    fn properties_and_deadlines_are_tolerated() {
        let text = "\
- a note\n  id:: 65f0\n  collapsed:: true\n  DEADLINE: <2026-11-09>\n- LATER another\n";
        let bullets = parse(text);

        assert_eq!(bullets.len(), 2);
        assert_eq!(bullets[0].content, "a note", "properties are not content");
        assert_eq!(bullets[1].marker, Some(Marker::Todo), "LATER is a todo");
    }

    #[test]
    fn tabs_and_deeper_indentation_still_nest() {
        let text = "- one\n\t- two\n    - three\n";
        let bullets = parse(text);

        assert_eq!(bullets.iter().map(|b| b.depth).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn a_journal_file_names_its_day() {
        assert_eq!(
            day_of(Path::new("journals/2026_11_02.md")).as_deref(),
            Some("2026-11-02")
        );
        assert_eq!(
            day_of(Path::new("journals/2026-11-02.md")).as_deref(),
            Some("2026-11-02")
        );
        assert_eq!(day_of(Path::new("journals/notes.md")), None);
    }
}
