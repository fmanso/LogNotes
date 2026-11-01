//! LogNotes' model and store.
//!
//! Owns pages, blocks, sibling order, and the durable state behind them. No Qt, no UI: the
//! `lognotes-qt` crate calls in here and never the other way round.
//!
//! See `lognotes-core/SPEC.md` for the contract this implements.

mod error;
pub mod markdown;
mod model;
mod order;
mod store;
mod text;

pub use error::{Error, Result};
pub use model::{
    AgendaRow, BlockHit, BlockRow, JournalDay, Marker, Page, PageKind, ReferenceKind, ReferenceRow,
    SearchResults,
};
pub use order::key_between;
pub use store::Store;
