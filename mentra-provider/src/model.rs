use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Display;
use time::OffsetDateTime;

/// Metadata describing a model available from a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: crate::ProviderId,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>, provider: impl Into<crate::ProviderId>) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            display_name: None,
            description: None,
            created_at: None,
        }
    }
}

/// Selection strategy used when resolving a model from a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelector {
    Id(String),
    NewestAvailable,
}

/// Provider-neutral token usage metadata for a completed or in-progress response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub thoughts_tokens: Option<u64>,
    pub tool_input_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
            && self.reasoning_tokens.is_none()
            && self.thoughts_tokens.is_none()
            && self.tool_input_tokens.is_none()
    }
}

/// Provider-neutral chat role labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Unknown(String),
}

impl Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Unknown(role) => role.as_str(),
        };
        f.write_str(value)
    }
}

/// Image payload supported by model providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageSource {
    Bytes { media_type: String, data: Vec<u8> },
    Url { url: String },
}

impl ImageSource {
    pub fn bytes(media_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self::Url { url: url.into() }
    }
}

/// A provider-neutral content block exchanged with models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image_bytes(media_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::Image {
            source: ImageSource::bytes(media_type, data),
        }
    }

    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image {
            source: ImageSource::url(url),
        }
    }
}

/// Provider-neutral chat message content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(content: ContentBlock) -> Self {
        Self {
            role: Role::User,
            content: vec![content],
        }
    }

    pub fn assistant(content: ContentBlock) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![content],
        }
    }

    pub fn unknown(role: impl Into<String>, content: ContentBlock) -> Self {
        Self {
            role: Role::Unknown(role.into()),
            content: vec![content],
        }
    }

    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Provider-neutral tool choice hint passed to model APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolChoice {
    #[default]
    Auto,
    Any,
    Tool {
        name: String,
    },
}
