use async_trait::async_trait;

use crate::{
    BuiltinProvider,
    provider::{
        Provider,
        model::{ModelInfo, ProviderDescriptor, ProviderError, ProviderEventStream, Request},
        openai::OpenAIProvider,
    },
};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/";

#[derive(Clone)]
pub struct OllamaProvider {
    inner: OpenAIProvider,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    pub fn with_base_url(base_url: impl AsRef<str>) -> Self {
        Self {
            inner: OpenAIProvider::openai_compatible(
                BuiltinProvider::Ollama,
                "Ollama",
                "Ollama OpenAI-compatible Responses API provider",
                base_url.as_ref(),
                "ollama",
            ),
        }
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.list_models().await
    }

    async fn stream(&self, request: Request<'_>) -> Result<ProviderEventStream, ProviderError> {
        self.inner.stream(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::OllamaProvider;
    use crate::{BuiltinProvider, provider::Provider};

    #[test]
    fn descriptor_uses_ollama_identity() {
        let provider = OllamaProvider::new();
        let descriptor = provider.descriptor();

        assert_eq!(descriptor.id, BuiltinProvider::Ollama.into());
        assert_eq!(descriptor.display_name.as_deref(), Some("Ollama"));
    }
}
