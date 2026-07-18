use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use rmcp::model::RequestId;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

use super::MAX_PENDING_REQUESTS;

#[derive(Clone)]
pub(super) struct Admission {
    active: Arc<Mutex<Vec<ActiveRequest>>>,
    request_slots: Arc<Semaphore>,
}

struct ActiveRequest {
    id: RequestId,
    delivered: bool,
    cancellation: bool,
}

pub(super) enum CancellationStatus {
    Ready,
    Deferred,
    Stale,
}

impl Admission {
    pub(super) fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(Vec::with_capacity(MAX_PENDING_REQUESTS))),
            request_slots: Arc::new(Semaphore::new(MAX_PENDING_REQUESTS)),
        }
    }

    pub(super) fn try_request(&self, id: &RequestId) -> Option<OwnedSemaphorePermit> {
        let permit = self.request_slots.clone().try_acquire_owned().ok()?;
        self.lock().push(ActiveRequest {
            id: id.clone(),
            delivered: false,
            cancellation: false,
        });
        Some(permit)
    }

    pub(super) fn mark_delivered(&self, id: &RequestId) {
        if let Some(request) = self.lock().iter_mut().find(|request| request.id == *id) {
            request.delivered = true;
        }
    }

    pub(super) fn cancellation_status(&self, id: &RequestId) -> CancellationStatus {
        self.lock().iter().find(|request| request.id == *id).map_or(
            CancellationStatus::Stale,
            |request| {
                if request.delivered {
                    CancellationStatus::Ready
                } else {
                    CancellationStatus::Deferred
                }
            },
        )
    }

    pub(super) fn try_cancellation(&self, id: &RequestId) -> bool {
        let active = self.lock();
        let result = active
            .iter()
            .find(|request| request.id == *id)
            .is_some_and(|request| !request.cancellation);
        drop(active);
        result
    }

    pub(super) fn admit_cancellation(&self, id: &RequestId) {
        let mut active = self.lock();
        if let Some(request) = active.iter_mut().find(|request| request.id == *id)
            && !request.cancellation
        {
            request.cancellation = true;
        }
    }

    pub(super) fn complete(&self, id: &RequestId) {
        let mut active = self.lock();
        let index = active.iter().position(|request| request.id == *id);
        if let Some(index) = index {
            active.swap_remove(index);
        }
        drop(active);
    }

    pub(super) fn clear(&self) {
        self.lock().clear();
    }

    #[cfg(test)]
    pub(super) fn active_count(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    pub(super) fn cancellation_count(&self) -> usize {
        self.lock()
            .iter()
            .filter(|request| request.cancellation)
            .count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<ActiveRequest>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Clone)]
pub struct ConnectionEnd {
    ended: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    notification: Arc<Notify>,
}

impl ConnectionEnd {
    pub(super) fn new() -> Self {
        Self {
            ended: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            notification: Arc::new(Notify::new()),
        }
    }

    pub(super) fn finish(&self) {
        self.ended.store(true, Ordering::Release);
        self.notification.notify_waiters();
    }

    pub(super) fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        self.finish();
    }

    pub async fn wait(&self) {
        while !self.ended.load(Ordering::Acquire) {
            self.notification.notified().await;
        }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}
