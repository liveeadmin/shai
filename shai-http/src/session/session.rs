use shai_core::agent::{AgentController, AgentError, AgentEvent};
use shai_llm::ChatMessage;
use std::sync::Arc;
use tokio::sync::{broadcast::Receiver, Mutex};
use tokio::task::JoinHandle;
use tracing::debug;
use openai_dive::v1::resources::chat::ChatMessageContentPart;
use shai_llm::ChatMessageContent;

use super::{RequestSession, BackgroundLifecycle, EphemeralLifecycle};

/// Configuration for creating a new agent session
#[derive(Clone)]
pub struct SessionConfig {
    pub agent_name: Option<String>,
    pub ephemeral: bool,
}

/// A single agent session - represents one running agent instance
/// Can be ephemeral (destroyed after request) or persistent (kept alive)
pub struct AgentSession {
    controller: Arc<Mutex<AgentController>>,
    event_rx: Receiver<AgentEvent>,
    agent_task: JoinHandle<()>,
    pub session_id: String,
    pub agent_name: String,
    pub ephemeral: bool,
}

impl AgentSession {
    /// Create a new agent session with the given agent and configuration
    /// Called by SessionManager which handles the agent task spawning and cleanup
    pub fn new(
        controller: AgentController,
        event_rx: Receiver<AgentEvent>,
        agent_task: JoinHandle<()>,
        config: SessionConfig,
        session_id: String,
    ) -> Self {
        let agent_name_display = config.agent_name.clone().unwrap_or_else(|| "default".to_string());

        Self {
            controller: Arc::new(Mutex::new(controller)),
            event_rx,
            agent_task,
            session_id,
            agent_name: agent_name_display,
            ephemeral: config.ephemeral,
        }
    }

    pub async fn cancel(&self, http_request_id: &String)  -> Result<(), AgentError> {
        debug!("[{}] - [{}] Acquiring controller lock", http_request_id, self.session_id);
        let controller_guard = self.controller.clone().lock_owned().await;
        debug!("[{}] - [{}] Controller lock acquired", http_request_id, self.session_id);
        controller_guard.cancel().await
    }

    /// Handle a request for this agent session
    /// Returns a RequestSession that manages the lifecycle
    pub async fn handle_request(&self, http_request_id: &String, trace: Vec<ChatMessage>) -> Result<RequestSession, AgentError> {
        debug!("[{}] - [{}] Acquiring controller lock", http_request_id, self.session_id);
        let controller_guard = self.controller.clone().lock_owned().await;
        debug!("[{}] - [{}] Controller lock acquired", http_request_id, self.session_id);

        // Send all user messages to the agent
        for msg in trace {
            match msg {
                ChatMessage::User { content, .. } => {
                    let text = match content {
                        ChatMessageContent::Text(t) => t,
                        ChatMessageContent::ContentPart(parts) => {
                            parts.iter()
                                .filter_map(|p| match p {
                                    ChatMessageContentPart::Text(text_part) => Some(text_part.text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                        ChatMessageContent::None => String::new(),
                    };
                    if !text.is_empty() {
                        controller_guard.send_user_input(text).await?;
                    }
                }
                _ => {}
            }
        }

        let event_rx = self.event_rx.resubscribe();
        let controller = controller_guard.clone();

        // Choose lifecycle based on ephemeral flag
        let lifecycle: Box<dyn super::RequestLifecycle> = if self.ephemeral {
            Box::new(EphemeralLifecycle::new(controller_guard, self.session_id.clone()))
        } else {
            Box::new(BackgroundLifecycle::new(controller_guard, self.session_id.clone()))
        };

        Ok(RequestSession::new(controller, event_rx, lifecycle))
    }

    pub fn is_ephemeral(&self) -> bool {
        self.ephemeral
    }
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        debug!("[] - [{}] Dropping agent session", self.session_id);
        self.agent_task.abort();
    }
}
