use crate::provider::model::{ModelProviderKind, ProviderError};

#[derive(Debug)]
pub enum RuntimeError {
    ProviderNotFound(Option<ModelProviderKind>),
    FailedToSendRequest(ProviderError),
    FailedToListModels(ProviderError),
    FailedToStreamResponse(ProviderError),
    InvalidToolUseInput {
        id: String,
        name: String,
        source: serde_json::Error,
    },
    MalformedProviderEvent(String),
}
