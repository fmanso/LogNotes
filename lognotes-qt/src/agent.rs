//! The JSON surface behind the local agent bridge.
//!
//! The MCP binary is a translator; it never opens the database. This module turns one bridge request into
//! the app's own `Store` calls and one JSON answer. Reads are ordinary queries; the one write is
//! `project_consolidation`, which is one undoable store command.

use chrono::Local;
use lognotes_core::project::{
    BlockRef, Brief, BriefItem, ConsolidationRequest, ConsolidationResult, DigestInfo,
    ProjectContext, ProjectSource, TaskRef, TaskToCreate, TaskUpdate, TimelineEntry,
};
use lognotes_core::{BlockRow, Marker, Page, PageKind, ReferenceRow, SearchResults, Store};
use serde_json::{Value, json};

pub struct AgentOutcome {
    pub result: Result<String, String>,
    pub changed: bool,
}

impl AgentOutcome {
    fn ok(value: Value) -> AgentOutcome {
        AgentOutcome {
            result: Ok(value.to_string()),
            changed: false,
        }
    }

    fn applied(value: Value, changed: bool) -> AgentOutcome {
        AgentOutcome {
            result: Ok(value.to_string()),
            changed,
        }
    }

    fn err(message: impl Into<String>) -> AgentOutcome {
        AgentOutcome {
            result: Err(message.into()),
            changed: false,
        }
    }
}

pub fn audit_line(params: &Value, result_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(result_json).ok()?;
    if value.get("applied") != Some(&Value::Bool(true)) {
        return None;
    }
    let project = params.get("page").and_then(Value::as_str).unwrap_or("?");
    let through = value.get("through").and_then(Value::as_str).unwrap_or("?");
    let sources = value
        .get("source_days")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let created = value
        .get("tasks_created")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let closed = value
        .get("tasks_closed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(format!(
        "agent consolidation: project={project} through={through} source_days={sources} tasks_created={created} tasks_closed={closed}"
    ))
}

pub fn handle(store: &mut Store, method: &str, params: &Value) -> AgentOutcome {
    match method {
        "project_context" => match project_context(store, params) {
            Ok(context) => AgentOutcome::ok(context),
            Err(error) => AgentOutcome::err(error),
        },
        "project_consolidation" => match consolidation(store, params) {
            Ok((result, changed)) => AgentOutcome::applied(result, changed),
            Err(error) => AgentOutcome::err(error),
        },
        "search_notes" => match search(store, params) {
            Ok(value) => AgentOutcome::ok(value),
            Err(error) => AgentOutcome::err(error),
        },
        "get_page" => match page(store, params) {
            Ok(value) => AgentOutcome::ok(value),
            Err(error) => AgentOutcome::err(error),
        },
        "get_day" => match day(store, params) {
            Ok(value) => AgentOutcome::ok(value),
            Err(error) => AgentOutcome::err(error),
        },
        "get_agenda" => match agenda(store) {
            Ok(value) => AgentOutcome::ok(value),
            Err(error) => AgentOutcome::err(error),
        },
        "get_backlinks" => match backlinks(store, params) {
            Ok(value) => AgentOutcome::ok(value),
            Err(error) => AgentOutcome::err(error),
        },
        other => AgentOutcome::err(format!("unknown bridge method: {other}")),
    }
}

fn project_context(store: &Store, params: &Value) -> Result<Value, String> {
    let page = required_str(params, "page")?;
    let since = optional_str(params, "since")?;
    let full = params.get("full").and_then(Value::as_bool).unwrap_or(false);
    let context = store
        .project_context(&page, since.as_deref(), full)
        .map_err(|error| error.to_string())?;
    Ok(context_json(&context))
}

fn consolidation(store: &mut Store, params: &Value) -> Result<(Value, bool), String> {
    let request = consolidation_request(params)?;
    let result = store
        .apply_consolidation(&request)
        .map_err(|error| error.to_string())?;
    let changed = result.applied;
    Ok((consolidation_result_json(&result), changed))
}

fn search(store: &Store, params: &Value) -> Result<Value, String> {
    let query = required_str(params, "query")?;
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 100) as usize)
        .unwrap_or(20);
    let answer = store
        .search(&query, limit, limit)
        .map_err(|error| error.to_string())?;
    Ok(search_json(&answer))
}

fn page(store: &Store, params: &Value) -> Result<Value, String> {
    let name = required_str(params, "name")?;
    let page = store
        .page_by_name(&name)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no page named {name}"))?;
    let blocks = store
        .page_blocks_all(page.id)
        .map_err(|error| error.to_string())?;
    let references = store
        .references(page.id)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "page": page_json(&page),
        "blocks": blocks.iter().map(block_row_json).collect::<Vec<_>>(),
        "references": references.iter().map(reference_json).collect::<Vec<_>>(),
    }))
}

fn day(store: &Store, params: &Value) -> Result<Value, String> {
    let date = required_str(params, "date")?;
    let journal = store.day(&date).map_err(|error| error.to_string())?;
    let blocks = match journal.page_id {
        Some(page_id) => store
            .page_blocks_all(page_id)
            .map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    Ok(json!({
        "date": journal.journal_date,
        "page_id": journal.page_id,
        "blocks": blocks.iter().map(block_row_json).collect::<Vec<_>>(),
    }))
}

fn agenda(store: &Store) -> Result<Value, String> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let rows = store.agenda(&today, 7).map_err(|error| error.to_string())?;
    Ok(json!({
        "today": today,
        "rows": rows.iter().map(|row| json!({
            "block": block_row_json(&row.block),
            "page_name": row.page_name,
            "journal_date": row.journal_date,
            "overdue": row.overdue,
        })).collect::<Vec<_>>(),
    }))
}

fn backlinks(store: &Store, params: &Value) -> Result<Value, String> {
    let name = required_str(params, "page")?;
    let page = store
        .page_by_name(&name)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no page named {name}"))?;
    let references = store
        .references(page.id)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "page": page_json(&page),
        "references": references.iter().map(reference_json).collect::<Vec<_>>(),
    }))
}

fn consolidation_request(params: &Value) -> Result<ConsolidationRequest, String> {
    let page = required_str(params, "page")?;
    let through = required_str(params, "through")?;
    let empty = Value::Null;
    let brief = params.get("brief").unwrap_or(&empty);
    let brief = Brief {
        current_state: brief_items(brief, "current_state")?,
        decisions: brief_items(brief, "decisions")?,
        open_questions: brief_items(brief, "open_questions")?,
    };
    let timeline_entries = array(params, "timeline_entries")?
        .into_iter()
        .map(|entry| {
            Ok(TimelineEntry {
                date: required_str(&entry, "date")?,
                text: required_str(&entry, "text")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let tasks_to_create = array(params, "tasks_to_create")?
        .into_iter()
        .map(|task| {
            Ok(TaskToCreate {
                text: required_str(&task, "text")?,
                source_day: required_str(&task, "source_day")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let task_updates = array(params, "task_updates")?
        .into_iter()
        .map(|update| {
            let block_id = update
                .get("block_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| "task_updates needs block_id".to_string())?;
            let marker = parse_marker(&required_str(&update, "marker")?)?;
            Ok(TaskUpdate { block_id, marker })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ConsolidationRequest {
        page,
        through,
        brief,
        timeline_entries,
        tasks_to_create,
        task_updates,
    })
}

fn brief_items(brief: &Value, key: &str) -> Result<Vec<BriefItem>, String> {
    array(brief, key)?
        .into_iter()
        .map(|item| {
            Ok(BriefItem {
                text: required_str(&item, "text")?,
                sources: array(&item, "sources")?
                    .into_iter()
                    .map(|source| {
                        source
                            .as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "sources must be date strings".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            })
        })
        .collect()
}

fn array(value: &Value, key: &str) -> Result<Vec<Value>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) => Err(format!("{key} must be an array")),
    }
}

fn required_str(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{key} is required"))
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn parse_marker(raw: &str) -> Result<Marker, String> {
    match raw {
        "TODO" => Ok(Marker::Todo),
        "DOING" => Ok(Marker::Doing),
        "DONE" => Ok(Marker::Done),
        _ => Err(format!("unknown marker: {raw}")),
    }
}

fn page_json(page: &Page) -> Value {
    json!({
        "id": page.id,
        "name": page.name,
        "kind": match page.kind { PageKind::Journal => "journal", PageKind::Page => "page" },
        "journal_date": page.journal_date,
    })
}

fn block_json(block: &BlockRef) -> Value {
    json!({
        "id": block.id,
        "parent_id": block.parent_id,
        "depth": block.depth,
        "content": block.content,
        "marker": block.marker.map(Marker::label),
        "scheduled_date": block.scheduled_date,
    })
}

fn block_row_json(block: &BlockRow) -> Value {
    json!({
        "id": block.id,
        "parent_id": block.parent_id,
        "depth": block.depth,
        "content": block.content,
        "marker": block.marker.map(Marker::label),
        "scheduled_date": block.scheduled_date,
        "collapsed": block.collapsed,
        "child_count": block.child_count,
    })
}

fn source_json(source: &ProjectSource) -> Value {
    json!({
        "day": source.day,
        "root": block_json(&source.root),
        "ancestors": source.ancestors.iter().map(block_json).collect::<Vec<_>>(),
        "children": source.children.iter().map(block_json).collect::<Vec<_>>(),
    })
}

fn digest_json(digest: &DigestInfo) -> Value {
    json!({
        "exists": digest.exists,
        "block_id": digest.block_id,
        "last_consolidated": digest.last_consolidated,
        "blocks": digest.blocks.iter().map(block_json).collect::<Vec<_>>(),
        "human_notes": digest.human_notes.iter().map(block_json).collect::<Vec<_>>(),
    })
}

fn task_json(task: &TaskRef) -> Value {
    json!({
        "block_id": task.block_id,
        "text": task.text,
        "marker": task.marker.map(Marker::label),
        "source_day": task.source_day,
        "page_name": task.page_name,
        "journal_date": task.journal_date,
        "agent_owned": task.agent_owned,
    })
}

fn context_json(context: &ProjectContext) -> Value {
    json!({
        "project": page_json(&context.project),
        "digest": digest_json(&context.digest),
        "sources": context.sources.iter().map(source_json).collect::<Vec<_>>(),
        "agent_tasks": context.agent_tasks.iter().map(task_json).collect::<Vec<_>>(),
        "human_open_tasks": context.human_open_tasks.iter().map(task_json).collect::<Vec<_>>(),
        "nothing_new": context.nothing_new,
    })
}

fn consolidation_result_json(result: &ConsolidationResult) -> Value {
    json!({
        "applied": result.applied,
        "through": result.through,
        "source_days": result.source_days,
        "tasks_created": result.tasks_created,
        "tasks_closed": result.tasks_closed,
        "digest_block_id": result.digest_block_id,
        "task_root_block_id": result.task_root_block_id,
    })
}

fn search_json(results: &SearchResults) -> Value {
    json!({
        "pages": results.pages.iter().map(page_json).collect::<Vec<_>>(),
        "blocks": results.blocks.iter().map(|hit| json!({
            "block": block_row_json(&hit.block),
            "page_name": hit.page_name,
            "journal_date": hit.journal_date,
            "snippet": hit.snippet,
            "matched": hit.matched.map(|(start, end)| vec![start, end]),
        })).collect::<Vec<_>>(),
        "total": results.total,
    })
}

fn reference_json(reference: &ReferenceRow) -> Value {
    json!({
        "block": block_row_json(&reference.block),
        "page_name": reference.page_name,
        "journal_date": reference.journal_date,
        "kind": match reference.kind {
            lognotes_core::ReferenceKind::Linked => "linked",
            lognotes_core::ReferenceKind::Mention => "mention",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lognotes_core::Store;
    use serde_json::json;

    #[test]
    fn unknown_methods_are_refused() {
        let mut store = Store::open_in_memory().unwrap();
        let outcome = handle(&mut store, "run_sql", &json!({}));
        assert!(outcome.result.is_err());
        assert!(!outcome.changed);
    }

    #[test]
    fn a_malformed_consolidation_is_refused() {
        let mut store = Store::open_in_memory().unwrap();
        let outcome = handle(
            &mut store,
            "project_consolidation",
            &json!({"page": "Missing", "through": "2026-11-22"}),
        );
        assert!(outcome.result.is_err());
        assert!(!outcome.changed);
    }

    #[test]
    fn the_audit_line_names_the_result_without_the_token() {
        let line = audit_line(
            &json!({"page": "Project Alpha", "token": "secret"}),
            &json!({
                "applied": true,
                "through": "2026-11-22",
                "source_days": ["2026-11-20", "2026-11-21"],
                "tasks_created": 2,
                "tasks_closed": 1
            })
            .to_string(),
        )
        .expect("an audit line");
        assert!(line.contains("project=Project Alpha"));
        assert!(line.contains("through=2026-11-22"));
        assert!(line.contains("source_days=2"));
        assert!(line.contains("tasks_created=2"));
        assert!(line.contains("tasks_closed=1"));
        assert!(!line.contains("secret"));
    }

    #[test]
    fn an_audit_line_reaches_the_log_file() {
        let dir = std::env::temp_dir().join(format!("lognotes-agent-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let log = dir.join("lognotes.log");
        crate::diagnostics::init(&log, &dir.join("lognotes.db"));
        let line = audit_line(
            &json!({"page": "Project Alpha"}),
            &json!({
                "applied": true,
                "through": "2026-11-22",
                "source_days": ["2026-11-20"],
                "tasks_created": 1,
                "tasks_closed": 0
            })
            .to_string(),
        )
        .expect("an audit line");
        crate::diagnostics::info(&line);
        let text = std::fs::read_to_string(&log).expect("the log");
        assert!(text.contains("agent consolidation: project=Project Alpha"));
        assert!(text.contains("tasks_created=1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
