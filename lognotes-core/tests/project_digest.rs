use chrono::{Days, Local, NaiveDate};
use lognotes_core::project::{
    AGENT_PREFIX, DIGEST_HEADING, LAST_CONSOLIDATED_PREFIX, TIMELINE_HEADING,
};
use lognotes_core::{
    Brief, BriefItem, ConsolidationRequest, Error, Marker, Store, TaskToCreate, TaskUpdate,
    TimelineEntry,
};

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn day(back: u64) -> String {
    today()
        .checked_sub_days(Days::new(back))
        .expect("a past day")
        .format("%Y-%m-%d")
        .to_string()
}

fn child(store: &mut Store, page: i64, parent: i64, text: &str) -> i64 {
    let id = store
        .insert_block(page, Some(parent), text)
        .expect("a child");
    store.indent(id).expect("indent the child");
    id
}

fn setup_project() -> (Store, i64, String, String) {
    let mut store = Store::open_in_memory().expect("a store");
    let project = store.ensure_page("Project Alpha").expect("a project");
    store
        .insert_block(project, None, "Keep this human note")
        .expect("a human note");

    let older = day(2);
    let newer = day(1);
    let older_page = store.ensure_journal_page(&older).expect("an older day");
    let root = store
        .insert_block(older_page, None, "Worked on [[Project Alpha]]")
        .expect("a root");
    child(
        &mut store,
        older_page,
        root,
        "decided the cache belongs in core",
    );
    child(&mut store, older_page, root, "open question: invalidation");

    let newer_page = store.ensure_journal_page(&newer).expect("a newer day");
    store
        .insert_block(newer_page, None, "[[Project Alpha]] importer")
        .expect("a second root");

    (store, project, older, newer)
}

fn request(through: &str, older: &str) -> ConsolidationRequest {
    ConsolidationRequest {
        page: "Project Alpha".to_string(),
        through: through.to_string(),
        brief: Brief {
            current_state: vec![BriefItem {
                text: "The cache lives in core".to_string(),
                sources: vec![older.to_string()],
            }],
            decisions: vec![BriefItem {
                text: "SQLite is the source of truth".to_string(),
                sources: vec![older.to_string()],
            }],
            open_questions: vec![BriefItem {
                text: "How is the cache invalidated?".to_string(),
                sources: vec![older.to_string()],
            }],
        },
        timeline_entries: vec![TimelineEntry {
            date: older.to_string(),
            text: "Decided the cache belongs in core".to_string(),
        }],
        tasks_to_create: vec![TaskToCreate {
            text: "Review cache invalidation".to_string(),
            source_day: older.to_string(),
        }],
        task_updates: Vec::new(),
    }
}

#[test]
fn context_has_roots_children_and_framing_but_not_self_agent_or_mentions() {
    let (mut store, project, older, newer) = setup_project();

    let older_page = store.journal_page(&older).unwrap().unwrap().id;
    let morning = store.insert_block(older_page, None, "Morning").unwrap();
    let nested = child(
        &mut store,
        older_page,
        morning,
        "Worked on [[Project Alpha]]",
    );
    child(&mut store, older_page, nested, "nested detail");

    let mention_day = day(0);
    let mention_page = store.ensure_journal_page(&mention_day).unwrap();
    store
        .insert_block(mention_page, None, "Project Alpha is nice")
        .unwrap();

    let task_day = day(0);
    let task_page = store.ensure_journal_page(&task_day).unwrap();
    let task_root = store
        .insert_block(
            task_page,
            None,
            &format!("{AGENT_PREFIX} [[Project Alpha]] tasks"),
        )
        .unwrap();
    let task = child(&mut store, task_page, task_root, "TODO review cache");
    store.set_marker(task, Some(Marker::Todo)).unwrap();

    store
        .insert_block(project, None, "[[Project Alpha]] self mention")
        .unwrap();

    let context = store.project_context("Project Alpha", None, false).unwrap();
    assert!(!context.digest.exists);
    assert_eq!(context.sources.len(), 3);
    assert!(context.sources.iter().any(|source| {
        source.day == older
            && source.root.content.contains("Worked on")
            && source.root.id == nested
            && source
                .ancestors
                .iter()
                .any(|ancestor| ancestor.content == "Morning")
            && source
                .children
                .iter()
                .any(|child| child.content == "nested detail")
    }));
    assert!(context.sources.iter().any(|source| {
        source.day == newer && source.root.content == "[[Project Alpha]] importer"
    }));
    assert!(
        context
            .sources
            .iter()
            .all(|source| source.day != mention_day)
    );
    assert!(
        context
            .sources
            .iter()
            .all(|source| source.root.id != task_root)
    );
    assert_eq!(context.agent_tasks.len(), 1);
    assert_eq!(context.agent_tasks[0].text, "TODO review cache");
    assert!(context.agent_tasks[0].source_day.is_none());
    assert!(context.human_open_tasks.is_empty());
    assert!(
        context
            .digest
            .human_notes
            .iter()
            .any(|note| note.content.contains("self mention"))
    );

    let filtered = store
        .project_context("Project Alpha", Some(&older), false)
        .unwrap();
    assert_eq!(filtered.sources.len(), 1);
    assert_eq!(filtered.sources[0].day, newer);
    let full = store
        .project_context("Project Alpha", Some(&newer), true)
        .unwrap();
    assert_eq!(full.sources.len(), 3);
}

#[test]
fn digest_section_exposes_last_date_and_hides_from_human_notes() {
    let (mut store, project, older, _) = setup_project();
    let digest = store.insert_block(project, None, DIGEST_HEADING).unwrap();
    let last = child(
        &mut store,
        project,
        digest,
        &format!("{LAST_CONSOLIDATED_PREFIX} [[{older}]]"),
    );
    let current = child(&mut store, project, digest, "### Current state");
    child(&mut store, project, current, "A current fact");
    let timeline = child(&mut store, project, digest, TIMELINE_HEADING);
    child(
        &mut store,
        project,
        timeline,
        &format!("{older}: something [[{older}]]"),
    );

    let context = store.project_context("Project Alpha", None, false).unwrap();
    assert!(context.digest.exists);
    assert_eq!(context.digest.block_id, Some(digest));
    assert_eq!(
        context.digest.last_consolidated.as_deref(),
        Some(older.as_str())
    );
    assert!(context.digest.blocks.iter().any(|block| block.id == last));
    assert!(
        context
            .digest
            .blocks
            .iter()
            .any(|block| block.content == "A current fact")
    );
    assert!(
        context
            .digest
            .human_notes
            .iter()
            .any(|note| note.content == "Keep this human note")
    );
    assert!(
        !context
            .digest
            .human_notes
            .iter()
            .any(|note| note.content == "A current fact")
    );

    store.set_collapsed(digest, true).unwrap();
    let folded = store.project_context("Project Alpha", None, false).unwrap();
    assert!(
        folded
            .digest
            .blocks
            .iter()
            .any(|block| block.content == "A current fact")
    );

    store.insert_block(project, None, DIGEST_HEADING).unwrap();
    assert!(matches!(
        store.project_context("Project Alpha", None, false),
        Err(Error::InvalidConsolidation(_))
    ));
}

#[test]
fn a_malformed_last_consolidated_date_is_treated_as_absent() {
    let (mut store, project, _older, _) = setup_project();
    let digest = store.insert_block(project, None, DIGEST_HEADING).unwrap();
    child(
        &mut store,
        project,
        digest,
        &format!("{LAST_CONSOLIDATED_PREFIX} not-a-date"),
    );
    let context = store.project_context("Project Alpha", None, false).unwrap();
    assert!(context.digest.exists);
    assert_eq!(context.digest.last_consolidated, None);
}

#[test]
fn consolidation_writes_digest_tasks_and_one_undo() {
    let (mut store, project, older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    let result = store
        .apply_consolidation(&request(&through, &older))
        .expect("a consolidation");
    assert!(result.applied);
    assert_eq!(result.tasks_created, 1);
    assert_eq!(result.tasks_closed, 0);
    assert!(result.source_days.contains(&older));

    let page = store.page_blocks_all(project).unwrap();
    let digest = page
        .iter()
        .find(|block| block.content == DIGEST_HEADING)
        .expect("a digest");
    assert_eq!(
        page.iter().filter(|b| b.content == DIGEST_HEADING).count(),
        1
    );
    assert!(
        page.iter()
            .any(|b| b.content.starts_with(LAST_CONSOLIDATED_PREFIX))
    );
    assert!(page.iter().any(|b| b.content == "### Current state"));
    assert!(
        page.iter()
            .any(|b| b.content.contains("The cache lives in core"))
    );
    assert!(
        page.iter()
            .any(|b| b.content.contains(&format!("[[{older}]]")))
    );
    assert!(page.iter().any(|b| b.content == TIMELINE_HEADING));
    assert!(
        page.iter()
            .any(|b| b.content.starts_with(&format!("{older}: ")))
    );

    let human = page
        .iter()
        .find(|block| block.content == "Keep this human note")
        .expect("the human note");
    assert_eq!(human.depth, 0);

    let today_page = store.journal_page(&through).unwrap().expect("today's page");
    let today_blocks = store.page_blocks_all(today_page.id).unwrap();
    let task_root = today_blocks
        .iter()
        .find(|block| block.content == "[agent] [[Project Alpha]] tasks")
        .expect("the task root");
    assert_eq!(task_root.depth, 0);
    let task = today_blocks
        .iter()
        .find(|block| block.content.contains("Review cache invalidation"))
        .expect("the task");
    assert_eq!(task.marker, Some(Marker::Todo));
    assert!(task.scheduled_date.is_none());
    assert!(task.content.contains(&format!("[[{older}]]")));

    assert!(store.undo().unwrap());
    let after_undo = store.page_blocks_all(project).unwrap();
    assert!(
        !after_undo
            .iter()
            .any(|block| block.content == DIGEST_HEADING)
    );
    assert!(
        after_undo
            .iter()
            .any(|block| block.content == "Keep this human note")
    );
    let tasks_after_undo = store.page_blocks_all(today_page.id).unwrap();
    assert!(tasks_after_undo.is_empty());
    let _ = digest;
}

#[test]
fn repeat_consolidation_does_not_duplicate_timeline_or_tasks() {
    let (mut store, project, older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    let req = request(&through, &older);
    store.apply_consolidation(&req).unwrap();
    let second = store.apply_consolidation(&req).unwrap();
    assert_eq!(second.tasks_created, 0);
    assert_eq!(second.tasks_closed, 0);

    let page = store.page_blocks_all(project).unwrap();
    assert_eq!(
        page.iter()
            .filter(|block| block.content.starts_with(&format!("{older}: ")))
            .count(),
        1
    );
    let today_page = store.journal_page(&through).unwrap().unwrap();
    let tasks = store.page_blocks_all(today_page.id).unwrap();
    assert_eq!(
        tasks
            .iter()
            .filter(|block| block.content.contains("Review cache invalidation"))
            .count(),
        1
    );
}

#[test]
fn agent_tasks_are_deduped_across_days_and_done_tasks_stay_done() {
    let (mut store, _project, older, _) = setup_project();
    let older_page = store.journal_page(&older).unwrap().unwrap().id;
    let root = store
        .insert_block(
            older_page,
            None,
            &format!("{AGENT_PREFIX} [[Project Alpha]] tasks"),
        )
        .unwrap();
    let task = child(
        &mut store,
        older_page,
        root,
        &format!("Review cache invalidation [[{older}]]"),
    );
    store.set_marker(task, Some(Marker::Todo)).unwrap();

    let through = today().format("%Y-%m-%d").to_string();
    let result = store
        .apply_consolidation(&request(&through, &older))
        .expect("a consolidation");
    assert_eq!(result.tasks_created, 0);

    store.set_marker(task, Some(Marker::Done)).unwrap();
    let again = store
        .apply_consolidation(&request(&through, &older))
        .expect("a consolidation");
    assert_eq!(again.tasks_created, 0);
    assert_eq!(
        store
            .page_blocks_all(older_page)
            .unwrap()
            .iter()
            .filter(|block| block.content.contains("Review cache invalidation"))
            .count(),
        1
    );
}

#[test]
fn closing_an_agent_task_changes_only_its_marker() {
    let (mut store, _project, older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    store
        .apply_consolidation(&request(&through, &older))
        .expect("a consolidation");
    let today_page = store.journal_page(&through).unwrap().unwrap().id;
    let task = store
        .page_blocks_all(today_page)
        .unwrap()
        .into_iter()
        .find(|block| block.content.contains("Review cache invalidation"))
        .expect("the task");
    let before = task.content.clone();

    let req = ConsolidationRequest {
        page: "Project Alpha".to_string(),
        through: through.clone(),
        brief: Brief::default(),
        timeline_entries: Vec::new(),
        tasks_to_create: Vec::new(),
        task_updates: vec![TaskUpdate {
            block_id: task.id,
            marker: Marker::Done,
        }],
    };
    let result = store.apply_consolidation(&req).expect("a close");
    assert_eq!(result.tasks_closed, 1);
    let after = store
        .page_blocks_all(today_page)
        .unwrap()
        .into_iter()
        .find(|block| block.id == task.id)
        .expect("the task");
    assert_eq!(after.marker, Some(Marker::Done));
    assert_eq!(after.content, before);
}

#[test]
fn human_tasks_cannot_be_closed_by_an_agent() {
    let (mut store, _project, older, _) = setup_project();
    let page = store.journal_page(&older).unwrap().unwrap().id;
    let human = store
        .insert_block(page, None, "TODO human task [[Project Alpha]]")
        .unwrap();
    store.set_marker(human, Some(Marker::Todo)).unwrap();
    let context = store.project_context("Project Alpha", None, false).unwrap();
    assert_eq!(context.human_open_tasks.len(), 1);
    assert!(!context.human_open_tasks[0].agent_owned);
    assert_eq!(context.agent_tasks.len(), 0);
    let through = today().format("%Y-%m-%d").to_string();
    let req = ConsolidationRequest {
        page: "Project Alpha".to_string(),
        through,
        brief: Brief::default(),
        timeline_entries: Vec::new(),
        tasks_to_create: Vec::new(),
        task_updates: vec![TaskUpdate {
            block_id: human,
            marker: Marker::Done,
        }],
    };
    assert!(matches!(
        store.apply_consolidation(&req),
        Err(Error::InvalidConsolidation(_))
    ));
    assert_eq!(
        store
            .page_blocks_all(page)
            .unwrap()
            .iter()
            .find(|b| b.id == human)
            .unwrap()
            .marker,
        Some(Marker::Todo)
    );
}

#[test]
fn invalid_requests_are_refused_before_writing() {
    let (mut store, project, older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    let before = store.page_blocks_all(project).unwrap();

    let mut self_link = request(&through, &older);
    self_link.brief.current_state[0].text = "See [[Project Alpha]]".to_string();
    assert!(matches!(
        store.apply_consolidation(&self_link),
        Err(Error::InvalidConsolidation(_))
    ));

    let mut future = request(&through, &older);
    future.through = today()
        .checked_add_days(Days::new(1))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    assert!(matches!(
        store.apply_consolidation(&future),
        Err(Error::BadDate(_))
    ));

    let mut journal_target = request(&through, &older);
    journal_target.page = older.clone();
    assert!(matches!(
        store.apply_consolidation(&journal_target),
        Err(Error::InvalidConsolidation(_))
    ));

    let mut no_provenance = request(&through, &older);
    no_provenance.tasks_to_create[0].source_day.clear();
    assert!(matches!(
        store.apply_consolidation(&no_provenance),
        Err(Error::BadDate(_)) | Err(Error::InvalidConsolidation(_))
    ));

    assert_eq!(store.page_blocks_all(project).unwrap(), before);
}

#[test]
fn noop_does_not_create_a_digest() {
    let (mut store, project, _older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    let req = ConsolidationRequest {
        page: "Project Alpha".to_string(),
        through: through.clone(),
        brief: Brief::default(),
        timeline_entries: Vec::new(),
        tasks_to_create: Vec::new(),
        task_updates: Vec::new(),
    };
    let result = store.apply_consolidation(&req).unwrap();
    assert!(!result.applied);
    assert!(
        !store
            .page_blocks_all(project)
            .unwrap()
            .iter()
            .any(|block| block.content == DIGEST_HEADING)
    );
}

#[test]
fn nothing_new_is_reported_after_consolidation() {
    let (mut store, _project, older, _) = setup_project();
    let through = today().format("%Y-%m-%d").to_string();
    store
        .apply_consolidation(&request(&through, &older))
        .unwrap();
    let context = store.project_context("Project Alpha", None, false).unwrap();
    assert!(context.nothing_new);
    assert!(context.sources.is_empty());
    assert_eq!(
        context.digest.last_consolidated.as_deref(),
        Some(through.as_str())
    );
}

#[test]
fn consolidation_does_not_change_the_schema_version() {
    let path = std::env::temp_dir().join(format!(
        "lognotes-project-digest-{}-{}.db",
        std::process::id(),
        today()
    ));
    let _ = std::fs::remove_file(&path);
    {
        let mut store = Store::open(&path).expect("a store");
        let project = store.ensure_page("Project Alpha").unwrap();
        store.insert_block(project, None, "human").unwrap();
        let day = day(1);
        let page = store.ensure_journal_page(&day).unwrap();
        store
            .insert_block(page, None, "Worked on [[Project Alpha]]")
            .unwrap();
        let through = today().format("%Y-%m-%d").to_string();
        store.apply_consolidation(&request(&through, &day)).unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
    drop(conn);
    let _ = std::fs::remove_file(&path);
}
