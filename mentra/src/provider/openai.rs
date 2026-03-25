use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use url::Url;

pub(crate) mod model;
pub(crate) mod sse;

use crate::{
    BuiltinProvider,
    provider::{
        Provider,
        model::{ModelInfo, ProviderDescriptor, ProviderError, ProviderEventStream, Request},
    },
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/";

/// Supplies OpenAI API credentials on demand.
#[async_trait]
pub trait OpenAICredentialSource: Send + Sync {
    async fn api_key(&self) -> Result<String, String>;
}

#[derive(Clone)]
struct StaticOpenAICredentialSource {
    api_key: Arc<str>,
}

impl StaticOpenAICredentialSource {
    fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Arc::<str>::from(api_key.into()),
        }
    }
}

#[async_trait]
impl OpenAICredentialSource for StaticOpenAICredentialSource {
    async fn api_key(&self) -> Result<String, String> {
        Ok(self.api_key.to_string())
    }
}

#[derive(Clone)]
pub struct OpenAIProvider {
    client: reqwest::Client,
    base_url: Url,
    credential_source: Arc<dyn OpenAICredentialSource>,
    provider: BuiltinProvider,
    display_name: &'static str,
    description: &'static str,
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_credential_source(StaticOpenAICredentialSource::new(api_key))
    }

    pub fn with_credential_source(source: impl OpenAICredentialSource + 'static) -> Self {
        Self::with_shared_credential_source(Arc::new(source))
    }

    pub fn with_shared_credential_source(source: Arc<dyn OpenAICredentialSource>) -> Self {
        Self::with_provider_details(
            BuiltinProvider::OpenAI,
            "OpenAI",
            "OpenAI Responses API provider",
            DEFAULT_BASE_URL,
            source,
        )
    }

    pub(crate) fn openai_compatible(
        provider: BuiltinProvider,
        display_name: &'static str,
        description: &'static str,
        base_url: &str,
        api_key: impl Into<String>,
    ) -> Self {
        Self::with_provider_details(
            provider,
            display_name,
            description,
            base_url,
            Arc::new(StaticOpenAICredentialSource::new(api_key)),
        )
    }

    fn with_provider_details(
        provider: BuiltinProvider,
        display_name: &'static str,
        description: &'static str,
        base_url: &str,
        credential_source: Arc<dyn OpenAICredentialSource>,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .build()
                .expect("Failed to build client"),
            base_url: Url::parse(base_url).expect("Failed to parse provider base URL"),
            credential_source,
            provider,
            display_name,
            description,
        }
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.provider.into(),
            display_name: Some(self.display_name.to_string()),
            description: Some(self.description.to_string()),
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let api_key = self.load_api_key().await?;
        let response = self
            .client
            .get(
                self.base_url
                    .join("v1/models")
                    .expect("Failed to join models URL"),
            )
            .bearer_auth(api_key)
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
            .json::<model::OpenAIModelsPage>()
            .await
            .map_err(ProviderError::Decode)?;

        Ok(models.into_model_info(self.provider))
    }

    async fn stream(&self, request: Request<'_>) -> Result<ProviderEventStream, ProviderError> {
        let response = self.send_response(request, true).await?;
        Ok(sse::spawn_event_stream(response))
    }
}

impl OpenAIProvider {
    async fn load_api_key(&self) -> Result<String, ProviderError> {
        self.credential_source
            .api_key()
            .await
            .map_err(ProviderError::InvalidRequest)
    }

    async fn send_response(
        &self,
        request: Request<'_>,
        stream: bool,
    ) -> Result<reqwest::Response, ProviderError> {
        let api_key = self.load_api_key().await?;
        let request = model::OpenAIResponsesRequest::try_from_request(request, self.display_name)?;
        let mut body = serde_json::to_value(request).map_err(ProviderError::Serialize)?;
        if stream {
            body["stream"] = Value::Bool(true);
        }

        let response = self
            .client
            .post(
                self.base_url
                    .join("v1/responses")
                    .expect("Failed to join responses URL"),
            )
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(ProviderError::Transport)?;

        if !response.status().is_success() {
            return Err(ProviderError::Http {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }

        Ok(response)
    }
}
