//! Project consolidation: the context an agent reads and the structure it writes.
//!
//! A project page is a named page. Journal blocks that explicitly link it are its sources. The page
//! carries one visible, agent-owned digest section; agent-created tasks live under a marked root on
//! today's journal page. Nothing here stores a second copy of the notes — the digest and tasks are
//! ordinary blocks.

use crate::model::{Marker, Page};

/// The exact top-level heading of the agent-owned digest section.
pub const DIGEST_HEADING: &str = "## Agent digest (auto-generated)";

/// The prefix that marks an agent-owned task root.
pub const AGENT_PREFIX: &str = "[agent]";

/// The child heading whose descendants are the append-only timeline.
pub const TIMELINE_HEADING: &str = "### Timeline";

/// The prefix of the visible last-consolidated line.
pub const LAST_CONSOLIDATED_PREFIX: &str = "Last consolidated:";

/// The largest consolidation payload the store will accept, in bytes of text.
pub const MAX_CONSOLIDATION_TEXT: usize = 64 * 1024;

/// One block as context or a write result sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRef {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub depth: u32,
    pub content: String,
    pub marker: Option<Marker>,
    pub scheduled_date: Option<String>,
}

/// One explicit journal root that links a project, with its framing and subtree.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSource {
    pub day: String,
    pub root: BlockRef,
    pub ancestors: Vec<BlockRef>,
    pub children: Vec<BlockRef>,
}

/// The agent-owned section and the human notes outside it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DigestInfo {
    pub exists: bool,
    pub block_id: Option<i64>,
    pub last_consolidated: Option<String>,
    /// The digest section's descendants, in order, including the last-consolidated line and timeline.
    pub blocks: Vec<BlockRef>,
    /// Every page block outside the digest subtree.
    pub human_notes: Vec<BlockRef>,
}

/// A task the agent may see or own.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRef {
    pub block_id: i64,
    pub text: String,
    pub marker: Option<Marker>,
    pub source_day: Option<String>,
    pub page_name: String,
    pub journal_date: Option<String>,
    pub agent_owned: bool,
}

/// Everything an agent needs to consolidate one project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectContext {
    pub project: Page,
    pub digest: DigestInfo,
    pub sources: Vec<ProjectSource>,
    pub agent_tasks: Vec<TaskRef>,
    pub human_open_tasks: Vec<TaskRef>,
    pub nothing_new: bool,
}

/// A claim in the living brief, with the journal days that support it.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefItem {
    pub text: String,
    pub sources: Vec<String>,
}

/// The living part of the digest. The timeline is separate and append-only.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Brief {
    pub current_state: Vec<BriefItem>,
    pub decisions: Vec<BriefItem>,
    pub open_questions: Vec<BriefItem>,
}

impl Brief {
    pub fn is_empty(&self) -> bool {
        self.current_state.is_empty() && self.decisions.is_empty() && self.open_questions.is_empty()
    }
}

/// One day in the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    pub date: String,
    pub text: String,
}

/// A task the agent proposes for today's journal.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskToCreate {
    pub text: String,
    pub source_day: String,
}

/// A status change for an agent-owned task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskUpdate {
    pub block_id: i64,
    pub marker: Marker,
}

/// One consolidation, as the bridge receives it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidationRequest {
    pub page: String,
    pub through: String,
    pub brief: Brief,
    pub timeline_entries: Vec<TimelineEntry>,
    pub tasks_to_create: Vec<TaskToCreate>,
    pub task_updates: Vec<TaskUpdate>,
}

impl ConsolidationRequest {
    pub fn is_noop(&self) -> bool {
        self.brief.is_empty()
            && self.timeline_entries.is_empty()
            && self.tasks_to_create.is_empty()
            && self.task_updates.is_empty()
    }
}

/// What a consolidation changed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConsolidationResult {
    pub applied: bool,
    pub through: String,
    pub source_days: Vec<String>,
    pub tasks_created: usize,
    pub tasks_closed: usize,
    pub digest_block_id: Option<i64>,
    pub task_root_block_id: Option<i64>,
}

/// Whether a block is the top of an agent-owned task subtree.
pub fn is_agent_root(content: &str) -> bool {
    content.trim_start().starts_with(AGENT_PREFIX)
}

/// The first strict `YYYY-MM-DD` inside `[[...]]` links in `text`.
pub fn first_date_link(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            return None;
        };
        let target = &after[..end];
        if crate::is_date_name(target) {
            return Some(target.to_string());
        }
        rest = &after[end + 2..];
    }
    None
}

/// A task's text with day links removed, for comparing the same task across runs.
pub fn normalized_task_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find("[[") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            out.push_str(&rest[start..]);
            break;
        };
        let target = &after[..end];
        if !crate::is_date_name(target) {
            out.push_str(&rest[start..start + 2 + end + 2]);
        }
        rest = &after[end + 2..];
    }
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Format a task as an agent block linking its source day.
pub fn format_task(text: &str, source_day: &str) -> String {
    let text = text.trim();
    let link = format!("[[{source_day}]]");
    if text.contains(&link) {
        text.to_string()
    } else if text.is_empty() {
        link
    } else {
        format!("{text} {link}")
    }
}

/// Format a timeline entry as a dated block linking its day.
pub fn format_timeline_entry(date: &str, text: &str) -> String {
    let text = text.trim();
    let link = format!("[[{date}]]");
    let body = if text.contains(&link) {
        text.to_string()
    } else if text.is_empty() {
        link.clone()
    } else {
        format!("{text} {link}")
    };
    format!("{date}: {body}")
}

/// Format a brief item with its source-day links, or mark it unsourced.
pub fn format_brief_item(item: &BriefItem) -> String {
    let mut text = item.text.trim().to_string();
    if item.sources.is_empty() {
        if !text.to_lowercase().contains("unsourced") {
            text.push_str(" (unsourced)");
        }
        return text;
    }
    for source in &item.sources {
        let link = format!("[[{source}]]");
        if !text.contains(&link) {
            text.push(' ');
            text.push_str(&link);
        }
    }
    text
}

/// The date at the start of a timeline entry, when it has one.
pub fn timeline_entry_date(content: &str) -> Option<String> {
    let prefix = content.get(..10)?;
    if crate::is_date_name(prefix)
        && content
            .as_bytes()
            .get(10)
            .is_some_and(|c| *c == b':' || *c == b' ')
    {
        return Some(prefix.to_string());
    }
    first_date_link(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_task_text_across_day_links() {
        assert_eq!(
            normalized_task_text("Review cache [[2026-11-20]]"),
            "review cache"
        );
        assert_eq!(
            normalized_task_text("Review  cache   [[2026-11-21]]"),
            "review cache"
        );
    }

    #[test]
    fn formats_a_task_with_one_source_link() {
        assert_eq!(
            format_task("Review cache", "2026-11-20"),
            "Review cache [[2026-11-20]]"
        );
        assert_eq!(
            format_task("Review cache [[2026-11-20]]", "2026-11-20"),
            "Review cache [[2026-11-20]]"
        );
    }

    #[test]
    fn formats_unsourced_brief_items() {
        let item = BriefItem {
            text: "A hunch".to_string(),
            sources: vec![],
        };
        assert_eq!(format_brief_item(&item), "A hunch (unsourced)");
    }

    #[test]
    fn reads_a_timeline_date() {
        assert_eq!(
            timeline_entry_date("2026-11-20: shipped [[2026-11-20]]").as_deref(),
            Some("2026-11-20")
        );
    }
}
