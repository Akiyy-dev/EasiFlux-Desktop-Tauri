use super::{
    plugin_state::StateSchema,
    safe_plugin_document::{CandidateBytes, PersistResult, SafeDocumentNames, SafePluginDocument},
};
use crate::{
    error::{AppError, AppResult},
    models::config::APP_NAME,
    plugin::ownership::{ManagedOwnershipIndexV1, MAX_MANAGED_OWNERSHIP_BYTES},
};
use std::{path::PathBuf, sync::Mutex};

#[derive(Clone, Debug)]
pub(crate) struct ManagedOwnershipLoad {
    pub(crate) index: ManagedOwnershipIndexV1,
    /// Secondary recovery is not deletion authority until equivalent safe rewrite.
    pub(crate) requires_rewrite: bool,
}
pub(crate) trait ManagedOwnershipPersistence: Send + Sync {
    fn load(&self) -> AppResult<ManagedOwnershipLoad>;
    fn save(&self, index: &ManagedOwnershipIndexV1) -> PersistResult;
}
pub(crate) struct ManagedOwnershipStore {
    root: Option<PathBuf>,
    transaction: Mutex<()>,
    #[cfg(test)]
    hook: Option<super::safe_plugin_document::StepHook>,
}
impl ManagedOwnershipStore {
    pub(crate) fn new() -> Self {
        Self {
            root: None,
            transaction: Mutex::new(()),
            #[cfg(test)]
            hook: None,
        }
    }
    pub(crate) fn with_plugins_root(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            transaction: Mutex::new(()),
            #[cfg(test)]
            hook: None,
        }
    }
    fn document(&self) -> AppResult<SafePluginDocument> {
        let root = match &self.root {
            Some(root) => root.clone(),
            None => dirs::config_dir()
                .ok_or_else(unavailable)?
                .join(APP_NAME)
                .join("plugins"),
        };
        let document = SafePluginDocument::new(
            root,
            SafeDocumentNames::managed_ownership(),
            MAX_MANAGED_OWNERSHIP_BYTES,
        );
        #[cfg(test)]
        let document = {
            let mut document = document;
            document.hook = self.hook.clone();
            document
        };
        Ok(document)
    }
    fn load_document(document: &SafePluginDocument) -> AppResult<Option<ManagedOwnershipLoad>> {
        let mut present = false;
        for (position, candidate) in document
            .load_candidates()
            .map_err(|_| unavailable())?
            .into_iter()
            .enumerate()
        {
            let bytes = match candidate {
                CandidateBytes::Missing => continue,
                CandidateBytes::Oversized if position == 0 => return Err(unavailable()),
                CandidateBytes::Oversized => {
                    present = true;
                    continue;
                }
                CandidateBytes::Bounded(bytes) => bytes,
            };
            present = true;
            if serde_json::from_slice::<StateSchema>(&bytes).is_ok_and(|header| header.0 > 1) {
                return Err(unavailable());
            }
            if let Ok(index) = ManagedOwnershipIndexV1::parse(&bytes) {
                return Ok(Some(ManagedOwnershipLoad {
                    index,
                    requires_rewrite: position != 0,
                }));
            }
        }
        if present {
            Err(unavailable())
        } else {
            Ok(None)
        }
    }
}
impl ManagedOwnershipPersistence for ManagedOwnershipStore {
    fn load(&self) -> AppResult<ManagedOwnershipLoad> {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        Ok(
            Self::load_document(&self.document()?)?.unwrap_or_else(|| ManagedOwnershipLoad {
                index: ManagedOwnershipIndexV1::empty(),
                requires_rewrite: false,
            }),
        )
    }
    fn save(&self, index: &ManagedOwnershipIndexV1) -> PersistResult {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        let document = self.document()?;
        let previous = Self::load_document(&document)?
            .map(|loaded| loaded.index.canonical_bytes())
            .transpose()
            .map_err(|_| unavailable())?;
        let next = index.canonical_bytes().map_err(|_| unavailable())?;
        document.persist(previous.as_deref(), &next)
    }
}
fn unavailable() -> AppError {
    AppError::Plugin {
        code: "plugin_ownership_unavailable",
        message: "所有权记录暂不可用",
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests;
