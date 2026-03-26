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
    CredentialSource, ModelInfo, ProviderCredentials, ProviderDefinition, ProviderError,
    ProviderEventStream, ProviderSession, Request, Response, SessionRequestOptions,
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
        let session = SessionRequestOptions {
            sticky_turn_state: self.state.turn_state.get().cloned(),
            turn_metadata: turn_metadata_header.map(str::to_string),
            prefer_connection_reuse: Some(self.connection_reused()),
            session_affinity: None,
        };
        self.build_websocket_headers_for_session(credentials, Some(&session))
    }

    pub fn build_websocket_headers_for_session(
        &self,
        credentials: &ProviderCredentials,
        session: Option<&SessionRequestOptions>,
    ) -> Result<HeaderMap, ProviderError> {
        let mut headers = self.definition.build_headers(credentials)?;
        if let Some(turn_state) = session
            .and_then(|session| session.sticky_turn_state.as_deref())
            .or_else(|| self.state.turn_state.get().map(String::as_str))
            && let Ok(value) = HeaderValue::from_str(turn_state)
        {
            headers.insert("x-mentra-turn-state", value.clone());
            headers.insert("x-codex-turn-state", value);
        }
        if let Some(value) = session.and_then(|session| session.turn_metadata.as_deref())
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert("x-mentra-turn-metadata", value.clone());
            headers.insert("x-codex-turn-metadata", value);
        }
        if let Some(value) = session.and_then(|session| session.session_affinity.as_deref())
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert("x-mentra-session-affinity", value);
        }
        if let Some(prefer_connection_reuse) =
            session.and_then(|session| session.prefer_connection_reuse)
        {
            headers.insert(
                "x-mentra-connection-reuse",
                HeaderValue::from_static(if prefer_connection_reuse {
                    "prefer-reuse"
                } else {
                    "prefer-fresh"
                }),
            );
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
        crate::collect_response_from_stream(self.stream_response(request).await?).await
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
