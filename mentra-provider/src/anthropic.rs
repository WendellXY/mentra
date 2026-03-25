use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

pub(crate) mod model;
pub(crate) mod sse;
pub(crate) mod stream_model;

use crate::{
    AuthScheme, BuiltinProvider, ModelCatalog, ModelInfo, ProviderCapabilities, ProviderDefinition,
    ProviderError, ProviderEventStream, ProviderSession, ProviderSessionFactory,
    RegisteredProvider, Request, WireApi,
};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: Url,
    definition: ProviderDefinition,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            api_key.into().parse().expect("Failed to parse API key"),
        );
        headers.insert(
            "anthropic-version",
            ANTHROPIC_VERSION
                .parse()
                .expect("Failed to parse Anthropic version"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build client");

        Self {
            client,
            base_url: Url::parse(DEFAULT_BASE_URL).expect("Failed to parse default base URL"),
            definition: Self::definition(),
        }
    }

    fn definition() -> ProviderDefinition {
        let mut definition = ProviderDefinition::new(BuiltinProvider::Anthropic);
        definition.descriptor.display_name = Some("Anthropic".to_string());
        definition.descriptor.description = Some("Anthropic Messages API provider".to_string());
        definition.wire_api = WireApi::AnthropicMessages;
        definition.auth_scheme = AuthScheme::Header {
            name: "x-api-key".to_string(),
        };
        definition.capabilities = ProviderCapabilities {
            supports_model_listing: true,
            supports_streaming: true,
            supports_websockets: false,
            supports_tool_calls: true,
            supports_images: true,
        };
        definition.base_url = Some(DEFAULT_BASE_URL.to_string());
        definition.headers = Some(HashMap::from([(
            "anthropic-version".to_string(),
            ANTHROPIC_VERSION.to_string(),
        )]));
        definition
    }
}

#[async_trait]
impl ModelCatalog for AnthropicProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut models = Vec::new();
        let mut after_id = None;

        loop {
            let response = self
                .client
                .get(
                    self.base_url
                        .join("v1/models")
                        .expect("Failed to join models URL"),
                )
                .query(&[
                    ("limit", "1000"),
                    ("after_id", after_id.as_deref().unwrap_or("")),
                ])
                .send()
                .await
                .map_err(ProviderError::Transport)?;

            if !response.status().is_success() {
                return Err(ProviderError::Http {
                    status: response.status(),
                    body: response.text().await.unwrap_or_default(),
                });
            }

            let page = response
                .json::<model::AnthropicModelsPage>()
                .await
                .map_err(ProviderError::Decode)?;

            after_id = page.last_id.clone();
            models.extend(page.data.into_iter().map(|model| model.into()));

            if !page.has_more {
                break;
            }
        }

        Ok(models)
    }
}

#[async_trait]
impl ProviderSessionFactory for AnthropicProvider {
    async fn create_session(&self) -> Result<Box<dyn ProviderSession>, ProviderError> {
        Ok(Box::new(self.clone()))
    }
}

#[async_trait]
impl ProviderSession for AnthropicProvider {
    async fn stream(&self, request: Request<'_>) -> Result<ProviderEventStream, ProviderError> {
        let response = self.send_message(request, true).await?;
        Ok(sse::spawn_event_stream(response))
    }
}

#[async_trait]
impl RegisteredProvider for AnthropicProvider {
    fn definition(&self) -> ProviderDefinition {
        self.definition.clone()
    }
}

impl AnthropicProvider {
    async fn send_message(
        &self,
        request: Request<'_>,
        stream: bool,
    ) -> Result<reqwest::Response, ProviderError> {
        let request = model::AnthropicRequest::try_from(request)?;
        let mut body = serde_json::to_value(request).map_err(ProviderError::Serialize)?;
        if stream {
            body["stream"] = Value::Bool(true);
        }
        let response = self
            .client
            .post(
                self.base_url
                    .join("v1/messages")
                    .expect("Failed to join messages URL"),
            )
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
