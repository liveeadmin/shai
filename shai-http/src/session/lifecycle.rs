use shai_core::agent::AgentController;
use tokio::sync::OwnedMutexGuard;
use tracing::debug;

/// Trait for managing request lifecycle cleanup
pub trait RequestLifecycle: Send {
    // Cleanup happens automatically on Drop
}

/// Background lifecycle for persistent sessions
/// Holds the controller lock for the duration of the request (stream)
/// When dropped (stream completes), releases the lock so next request can proceed
/// The session remains in the manager's HashMap for reuse
pub struct BackgroundLifecycle {
    _controller_guard: OwnedMutexGuard<AgentController>,
    session_id: String,
}

impl BackgroundLifecycle {
    pub fn new(controller_guard: OwnedMutexGuard<AgentController>, session_id: String) -> Self {
        Self {
            _controller_guard: controller_guard,
            session_id,
        }
    }
}

impl Drop for BackgroundLifecycle {
    fn drop(&mut self) {
        debug!("[{}] Stream completed, releasing controller lock (background session)", self.session_id);
    }
}

impl RequestLifecycle for BackgroundLifecycle {}

/// Ephemeral lifecycle for temporary sessions
/// When dropped (stream completes):
/// - Cancels the agent via controller.cancel()
/// - Releases the controller lock
/// - Session will be removed from HashMap by the manager after stream completes
pub struct EphemeralLifecycle {
    controller_guard: OwnedMutexGuard<AgentController>,
    session_id: String,
}

impl EphemeralLifecycle {
    pub fn new(
        controller_guard: OwnedMutexGuard<AgentController>,
        session_id: String,
    ) -> Self {
        Self {
            controller_guard,
            session_id,
        }
    }
}

impl Drop for EphemeralLifecycle {
    fn drop(&mut self) {
        debug!("[] - [{}] Stream completed, canceling ephemeral agent", self.session_id);
        // Cancel the agent - this will stop the agent loop gracefully
        let ctrl = self.controller_guard.clone();
        tokio::spawn(async move {
            let _ = ctrl.cancel().await;
        });
    }
}

impl RequestLifecycle for EphemeralLifecycle {}
