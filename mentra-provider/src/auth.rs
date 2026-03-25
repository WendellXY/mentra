use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::ProviderError;

/// What kind of auth material a provider expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthScheme {
    #[default]
    None,
    BearerToken,
    Header {
        name: String,
    },
    QueryParam {
        name: String,
    },
}

/// Provider credentials resolved at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCredentials {
    pub bearer_token: Option<String>,
    pub account_id: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Supplies credentials on demand for a provider instance.
#[async_trait]
pub trait CredentialSource: Send + Sync {
    async fn credentials(&self) -> Result<ProviderCredentials, ProviderError>;

    async fn bearer_token(&self) -> Result<String, ProviderError> {
        self.credentials()
            .await?
            .bearer_token
            .ok_or_else(|| ProviderError::InvalidRequest("missing bearer token".to_string()))
    }
}
