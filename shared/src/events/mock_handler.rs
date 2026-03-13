use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::events::EventEnvelope;
use crate::events::consumer::HandlerError;

/// Test double for `EventHandler` that records received events and returns
/// pre-configured results in FIFO order.
///
/// When the result queue is empty, returns `Ok(())` (always-ok mode).
///
/// # Usage
/// ```ignore
/// let handler = MockEventHandler::new();
/// handler.push_result(Err(HandlerError::transient("fail once")));
/// // First call → Err(transient), second call → Ok(())
/// ```
pub struct MockEventHandler {
    results: Arc<Mutex<VecDeque<Result<(), HandlerError>>>>,
    received: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl MockEventHandler {
    pub fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(VecDeque::new())),
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queue a result to be returned on the next `handle()` call.
    pub fn push_result(&self, result: Result<(), HandlerError>) {
        self.results.lock().unwrap().push_back(result);
    }

    /// Snapshot of all envelopes received by `handle()`.
    pub fn received(&self) -> Vec<EventEnvelope> {
        self.received.lock().unwrap().clone()
    }

    /// Number of times `handle()` was called.
    pub fn received_count(&self) -> usize {
        self.received.lock().unwrap().len()
    }
}

impl Default for MockEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::events::consumer::EventHandler for MockEventHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        _tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        self.received.lock().unwrap().push(envelope.clone());
        match self.results.lock().unwrap().pop_front() {
            Some(result) => result,
            None => Ok(()),
        }
    }
}
