use super::*;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub(crate) struct ComputeSlot {
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
}
pub(crate) struct ComputeLease {
    slot: Arc<ComputeSlot>,
    pub(crate) cancel: Arc<AtomicBool>,
}
impl ComputeSlot {
    pub(crate) fn global() -> Arc<Self> {
        static SLOT: OnceLock<Arc<ComputeSlot>> = OnceLock::new();
        SLOT.get_or_init(|| Arc::new(Self::default())).clone()
    }
    pub(crate) fn acquire(self: &Arc<Self>, id: &str) -> AppResult<ComputeLease> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| error("plugin_compute_internal"))?;
        if active.is_some() {
            return Err(error("plugin_compute_busy"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((id.to_owned(), cancel.clone()));
        Ok(ComputeLease {
            slot: self.clone(),
            cancel,
        })
    }
    pub(crate) fn cancel(&self, id: &str) -> AppResult<bool> {
        let active = self
            .active
            .lock()
            .map_err(|_| error("plugin_compute_internal"))?;
        if let Some((_, cancel)) = active.as_ref().filter(|(active_id, _)| active_id == id) {
            cancel.store(true, Ordering::Release);
            return Ok(true);
        }
        Ok(false)
    }
    pub(crate) fn invalidate(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some((_, cancel)) = active.as_ref() {
                cancel.store(true, Ordering::Release);
            }
        }
    }
}
impl Drop for ComputeLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.slot.active.lock() {
            if active
                .as_ref()
                .is_some_and(|(_, cancel)| Arc::ptr_eq(cancel, &self.cancel))
            {
                *active = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_and_cancel_are_owned_until_worker_exit_and_old_ids_cannot_cancel_new_run() {
        let slot = Arc::new(ComputeSlot::default());
        let first = slot.acquire("first").unwrap();
        assert!(slot.acquire("second").is_err());
        assert!(!slot.cancel("old").unwrap());
        assert!(!first.cancel.load(Ordering::Acquire));
        assert!(slot.cancel("first").unwrap());
        assert!(first.cancel.load(Ordering::Acquire));
        assert!(slot.acquire("second").is_err());
        drop(first);
        let second = slot.acquire("second").unwrap();
        assert!(!slot.cancel("first").unwrap());
        assert!(!second.cancel.load(Ordering::Acquire));
        slot.invalidate();
        assert!(second.cancel.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn dropped_ipc_waiter_does_not_release_owned_blocking_worker_slot() {
        let slot = Arc::new(ComputeSlot::default());
        let lease = slot.acquire("first").unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                started.send(()).unwrap();
                hold.recv().unwrap();
                drop(lease);
                finished.send(()).unwrap();
            })
            .await
            .unwrap();
        });
        ready.await.unwrap();
        task.abort();
        assert!(slot.acquire("second").is_err());
        release.send(()).unwrap();
        done.await.unwrap();
        assert!(slot.acquire("second").is_ok());
    }
}
