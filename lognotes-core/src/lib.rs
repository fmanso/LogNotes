//! LogNotes' model and store.
//!
//! Owns pages, blocks, sibling order, and the durable state behind them. No Qt, no UI: the
//! `lognotes-qt` crate calls in here and never the other way round.
//!
//! See `lognotes-core/SPEC.md` for the contract this implements.

pub mod code;
mod error;
pub mod export;
pub mod markdown;
mod model;
mod order;
pub mod project;
mod store;
mod text;

pub use error::{Error, Result};
pub use export::{ImportMode, Moved, export_markdown, import_markdown};
pub use model::{
    AgendaRow, BlockHit, BlockRow, JournalDay, Marker, Page, PageKind, ReferenceKind, ReferenceRow,
    Renamed, SearchResults,
};
pub use order::key_between;
pub use project::{
    BlockRef, Brief, BriefItem, ConsolidationRequest, ConsolidationResult, DigestInfo,
    ProjectContext, ProjectSource, TaskRef, TaskToCreate, TaskUpdate, TimelineEntry,
};
pub use store::{Store, is_date_name};
