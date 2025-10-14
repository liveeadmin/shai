use axum::{
    extract::State,
    response::Response,
};
use openai_dive::v1::resources::chat::ChatCompletionParameters;
use tracing::info;
use uuid::Uuid;

use crate::{ApiJson, ServerState, ErrorResponse};

// TODO: Refactor this handler to use the new session architecture

/// Handle OpenAI chat completion - non-streaming only
pub async fn handle_chat_completion(
    State(_state): State<ServerState>,
    ApiJson(payload): ApiJson<ChatCompletionParameters>,
) -> Result<Response, ErrorResponse> {
    let session_id = Uuid::new_v4();

    // Log request with path
    info!("[{}] POST /v1/chat/completions model={}", session_id, payload.model);

    // TODO: Refactor to use state.session.handle_request()
    return Err(ErrorResponse::internal_error("Completion API not yet refactored to new architecture".to_string()));
}
