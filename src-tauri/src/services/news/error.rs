use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsServiceErrorKind {
    Unavailable,
    Repository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsServiceError(NewsServiceErrorKind);

impl NewsServiceError {
    pub(super) const fn unavailable() -> Self {
        Self(NewsServiceErrorKind::Unavailable)
    }

    pub(super) const fn repository() -> Self {
        Self(NewsServiceErrorKind::Repository)
    }

    #[cfg(test)]
    pub fn kind(&self) -> NewsServiceErrorKind {
        self.0
    }
}

impl fmt::Display for NewsServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            NewsServiceErrorKind::Unavailable => "news service unavailable",
            NewsServiceErrorKind::Repository => "news repository operation failed",
        })
    }
}

impl std::error::Error for NewsServiceError {}
