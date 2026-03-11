use crate::provider::model::{ModelProviderKind, ProviderError};

#[derive(Debug)]
pub enum RuntimeError {
    ProviderNotFound(Option<ModelProviderKind>),
    FailedToSendRequest(ProviderError),
    FailedToListModels(ProviderError),
    FailedToStreamResponse(ProviderError),
    FailedToCompactHistory(ProviderError),
    FailedToPersistTranscript(std::io::Error),
    FailedToSerializeTranscript(serde_json::Error),
    MaxRoundsExceeded(usize),
    InvalidToolUseInput {
        id: String,
        name: String,
        source: serde_json::Error,
    },
    MalformedProviderEvent(String),
}
