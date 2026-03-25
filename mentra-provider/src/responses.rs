pub mod model;
pub mod session;
pub mod sse;

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use http::{HeaderMap, HeaderName, HeaderValue, header};

use crate::{
    AuthScheme, BuiltinProvider, CredentialSource, ModelCatalog, ModelInfo, ProviderCapabilities,
    ProviderCredentials, ProviderDefinition, ProviderError, ProviderSessionFactory,
    RegisteredProvider, RetryPolicy, WireApi,
};

use self::session::{ResponsesSession, ResponsesSessionState};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/";

pub fn openai(api_key: impl Into<String>) -> ResponsesProvider<StaticCredentialSource> {
    ResponsesProvider::openai(api_key)
}

pub fn openrouter(api_key: impl Into<String>) -> ResponsesProvider<StaticCredentialSource> {
    ResponsesProvider::openrouter(api_key)
}

/// Shared Responses-family provider implementation.
///
/// This type owns the provider definition, credential source, client, and transport state while
/// the request mapping and SSE decoding live in the sibling modules.
#[derive(Clone)]
pub struct ResponsesProvider<C> {
    definition: ProviderDefinition,
    credential_source: Arc<C>,
    client: reqwest::Client,
    session_state: Arc<ResponsesSessionState>,
}

impl<C> ResponsesProvider<C>
where
    C: CredentialSource + 'static,
{
    pub fn new(definition: ProviderDefinition, credential_source: C) -> Self {
        Self::with_shared_credential_source(definition, Arc::new(credential_source))
    }

    pub fn with_shared_credential_source(
        definition: ProviderDefinition,
        credential_source: Arc<C>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("failed to build reqwest client");
        Self {
            definition,
            credential_source,
            client,
            session_state: Arc::new(ResponsesSessionState::default()),
        }
    }

    pub fn definition(&self) -> &ProviderDefinition {
        &self.definition
    }

    pub fn session(&self) -> ResponsesSession<C> {
        ResponsesSession::new(
            self.definition.clone(),
            Arc::clone(&self.credential_source),
            self.client.clone(),
            Arc::clone(&self.session_state),
        )
    }
}

impl ResponsesProvider<StaticCredentialSource> {
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::with_shared_credential_source(
            build_definition(
                BuiltinProvider::OpenAI,
                "OpenAI",
                "OpenAI Responses API provider",
                DEFAULT_OPENAI_BASE_URL,
            ),
            Arc::new(StaticCredentialSource::new(api_key)),
        )
    }

    pub fn openrouter(api_key: impl Into<String>) -> Self {
        Self::with_shared_credential_source(
            build_definition(
                BuiltinProvider::OpenRouter,
                "OpenRouter",
                "OpenRouter Responses API provider",
                DEFAULT_OPENROUTER_BASE_URL,
            ),
            Arc::new(StaticCredentialSource::new(api_key)),
        )
    }
}

fn build_definition(
    builtin: BuiltinProvider,
    display_name: &str,
    description: &str,
    base_url: &str,
) -> ProviderDefinition {
    let mut definition = ProviderDefinition::new(builtin);
    definition.descriptor.display_name = Some(display_name.to_string());
    definition.descriptor.description = Some(description.to_string());
    definition.wire_api = WireApi::Responses;
    definition.auth_scheme = AuthScheme::BearerToken;
    definition.capabilities = ProviderCapabilities {
        supports_model_listing: true,
        supports_streaming: true,
        supports_websockets: true,
        supports_tool_calls: true,
        supports_images: true,
    };
    definition.base_url = Some(base_url.to_string());
    definition.headers = Some(HashMap::new());
    definition.retry = RetryPolicy::default();
    definition
}

pub(crate) fn build_header_map(
    headers: Option<&HashMap<String, String>>,
) -> Result<HeaderMap, ProviderError> {
    let mut map = HeaderMap::new();

    if let Some(headers) = headers {
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ProviderError::InvalidRequest(format!(
                    "invalid provider header name {name:?}: {error}"
                ))
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|error| {
                ProviderError::InvalidRequest(format!(
                    "invalid provider header value for {name:?}: {error}"
                ))
            })?;
            map.insert(header_name, header_value);
        }
    }

    Ok(map)
}

pub(crate) fn build_request_headers(
    definition: &ProviderDefinition,
    credentials: &ProviderCredentials,
) -> Result<HeaderMap, ProviderError> {
    let mut headers = build_header_map(definition.headers.as_ref())?;

    for (name, value) in &credentials.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "invalid credential header name {name:?}: {error}"
            ))
        })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "invalid credential header value for {name:?}: {error}"
            ))
        })?;
        headers.insert(header_name, header_value);
    }

    if let Some(token) = &credentials.bearer_token {
        let auth_value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| {
            ProviderError::InvalidRequest(format!("invalid bearer token header: {error}"))
        })?;
        headers.insert(header::AUTHORIZATION, auth_value);
    }

    Ok(headers)
}

/// Supplies a static bearer token to the provider.
#[derive(Clone)]
pub struct StaticCredentialSource {
    api_key: Arc<str>,
}

impl StaticCredentialSource {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Arc::from(api_key.into()),
        }
    }
}

#[async_trait]
impl CredentialSource for StaticCredentialSource {
    async fn credentials(&self) -> Result<ProviderCredentials, ProviderError> {
        Ok(ProviderCredentials {
            bearer_token: Some(self.api_key.to_string()),
            account_id: None,
            headers: HashMap::new(),
        })
    }
}

#[async_trait]
impl<C> ModelCatalog for ResponsesProvider<C>
where
    C: CredentialSource + 'static,
{
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let credentials = self.credential_source.credentials().await?;
        let mut request = self.client.get(self.definition.url_for_path("v1/models"));
        let headers = build_request_headers(&self.definition, &credentials)?;
        request = request.headers(headers);

        let response = request.send().await.map_err(ProviderError::Transport)?;

        if !response.status().is_success() {
            return Err(ProviderError::Http {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }

        let models = response
            .json::<self::model::ResponsesModelsPage>()
            .await
            .map_err(ProviderError::Decode)?;

        Ok(models.into_model_info(self.definition.descriptor.id.clone()))
    }
}

#[async_trait]
impl<C> ProviderSessionFactory for ResponsesProvider<C>
where
    C: CredentialSource + 'static,
{
    async fn create_session(&self) -> Result<Box<dyn crate::ProviderSession>, ProviderError> {
        Ok(Box::new(self.session()))
    }
}

#[async_trait]
impl<C> RegisteredProvider for ResponsesProvider<C>
where
    C: CredentialSource + 'static,
{
    fn definition(&self) -> ProviderDefinition {
        self.definition.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderId;

    #[test]
    fn openai_preset_uses_responses_wire_api() {
        let provider = openai("test-key");
        let definition = provider.definition();

        assert_eq!(
            definition.descriptor.id,
            ProviderId::from(BuiltinProvider::OpenAI)
        );
        assert_eq!(
            definition.descriptor.display_name.as_deref(),
            Some("OpenAI")
        );
        assert_eq!(definition.wire_api, WireApi::Responses);
        assert!(definition.capabilities.supports_websockets);
        assert_eq!(
            definition.base_url.as_deref(),
            Some(DEFAULT_OPENAI_BASE_URL)
        );
    }

    #[test]
    fn openrouter_preset_uses_openrouter_base_url() {
        let provider = openrouter("test-key");
        let definition = provider.definition();

        assert_eq!(
            definition.descriptor.id,
            ProviderId::from(BuiltinProvider::OpenRouter)
        );
        assert_eq!(
            definition.descriptor.display_name.as_deref(),
            Some("OpenRouter")
        );
        assert_eq!(definition.wire_api, WireApi::Responses);
        assert_eq!(
            definition.base_url.as_deref(),
            Some(DEFAULT_OPENROUTER_BASE_URL)
        );
    }
}
