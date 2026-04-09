use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ProviderError;

/// Static metadata about an embedding model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingModelInfo {
    pub id: String,
    pub dimensions: usize,
    pub max_tokens: usize,
}

impl EmbeddingModelInfo {
    pub fn new(id: impl Into<String>, dimensions: usize, max_tokens: usize) -> Self {
        Self {
            id: id.into(),
            dimensions,
            max_tokens,
        }
    }
}

/// Input variants for an embedding request.
///
/// Only `Serialize` is derived — these types are sent in requests but never
/// deserialized from responses, so `Deserialize` is not needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum EmbeddingInput<'a> {
    Single(&'a str),
    Batch(&'a [&'a str]),
}

/// Request payload sent to the embeddings endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingRequest<'a> {
    pub model: &'a str,
    pub input: EmbeddingInput<'a>,
}

impl<'a> EmbeddingRequest<'a> {
    pub fn single(model: &'a str, text: &'a str) -> Self {
        Self {
            model,
            input: EmbeddingInput::Single(text),
        }
    }

    pub fn batch(model: &'a str, texts: &'a [&'a str]) -> Self {
        Self {
            model,
            input: EmbeddingInput::Batch(texts),
        }
    }
}

/// A single embedding vector with its position in the batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub index: usize,
    pub embedding: Vec<f32>,
}

/// Token usage reported by the embeddings endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

/// Response returned from the embeddings endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

/// Trait implemented by providers that support vector embeddings.
///
/// `EmbeddingProvider` is intentionally separate from `Provider` because not all
/// LLM providers expose an embeddings endpoint (e.g. Anthropic and Gemini do not).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single piece of text.
    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let texts = [text];
        let mut response = self.embed_batch(model, &texts).await?;
        response
            .data
            .pop()
            .map(|d| d.embedding)
            .ok_or_else(|| ProviderError::InvalidResponse("empty embedding response".to_string()))
    }

    /// Embed a batch of texts, returning one vector per input in order.
    async fn embed_batch(
        &self,
        model: &str,
        texts: &[&str],
    ) -> Result<EmbeddingResponse, ProviderError>;

    /// Returns metadata about the embedding models available from this provider.
    fn embedding_models(&self) -> Vec<EmbeddingModelInfo>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_request_single_serializes_as_string() {
        let req = EmbeddingRequest::single("text-embedding-3-small", "hello world");
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "text-embedding-3-small");
        assert_eq!(json["input"], "hello world");
    }

    #[test]
    fn embedding_request_batch_serializes_as_array() {
        let texts = ["hello", "world"];
        let req = EmbeddingRequest::batch("text-embedding-3-small", &texts);
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "text-embedding-3-small");
        assert_eq!(json["input"], serde_json::json!(["hello", "world"]));
    }

    #[test]
    fn embedding_response_deserializes_correctly() {
        let raw = serde_json::json!({
            "data": [
                { "index": 0, "embedding": [0.1, 0.2, 0.3] },
                { "index": 1, "embedding": [0.4, 0.5, 0.6] }
            ],
            "model": "text-embedding-3-small",
            "usage": { "prompt_tokens": 5, "total_tokens": 5 }
        });

        let response: EmbeddingResponse = serde_json::from_value(raw).unwrap();

        assert_eq!(response.model, "text-embedding-3-small");
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].index, 0);
        assert_eq!(response.data[0].embedding, vec![0.1f32, 0.2, 0.3]);
        assert_eq!(response.data[1].index, 1);
        assert_eq!(response.usage.prompt_tokens, 5);
        assert_eq!(response.usage.total_tokens, 5);
    }

    #[test]
    fn embedding_model_info_stores_fields() {
        let info = EmbeddingModelInfo::new("text-embedding-3-large", 3072, 8191);
        assert_eq!(info.id, "text-embedding-3-large");
        assert_eq!(info.dimensions, 3072);
        assert_eq!(info.max_tokens, 8191);
    }

    #[test]
    fn embedding_input_single_serializes_as_bare_string() {
        let input = EmbeddingInput::Single("test text");
        let serialized = serde_json::to_string(&input).unwrap();
        assert_eq!(serialized, r#""test text""#);
    }

    #[test]
    fn embedding_input_batch_serializes_as_array() {
        let texts = ["a", "b"];
        let input = EmbeddingInput::Batch(&texts);
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json, serde_json::json!(["a", "b"]));
    }
}
