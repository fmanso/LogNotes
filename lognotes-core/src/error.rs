use thiserror::Error;

/// Everything that can go wrong in the store. Every variant can be shown to a person: the UI names the
/// page or block it concerns, and nothing panics on input the bridge can pass.
#[derive(Debug, Error)]
pub enum Error {
    #[error("no page or block with id {0}")]
    NotFound(i64),

    #[error("no ordering key exists between {prev:?} and {next:?}")]
    InvalidPosition {
        prev: Option<String>,
        next: Option<String>,
    },

    #[error("offset {0} is not on a character boundary")]
    BadOffset(usize),

    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: i64, supported: i64 },

    #[error("{0}")]
    BadDate(String),

    #[error(transparent)]
    Storage(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
