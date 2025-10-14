mod lifecycle;
mod session;
mod manager;

pub use lifecycle::{RequestLifecycle, BackgroundLifecycle, EphemeralLifecycle};
pub use session::{AgentSession, SessionConfig};
pub use manager::{SessionManager, SessionManagerConfig};

use shai_core::agent::{AgentController, AgentEvent};
use tokio::sync::broadcast::Receiver;

/// Represents a single request session with automatic lifecycle management
pub struct RequestSession {
    pub controller: AgentController,
    pub event_rx: Receiver<AgentEvent>,
    pub lifecycle: Box<dyn RequestLifecycle>,
}

impl RequestSession {
    pub fn new(
        controller: AgentController,
        event_rx: Receiver<AgentEvent>,
        lifecycle: Box<dyn RequestLifecycle>,
    ) -> Self {
        Self {
            controller,
            event_rx,
            lifecycle,
        }
    }
}
