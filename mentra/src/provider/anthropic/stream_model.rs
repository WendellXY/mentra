use serde::Deserialize;

use crate::provider::model::{ContentBlockDelta, ContentBlockStart, ProviderEvent, Role};

use super::model::AnthropicResponse;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicResponse,
    },
    ContentBlockStart {
        index: usize,
        content_block: AnthropicStreamContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: AnthropicContentBlockDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: AnthropicMessageDelta,
    },
    MessageStop,
    Ping,
    Error {
        error: AnthropicStreamError,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicStreamContentBlock {
    Text {},
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Unsupported,
}

impl AnthropicStreamContentBlock {
    pub(crate) fn into_provider_start(self) -> Option<ContentBlockStart> {
        match self {
            AnthropicStreamContentBlock::Text {} => Some(ContentBlockStart::Text),
            AnthropicStreamContentBlock::ToolUse { id, name } => {
                Some(ContentBlockStart::ToolUse { id, name })
            }
            AnthropicStreamContentBlock::Unsupported => None,
        }
    }

    pub(crate) fn is_supported(&self) -> bool {
        !matches!(self, AnthropicStreamContentBlock::Unsupported)
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicContentBlockDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unsupported,
}

impl AnthropicContentBlockDelta {
    pub(crate) fn into_provider_delta(self) -> Option<ContentBlockDelta> {
        match self {
            AnthropicContentBlockDelta::TextDelta { text } => Some(ContentBlockDelta::Text(text)),
            AnthropicContentBlockDelta::InputJsonDelta { partial_json } => {
                Some(ContentBlockDelta::ToolUseInputJson(partial_json))
            }
            AnthropicContentBlockDelta::Unsupported => None,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct AnthropicMessageDelta {
    pub(crate) stop_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct AnthropicStreamError {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) message: String,
}

impl AnthropicStreamEvent {
    pub(crate) fn into_provider_event(self) -> Result<Option<ProviderEvent>, AnthropicStreamError> {
        match self {
            AnthropicStreamEvent::MessageStart { message } => {
                Ok(Some(ProviderEvent::MessageStarted {
                    id: message.id,
                    model: message.model,
                    role: match message.role.as_str() {
                        "user" => Role::User,
                        "assistant" => Role::Assistant,
                        _ => Role::Unknown(message.role),
                    },
                }))
            }
            AnthropicStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => Ok(content_block
                .into_provider_start()
                .map(|kind| ProviderEvent::ContentBlockStarted { index, kind })),
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => Ok(delta
                .into_provider_delta()
                .map(|delta| ProviderEvent::ContentBlockDelta { index, delta })),
            AnthropicStreamEvent::ContentBlockStop { index } => {
                Ok(Some(ProviderEvent::ContentBlockStopped { index }))
            }
            AnthropicStreamEvent::MessageDelta { delta } => Ok(Some(ProviderEvent::MessageDelta {
                stop_reason: delta.stop_reason,
                usage: None,
            })),
            AnthropicStreamEvent::MessageStop => Ok(Some(ProviderEvent::MessageStopped)),
            AnthropicStreamEvent::Ping => Ok(None),
            AnthropicStreamEvent::Error { error } => Err(error),
        }
    }
}
