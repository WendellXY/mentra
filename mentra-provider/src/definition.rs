use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap, fmt::Display, time::Duration};
use strum::{Display as StrumDisplay, IntoStaticStr};
use url::Url;

/// Builtin provider families Mentra can construct from presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum BuiltinProvider {
    Anthropic,
    Gemini,
    OpenAI,
    OpenRouter,
    Ollama,
    LmStudio,
}

impl From<BuiltinProvider> for ProviderId {
    fn from(value: BuiltinProvider) -> Self {
        Self(Cow::Borrowed(value.into()))
    }
}

/// Stable identifier for a registered provider implementation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ProviderId(Cow<'static, str>);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(Cow::Owned(id.into()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ProviderId {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl From<&String> for ProviderId {
    fn from(value: &String) -> Self {
        Self::new(value.as_str())
    }
}

/// Human-facing metadata about a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

impl ProviderDescriptor {
    pub fn new(id: impl Into<ProviderId>) -> Self {
        Self {
            id: id.into(),
            display_name: None,
            description: None,
        }
    }
}

/// Capabilities advertised by a provider instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub supports_model_listing: bool,
    pub supports_streaming: bool,
    pub supports_websockets: bool,
    pub supports_tool_calls: bool,
    pub supports_images: bool,
}

/// Wire protocol supported by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireApi {
    #[default]
    Responses,
    AnthropicMessages,
    GeminiGenerateContent,
}

impl Display for WireApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Responses => "responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GeminiGenerateContent => "gemini_generate_content",
        };
        f.write_str(value)
    }
}

/// Retry configuration for provider transport calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u64,
    pub base_delay: Duration,
    pub retry_429: bool,
    pub retry_5xx: bool,
    pub retry_transport: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(200),
            retry_429: false,
            retry_5xx: true,
            retry_transport: true,
        }
    }
}

/// Serializable provider definition used by runtime and adapter layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDefinition {
    pub descriptor: ProviderDescriptor,
    #[serde(default)]
    pub wire_api: WireApi,
    #[serde(default)]
    pub auth_scheme: crate::AuthScheme,
    #[serde(default)]
    pub capabilities: ProviderCapabilities,
    pub base_url: Option<String>,
    #[serde(default)]
    pub query_params: Option<HashMap<String, String>>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub retry: RetryPolicy,
    #[serde(default = "default_stream_idle_timeout")]
    pub stream_idle_timeout: Duration,
    #[serde(default = "default_websocket_connect_timeout")]
    pub websocket_connect_timeout: Duration,
}

fn default_stream_idle_timeout() -> Duration {
    Duration::from_millis(300_000)
}

fn default_websocket_connect_timeout() -> Duration {
    Duration::from_millis(15_000)
}

impl ProviderDefinition {
    pub fn new(id: impl Into<ProviderId>) -> Self {
        Self {
            descriptor: ProviderDescriptor::new(id),
            wire_api: WireApi::default(),
            auth_scheme: crate::AuthScheme::default(),
            capabilities: ProviderCapabilities {
                supports_model_listing: true,
                supports_streaming: true,
                supports_websockets: false,
                supports_tool_calls: true,
                supports_images: true,
            },
            base_url: None,
            query_params: None,
            headers: None,
            retry: RetryPolicy::default(),
            stream_idle_timeout: default_stream_idle_timeout(),
            websocket_connect_timeout: default_websocket_connect_timeout(),
        }
    }

    pub fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    pub fn provider_id(&self) -> &ProviderId {
        &self.descriptor.id
    }

    pub fn url_for_path(&self, path: &str) -> String {
        let base = self
            .base_url
            .as_deref()
            .unwrap_or_default()
            .trim_end_matches('/');
        let path = path.trim_start_matches('/');
        let mut url = if path.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{path}")
        };

        if let Some(params) = &self.query_params
            && !params.is_empty()
        {
            let qs = params
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&");
            url.push('?');
            url.push_str(&qs);
        }

        url
    }

    pub fn websocket_url_for_path(&self, path: &str) -> Result<Url, url::ParseError> {
        let mut url = Url::parse(&self.url_for_path(path))?;

        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            "ws" | "wss" => return Ok(url),
            _ => return Ok(url),
        };
        let _ = url.set_scheme(scheme);
        Ok(url)
    }
}
