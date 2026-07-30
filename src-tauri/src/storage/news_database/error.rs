use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewsStorageErrorKind {
    Database,
    InvalidInput,
    SourceMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewsStorageError {
    kind: NewsStorageErrorKind,
}

impl NewsStorageError {
    pub(crate) const fn database() -> Self {
        Self {
            kind: NewsStorageErrorKind::Database,
        }
    }

    pub(crate) const fn invalid_input() -> Self {
        Self {
            kind: NewsStorageErrorKind::InvalidInput,
        }
    }

    pub(crate) const fn source_mismatch() -> Self {
        Self {
            kind: NewsStorageErrorKind::SourceMismatch,
        }
    }

    pub(crate) const fn kind(&self) -> NewsStorageErrorKind {
        self.kind
    }

    const fn category(&self) -> &'static str {
        match self.kind {
            NewsStorageErrorKind::Database => "database",
            NewsStorageErrorKind::InvalidInput => "invalid input",
            NewsStorageErrorKind::SourceMismatch => "source mismatch",
        }
    }
}

impl fmt::Display for NewsStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "news storage failed: {}", self.category())
    }
}

impl std::error::Error for NewsStorageError {}
