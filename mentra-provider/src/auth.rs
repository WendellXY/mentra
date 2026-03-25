use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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
///
/// `bearer_token` is the provider's primary auth secret and is applied according
/// to the provider definition's [`AuthScheme`]. For example, Responses-family
/// providers send it as `Authorization: Bearer ...`, while header/query auth
/// providers can reuse the same resolved secret in a different location.
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

/// Supplies a fixed auth secret to the provider.
#[derive(Clone)]
pub struct StaticCredentialSource {
    secret: Arc<str>,
}

impl StaticCredentialSource {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: Arc::from(secret.into()),
        }
    }
}

#[async_trait]
impl CredentialSource for StaticCredentialSource {
    async fn credentials(&self) -> Result<ProviderCredentials, ProviderError> {
        Ok(ProviderCredentials {
            bearer_token: Some(self.secret.to_string()),
            account_id: None,
            headers: HashMap::new(),
        })
    }
}
