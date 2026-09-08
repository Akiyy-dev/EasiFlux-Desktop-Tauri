use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use uuid::Uuid;

use super::{ImportPreview, PreparedManifest};
use crate::error::{AppError, AppResult};

enum SessionState {
    Idle,
    Preparing {
        owner: Uuid,
    },
    Ready {
        owner: Uuid,
        token: String,
        expires_at: Instant,
        generation: u64,
        content: PreparedManifest,
    },
    Committing {
        owner: Uuid,
    },
}

pub(crate) struct ImportSessions {
    state: Mutex<SessionState>,
}

pub(crate) struct PrepareLease {
    sessions: Arc<ImportSessions>,
    owner: Uuid,
}

pub(crate) struct CommitLease {
    sessions: Arc<ImportSessions>,
    owner: Uuid,
    generation: u64,
    content: PreparedManifest,
}

impl ImportSessions {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SessionState::Idle),
        })
    }

    pub(crate) fn reserve_prepare(self: &Arc<Self>, now: Instant) -> AppResult<PrepareLease> {
        let mut state = self.state.lock().map_err(|_| busy())?;
        expire(&mut state, now);
        if !matches!(*state, SessionState::Idle) {
            return Err(busy());
        }
        let owner = Uuid::new_v4();
        *state = SessionState::Preparing { owner };
        Ok(PrepareLease {
            sessions: Arc::clone(self),
            owner,
        })
    }

    pub(crate) fn claim_commit(
        self: &Arc<Self>,
        token: &str,
        now: Instant,
    ) -> AppResult<CommitLease> {
        let mut state = self.state.lock().map_err(|_| busy())?;
        expire(&mut state, now);
        if token.len() != 32
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid_token());
        }
        let owner = match &*state {
            SessionState::Ready {
                owner,
                token: current,
                ..
            } if current == token => *owner,
            _ => return Err(invalid_token()),
        };
        let SessionState::Ready {
            content,
            generation,
            ..
        } = std::mem::replace(&mut *state, SessionState::Committing { owner })
        else {
            unreachable!("ready state checked under the same mutex")
        };
        Ok(CommitLease {
            sessions: Arc::clone(self),
            owner,
            generation,
            content,
        })
    }

    pub(crate) fn cancel(&self, token: &str, now: Instant) -> AppResult<()> {
        let mut state = self.state.lock().map_err(|_| busy())?;
        expire(&mut state, now);
        match &*state {
            SessionState::Committing { .. } => Err(busy()),
            SessionState::Ready { token: current, .. } if current == token => {
                *state = SessionState::Idle;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl PrepareLease {
    pub(crate) fn publish(
        self,
        content: PreparedManifest,
        generation: u64,
        now: Instant,
    ) -> AppResult<ImportPreview> {
        let mut state = self.sessions.state.lock().map_err(|_| busy())?;
        if !matches!(*state, SessionState::Preparing { owner } if owner == self.owner) {
            return Err(busy());
        }
        let mut public_id = Uuid::new_v4();
        while public_id == self.owner {
            public_id = Uuid::new_v4();
        }
        let token = public_id.simple().to_string();
        let preview = ImportPreview::new(
            token.clone(),
            generation,
            content.record().manifest().clone(),
        );
        *state = SessionState::Ready {
            owner: self.owner,
            token,
            expires_at: now + Duration::from_secs(300),
            generation,
            content,
        };
        Ok(preview)
    }
}

impl CommitLease {
    pub(crate) fn content(&self) -> &PreparedManifest {
        &self.content
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

impl Drop for PrepareLease {
    fn drop(&mut self) {
        let mut state = self
            .sessions
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Publishing transfers this owner's admission to Ready; this destructor must not undo it.
        if matches!(*state, SessionState::Preparing { owner } if owner == self.owner) {
            *state = SessionState::Idle;
        }
    }
}

impl Drop for CommitLease {
    fn drop(&mut self) {
        let mut state = self
            .sessions
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if matches!(*state, SessionState::Committing { owner } if owner == self.owner) {
            *state = SessionState::Idle;
        }
    }
}

fn expire(state: &mut SessionState, now: Instant) {
    if matches!(state, SessionState::Ready { expires_at, .. } if now >= *expires_at) {
        *state = SessionState::Idle;
    }
}

fn busy() -> AppError {
    AppError::Plugin {
        code: "plugin_import_busy",
        message: "已有清单导入操作，请先完成或取消。",
        diagnostic: None,
    }
}

fn invalid_token() -> AppError {
    AppError::Plugin {
        code: "plugin_import_token_invalid",
        message: "预览已过期或失效，请重新选择清单。",
        diagnostic: None,
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    pub(crate) fn duplicate_prepare(lease: &PrepareLease) -> PrepareLease {
        PrepareLease {
            sessions: Arc::clone(&lease.sessions),
            owner: lease.owner,
        }
    }

    pub(crate) fn prepare_owner(lease: &PrepareLease) -> Uuid {
        lease.owner
    }

    pub(crate) fn duplicate_commit(lease: &CommitLease) -> CommitLease {
        CommitLease {
            sessions: Arc::clone(&lease.sessions),
            owner: lease.owner,
            generation: lease.generation,
            content: lease.content.clone(),
        }
    }
}
