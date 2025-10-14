use shai_core::agent::{Agent, AgentError};
use shai_llm::ChatMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

use super::{AgentSession, RequestSession, SessionConfig};

/// Configuration for the session manager
#[derive(Clone, Debug)]
pub struct SessionManagerConfig {
    /// Maximum number of concurrent sessions (None = unlimited)
    pub max_sessions: Option<usize>,
    /// Default agent name for new sessions
    pub agent_name: Option<String>,
    /// Whether sessions are ephemeral by default
    pub ephemeral: bool,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            max_sessions: Some(100),
            agent_name: None,
            ephemeral: false,
        }
    }
}

/// Session manager - manages multiple agent sessions by ID
/// Handles creation, deletion, and access control for sessions
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<AgentSession>>>>,
    max_sessions: Option<usize>,
    allow_creation: bool,
    default_config: SessionConfig,
}

impl SessionManager {
    /// Create a new session manager
    /// - `max_sessions`: Maximum number of concurrent sessions (None = unlimited)
    /// - `default_config`: Default configuration for new sessions
    pub fn new(config: SessionManagerConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            max_sessions: config.max_sessions,
            allow_creation: true,
            default_config: SessionConfig { 
                agent_name: config.agent_name, 
                ephemeral: config.ephemeral
            },
        }
    }

    /// Create a new agent session
    /// Spawns the agent task with cleanup logic for ephemeral sessions
    async fn create_session(
        &self,
        http_request_id: &String,
        session_id: &str,
        config: SessionConfig,
    ) -> Result<Arc<AgentSession>, AgentError> {
        use shai_core::agent::AgentBuilder;

        info!("[{}] - [{}] Creating new session", http_request_id, session_id);

        // Build the agent
        let mut agent = AgentBuilder::create(config.agent_name.clone())
            .await
            .map_err(|e| AgentError::ExecutionError(format!("Failed to create agent: {}", e)))?
            .sudo()
            .build();

        let controller = agent.controller();
        let event_rx = agent.watch();

        // Spawn agent task with cleanup logic
        let sessions_for_cleanup = self.sessions.clone();
        let sid_for_cleanup = session_id.to_string();

        let agent_task = tokio::spawn(async move {
            match agent.run().await {
                Ok(_) => {
                    info!("[] - [{}] Agent completed successfully", sid_for_cleanup);
                }
                Err(e) => {
                    error!("[] - [{}] Agent execution error: {}", sid_for_cleanup, e);
                }
            }

            // If ephemeral, remove from sessions HashMap when agent.run() exits
            // This happens after lifecycle calls controller.cancel()
            sessions_for_cleanup.lock().await.remove(&sid_for_cleanup);
            info!("[] - [{}] session removed from manager", sid_for_cleanup);
        });

        let session = Arc::new(AgentSession::new(
            controller,
            event_rx,
            agent_task,
            config,
            session_id.to_string(),
        ));

        Ok(session)
    }

    /// Get or create a session for the given session ID
    async fn get_or_create_session(
        &self,
        http_request_id: &String,
        session_id: &str,
        config: Option<SessionConfig>,
    ) -> Result<Arc<AgentSession>, AgentError> {
        let sessions = self.sessions.lock().await;

        // Check if session exists
        if let Some(session) = sessions.get(session_id) {
            info!("[{}] - [{}] Using existing session", http_request_id, session_id);
            return Ok(session.clone());
        }

        // Check if creation is allowed
        if !self.allow_creation {
            return Err(AgentError::ExecutionError(
                "Session creation disabled".to_string(),
            ));
        }

        // Check max sessions limit
        if let Some(max) = self.max_sessions {
            if sessions.len() >= max {
                return Err(AgentError::ExecutionError(format!(
                    "Maximum number of sessions reached: {}",
                    max
                )));
            }
        }

        // Create new session
        let session_config = config.unwrap_or_else(|| self.default_config.clone());

        // Drop the lock before creating session (which spawns agent task)
        drop(sessions);

        let session = self.create_session(&http_request_id, session_id, session_config).await?;

        // Re-acquire lock to insert into HashMap
        self.sessions.lock().await.insert(session_id.to_string(), session.clone());

        Ok(session)
    }

    /// Handle an incoming request
    /// - If `session_id` is provided, use or create that session
    /// - If `session_id` is None, generate a new ephemeral session ID
    pub async fn handle_request(
        &self,
        trace: Vec<ChatMessage>,
        session_id: Option<String>,
        http_request_id: String,
    ) -> Result<(RequestSession, String), AgentError> {
        // Determine session ID
        let session_id = session_id.unwrap_or_else(|| {
            // No session ID provided - generate a new UUID
            // Will use default config (which has the configured ephemeral setting)
            Uuid::new_v4().to_string()
        });

        // Get or create the session (using default config)
        let session = self.get_or_create_session(&http_request_id, &session_id, None).await?;

        // Handle the request
        let request_session = session.handle_request(&http_request_id, trace).await?;

        // Cleanup is now handled automatically:
        // 1. When stream completes, lifecycle Drop calls controller.cancel()
        // 2. Agent task's agent.run() exits
        // 3. Agent task cleanup code removes session from HashMap (for ephemeral only)

        Ok((request_session, session_id))
    }

    /// Delete a session by ID
    pub async fn delete_session(&self, session_id: &str) -> bool {
        info!("[] - [{}] Deleting session", session_id);
        self.sessions.lock().await.remove(session_id).is_some()
    }

    /// Cancel a session (stop the agent)
    pub async fn cancel_session(&self, http_request_id: &String, session_id: &str) -> Result<(), AgentError> {
        if let Some(session) = self.sessions.lock().await.get(session_id) {
            session.cancel(http_request_id).await?;
        }
        Ok(())
    }

    /// Get the number of active sessions
    pub async fn session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Set whether new sessions can be created
    pub fn set_allow_creation(&mut self, allow: bool) {
        self.allow_creation = allow;
    }
}
