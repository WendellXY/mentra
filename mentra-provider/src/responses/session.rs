use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http::{HeaderMap, HeaderValue};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use url::Url;

use crate::{
    ContentBlock, ContentBlockDelta, ContentBlockStart, CredentialSource, ModelInfo,
    ProviderCredentials, ProviderDefinition, ProviderError, ProviderEvent, ProviderEventStream,
    ProviderSession, Request, Response, Role, TokenUsage,
};

use super::model::{ResponsesModelsPage, ResponsesRequest};
use super::sse::spawn_event_stream;

/// Session-scoped Responses transport state.
///
/// This is intentionally lightweight and keeps the pieces needed for websocket prewarm and
/// HTTP fallback without binding the provider to any higher-level runtime.
pub struct ResponsesSession<C> {
    definition: ProviderDefinition,
    credential_source: Arc<C>,
    client: reqwest::Client,
    state: Arc<ResponsesSessionState>,
}

#[derive(Default)]
struct WebsocketSession {
    connection_reused: StdMutex<bool>,
    _last_request: Option<ResponsesRequest>,
    last_response_rx: Option<oneshot::Receiver<Response>>,
}

impl WebsocketSession {
    fn set_connection_reused(&self, connection_reused: bool) {
        *self
            .connection_reused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connection_reused;
    }

    fn connection_reused(&self) -> bool {
        *self
            .connection_reused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) struct ResponsesSessionState {
    disable_websockets: AtomicBool,
    websocket_session: StdMutex<WebsocketSession>,
    turn_state: Arc<OnceLock<String>>,
}

impl Default for ResponsesSessionState {
    fn default() -> Self {
        Self {
            disable_websockets: AtomicBool::new(false),
            websocket_session: StdMutex::new(WebsocketSession::default()),
            turn_state: Arc::new(OnceLock::new()),
        }
    }
}

impl<C> ResponsesSession<C>
where
    C: CredentialSource + 'static,
{
    pub(crate) fn new(
        definition: ProviderDefinition,
        credential_source: Arc<C>,
        client: reqwest::Client,
        state: Arc<ResponsesSessionState>,
    ) -> Self {
        Self {
            definition,
            credential_source,
            client,
            state,
        }
    }

    pub fn websocket_connect_timeout(&self) -> Duration {
        self.definition.websocket_connect_timeout
    }

    pub fn stream_idle_timeout(&self) -> Duration {
        self.definition.stream_idle_timeout
    }

    pub fn websocket_url_for_path(&self, path: &str) -> Result<Url, ProviderError> {
        self.definition
            .websocket_url_for_path(path)
            .map_err(|error| ProviderError::InvalidRequest(error.to_string()))
    }

    pub fn request_url_for_path(&self, path: &str) -> Result<Url, ProviderError> {
        Url::parse(&self.definition.url_for_path(path))
            .map_err(|error| ProviderError::InvalidRequest(error.to_string()))
    }

    pub fn disable_websockets(&self) {
        self.state.disable_websockets.store(true, Ordering::Relaxed);
    }

    pub fn websockets_enabled(&self) -> bool {
        self.definition.capabilities.supports_websockets
            && !self.state.disable_websockets.load(Ordering::Relaxed)
    }

    pub fn set_connection_reused(&self, connection_reused: bool) {
        self.state
            .websocket_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_connection_reused(connection_reused);
    }

    pub fn connection_reused(&self) -> bool {
        self.state
            .websocket_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connection_reused()
    }

    pub fn build_websocket_headers(
        &self,
        credentials: &ProviderCredentials,
        turn_metadata_header: Option<&str>,
    ) -> Result<HeaderMap, ProviderError> {
        let mut headers = self.definition.build_headers(credentials)?;
        if let Some(turn_state) = self.state.turn_state.get()
            && let Ok(value) = HeaderValue::from_str(turn_state)
        {
            headers.insert("x-codex-turn-state", value);
        }
        if let Some(value) = turn_metadata_header
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert("x-codex-turn-metadata", value);
        }
        Ok(headers)
    }

    pub fn set_turn_state(&self, turn_state: impl Into<String>) -> bool {
        self.state.turn_state.set(turn_state.into()).is_ok()
    }

    pub fn turn_state(&self) -> Option<String> {
        self.state.turn_state.get().cloned()
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let credentials = self.credential_source.credentials().await?;
        let response = self
            .client
            .get(
                self.definition
                    .request_url_with_auth_for_path("v1/models", &credentials)?,
            )
            .headers(self.definition.build_headers(&credentials)?)
            .send()
            .await
            .map_err(ProviderError::Transport)?;

        if !response.status().is_success() {
            return Err(ProviderError::Http {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }

        let models = response
            .json::<ResponsesModelsPage>()
            .await
            .map_err(ProviderError::Decode)?;

        Ok(models.into_model_info(self.definition.descriptor.id.clone()))
    }

    pub async fn stream_response<'a>(
        &self,
        request: Request<'a>,
    ) -> Result<ProviderEventStream, ProviderError> {
        let provider_name = self
            .definition
            .descriptor
            .display_name
            .as_deref()
            .unwrap_or(self.definition.descriptor.id.as_str());
        let request = ResponsesRequest::try_from_request(request, provider_name)?;
        let credentials = self.credential_source.credentials().await?;
        let response = self
            .client
            .post(
                self.definition
                    .request_url_with_auth_for_path("v1/responses", &credentials)?,
            )
            .headers(self.definition.build_headers(&credentials)?)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&request)
            .send()
            .await
            .map_err(ProviderError::Transport)?;

        if !response.status().is_success() {
            return Err(ProviderError::Http {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }

        Ok(spawn_event_stream(response))
    }

    pub async fn send_response<'a>(&self, request: Request<'a>) -> Result<Response, ProviderError> {
        collect_response_from_stream(self.stream_response(request).await?).await
    }

    pub fn take_turn_state(&self) -> Arc<OnceLock<String>> {
        Arc::clone(&self.state.turn_state)
    }

    pub fn last_response_rx_ready(&self) -> bool {
        let mut session = self
            .state
            .websocket_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        session
            .last_response_rx
            .as_mut()
            .is_some_and(|rx| matches!(rx.try_recv(), Ok(_) | Err(TryRecvError::Closed)))
    }
}

#[async_trait::async_trait]
impl<C> ProviderSession for ResponsesSession<C>
where
    C: CredentialSource + 'static,
{
    async fn stream(&self, request: Request<'_>) -> Result<ProviderEventStream, ProviderError> {
        self.stream_response(request).await
    }
}

pub async fn collect_response_from_stream(
    mut stream: ProviderEventStream,
) -> Result<Response, ProviderError> {
    let mut builder = StreamingResponseBuilder::default();

    while let Some(event) = stream.recv().await {
        builder.apply(event?)?;
    }

    builder.build()
}

#[derive(Default)]
struct StreamingResponseBuilder {
    id: Option<String>,
    model: Option<String>,
    role: Option<Role>,
    blocks: std::collections::BTreeMap<usize, StreamingContentBlock>,
    stop_reason: Option<String>,
    usage: Option<TokenUsage>,
    stopped: bool,
}

impl StreamingResponseBuilder {
    fn apply(&mut self, event: ProviderEvent) -> Result<(), ProviderError> {
        match event {
            ProviderEvent::MessageStarted { id, model, role } => {
                self.id = Some(id);
                self.model = Some(model);
                self.role = Some(role);
            }
            ProviderEvent::ContentBlockStarted { index, kind } => {
                self.blocks.insert(index, StreamingContentBlock::from(kind));
            }
            ProviderEvent::ContentBlockDelta { index, delta } => {
                let block = self.blocks.get_mut(&index).ok_or_else(|| {
                    ProviderError::MalformedStream(format!(
                        "content block delta received before start for index {index}"
                    ))
                })?;
                block.apply_delta(delta)?;
            }
            ProviderEvent::ContentBlockStopped { index } => {
                let block = self.blocks.get_mut(&index).ok_or_else(|| {
                    ProviderError::MalformedStream(format!(
                        "content block stop received before start for index {index}"
                    ))
                })?;
                block.mark_complete();
            }
            ProviderEvent::MessageDelta { stop_reason, usage } => {
                self.stop_reason = stop_reason;
                self.usage = usage;
            }
            ProviderEvent::MessageStopped => {
                self.stopped = true;
            }
        }

        Ok(())
    }

    fn build(self) -> Result<Response, ProviderError> {
        if !self.stopped {
            return Err(ProviderError::MalformedStream(
                "message stream ended before MessageStopped".to_string(),
            ));
        }

        let id = self
            .id
            .ok_or_else(|| ProviderError::MalformedStream("missing message id".to_string()))?;
        let model = self
            .model
            .ok_or_else(|| ProviderError::MalformedStream("missing model id".to_string()))?;
        let role = self
            .role
            .ok_or_else(|| ProviderError::MalformedStream("missing message role".to_string()))?;
        let mut content = Vec::with_capacity(self.blocks.len());

        for (index, block) in self.blocks {
            if !block.is_complete() {
                return Err(ProviderError::MalformedStream(format!(
                    "content block {index} did not complete"
                )));
            }
            content.push(block.try_into_content_block()?);
        }

        Ok(Response {
            id,
            model,
            role,
            content,
            stop_reason: self.stop_reason,
            usage: self.usage,
        })
    }
}

enum StreamingContentBlock {
    Text {
        text: String,
        complete: bool,
    },
    Image {
        source: crate::ImageSource,
        complete: bool,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
        complete: bool,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        complete: bool,
    },
}

impl StreamingContentBlock {
    fn apply_delta(&mut self, delta: ContentBlockDelta) -> Result<(), ProviderError> {
        match (self, delta) {
            (StreamingContentBlock::Text { text, .. }, ContentBlockDelta::Text(delta)) => {
                text.push_str(&delta);
                Ok(())
            }
            (
                StreamingContentBlock::ToolUse { input_json, .. },
                ContentBlockDelta::ToolUseInputJson(delta),
            ) => {
                input_json.push_str(&delta);
                Ok(())
            }
            (
                StreamingContentBlock::ToolResult { content, .. },
                ContentBlockDelta::ToolResultContent(delta),
            ) => {
                content.push_str(&delta);
                Ok(())
            }
            (block, delta) => Err(ProviderError::MalformedStream(format!(
                "delta {delta:?} is not valid for block {}",
                block.kind_name()
            ))),
        }
    }

    fn mark_complete(&mut self) {
        match self {
            StreamingContentBlock::Text { complete, .. }
            | StreamingContentBlock::Image { complete, .. }
            | StreamingContentBlock::ToolUse { complete, .. }
            | StreamingContentBlock::ToolResult { complete, .. } => *complete = true,
        }
    }

    fn is_complete(&self) -> bool {
        match self {
            StreamingContentBlock::Text { complete, .. }
            | StreamingContentBlock::Image { complete, .. }
            | StreamingContentBlock::ToolUse { complete, .. }
            | StreamingContentBlock::ToolResult { complete, .. } => *complete,
        }
    }

    fn try_into_content_block(self) -> Result<ContentBlock, ProviderError> {
        match self {
            StreamingContentBlock::Text { text, .. } => Ok(ContentBlock::Text { text }),
            StreamingContentBlock::Image { source, .. } => Ok(ContentBlock::Image { source }),
            StreamingContentBlock::ToolUse {
                id,
                name,
                input_json,
                ..
            } => Ok(ContentBlock::ToolUse {
                id,
                name,
                input: serde_json::from_str(&input_json).map_err(ProviderError::Deserialize)?,
            }),
            StreamingContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => Ok(ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            }),
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            StreamingContentBlock::Text { .. } => "text",
            StreamingContentBlock::Image { .. } => "image",
            StreamingContentBlock::ToolUse { .. } => "tool_use",
            StreamingContentBlock::ToolResult { .. } => "tool_result",
        }
    }
}

impl From<ContentBlockStart> for StreamingContentBlock {
    fn from(value: ContentBlockStart) -> Self {
        match value {
            ContentBlockStart::Text => StreamingContentBlock::Text {
                text: String::new(),
                complete: false,
            },
            ContentBlockStart::Image { source } => StreamingContentBlock::Image {
                source,
                complete: false,
            },
            ContentBlockStart::ToolUse { id, name } => StreamingContentBlock::ToolUse {
                id,
                name,
                input_json: String::new(),
                complete: false,
            },
            ContentBlockStart::ToolResult {
                tool_use_id,
                is_error,
            } => StreamingContentBlock::ToolResult {
                tool_use_id,
                content: String::new(),
                is_error,
                complete: false,
            },
        }
    }
}
