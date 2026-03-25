use tokio::sync::mpsc;

use crate::{model::ImageSource, model::Role, model::TokenUsage};

pub type ProviderEventStream = mpsc::UnboundedReceiver<Result<ProviderEvent, crate::ProviderError>>;

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    MessageStarted {
        id: String,
        model: String,
        role: Role,
    },
    ContentBlockStarted {
        index: usize,
        kind: ContentBlockStart,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDelta,
    },
    ContentBlockStopped {
        index: usize,
    },
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<TokenUsage>,
    },
    MessageStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlockStart {
    Text,
    Image { source: ImageSource },
    ToolUse { id: String, name: String },
    ToolResult { tool_use_id: String, is_error: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlockDelta {
    Text(String),
    ToolUseInputJson(String),
    ToolResultContent(String),
}
