//! Does the journal still feel instant after years of use?
//!
//! Ignored by default because it builds a multi-year graph. Run it when the question comes up:
//!
//! ```text
//! cargo test -p lognotes-core --test scale -- --ignored --nocapture
//! ```
//!
//! What it measures is the thing that grows: the column keeps every day it has loaded and re-reads all of
//! them after every save, so the cost of one keystroke is a function of how far back you have scrolled.

use std::time::Instant;

use lognotes_core::Store;

const BLOCKS_PER_DAY: usize = 8;

fn day(offset: i64) -> String {
    (chrono::NaiveDate::from_ymd_opt(2026, 11, 2).expect("a date")
        + chrono::Days::new(offset.unsigned_abs()))
    .format("%Y-%m-%d")
    .to_string()
}

/// A graph with `days` days of journal behind today, each with a few blocks and a link.
fn years_of_notes(store: &mut Store, days: i64) {
    for offset in 0..days {
        let date = day(offset);
        let page = store.ensure_journal_page(&date).expect("a day");
        for block in 0..BLOCKS_PER_DAY {
            let content = format!("note {block} on {date} about [[Project {}]] #journal", block % 3);
            let id = store.insert_block(page, None, &content).expect("a block");
            store.update_content(id, &content).expect("indexed");
        }
    }
}

/// What one snapshot costs: reading the days the column is holding.
fn read(store: &Store, days: i64) -> std::time::Duration {
    let dates: Vec<String> = (0..days).map(day).collect();
    let started = Instant::now();
    let mut blocks = 0;
    for date in &dates {
        blocks += store.day(date).expect("a day").blocks.len();
    }
    let elapsed = started.elapsed();
    assert!(blocks > 0);
    elapsed
}

#[test]
#[ignore = "builds five years of notes; run it on purpose"]
fn the_column_stays_quick_after_years() {
    let path = std::env::temp_dir().join("lognotes-scale.db");
    let _ = std::fs::remove_file(&path);
    let mut store = Store::open(&path).expect("a store");

    println!("building five years of notes…");
    let build = Instant::now();
    years_of_notes(&mut store, 365 * 5);
    println!(
        "  {} days, {} blocks in {:.1?}; database {:.1} MB",
        365 * 5,
        365 * 5 * BLOCKS_PER_DAY,
        build.elapsed(),
        std::fs::metadata(&path).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0),
    );

    println!("\nwhat one save costs (command + re-read of every loaded day):");
    for days in [14, 90, 365, 365 * 2, 365 * 5] {
        let read_only = read(&store, days);
        let id = store
            .page_by_name(&format!("Project {}", 0))
            .ok()
            .flatten()
            .map(|page| page.id)
            .and_then(|_| store.journal_page(&day(0)).ok().flatten())
            .map(|page| store.page_blocks(page.id).expect("blocks")[0].id)
            .expect("a block to edit");
        let started = Instant::now();
        store.update_content(id, "note edited after years of use").expect("saved");
        let save = started.elapsed();
        println!(
            "  {days:>5} days loaded: read {read_only:.1?}, save {save:.1?}, total ~{:.1?}",
            read_only + save
        );
    }

    println!("\nwhat the rest of the app costs at that size:");
    let started = Instant::now();
    let found = store.search("project", 5, 20).expect("a search");
    println!(
        "  quick switcher preview: {:.1?} ({} hits of {} matches)",
        started.elapsed(),
        found.pages.len() + found.blocks.len(),
        found.total
    );
    let started = Instant::now();
    let all = store.search("project", 50, 500).expect("a search");
    println!(
        "  full results view:      {:.1?} ({} rows)",
        started.elapsed(),
        all.pages.len() + all.blocks.len()
    );
    let started = Instant::now();
    let _ = store.recent_pages(8).expect("recents");
    let _ = store.pages_by_favourite().expect("favourites");
    println!("  sidebar:                {:.1?}", started.elapsed());
    let started = Instant::now();
    let _ = store.agenda(&day(0), 7).expect("the agenda");
    println!("  agenda:                 {:.1?}", started.elapsed());

    let _ = std::fs::remove_file(&path);
}
