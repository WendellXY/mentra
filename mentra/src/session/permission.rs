use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::event::PermissionRuleScope;

/// A pending permission request awaiting a UI decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub description: String,
    /// JSON-encoded preview data. Stored as `String` because
    /// `serde_json::Value` does not implement `Eq`.
    pub preview: String,
}

/// The response to a permission request from the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub allow: bool,
    pub remember_as: Option<PermissionRuleScope>,
}

impl PermissionDecision {
    /// Allow the tool call without remembering.
    pub fn allow() -> Self {
        Self {
            allow: true,
            remember_as: None,
        }
    }

    /// Deny the tool call without remembering.
    pub fn deny() -> Self {
        Self {
            allow: false,
            remember_as: None,
        }
    }

    /// Allow the tool call and remember the decision for the given scope.
    pub fn allow_and_remember(scope: PermissionRuleScope) -> Self {
        Self {
            allow: true,
            remember_as: Some(scope),
        }
    }

    /// Deny the tool call and remember the decision for the given scope.
    pub fn deny_and_remember(scope: PermissionRuleScope) -> Self {
        Self {
            allow: false,
            remember_as: Some(scope),
        }
    }
}

/// Key for looking up remembered permission rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleKey {
    pub tool_name: String,
    pub pattern: Option<String>,
}

/// A stored permission rule that was previously decided by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedRule {
    pub key: RuleKey,
    pub allow: bool,
    pub scope: PermissionRuleScope,
}

/// Thread-safe in-memory store for remembered permission rules.
#[derive(Debug, Clone)]
pub struct RuleStore {
    inner: Arc<Mutex<HashMap<RuleKey, RememberedRule>>>,
}

impl Default for RuleStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleStore {
    /// Creates an empty rule store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Adds or overwrites a remembered rule.
    pub fn add_rule(&self, rule: RememberedRule) {
        let mut rules = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        rules.insert(rule.key.clone(), rule);
    }

    /// Checks whether a tool is allowed by a remembered rule.
    ///
    /// Returns `Some(true)` if allowed, `Some(false)` if denied, or `None` if
    /// no matching rule exists.
    pub fn check(&self, tool_name: &str) -> Option<bool> {
        let rules = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = RuleKey {
            tool_name: tool_name.to_owned(),
            pattern: None,
        };
        rules.get(&key).map(|rule| rule.allow)
    }

    /// Returns all remembered rules as a vector.
    pub fn rules(&self) -> Vec<RememberedRule> {
        let rules = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        rules.values().cloned().collect()
    }

    /// Removes all rules that match the given scope.
    pub fn clear_scope(&self, scope: PermissionRuleScope) {
        let mut rules = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        rules.retain(|_, rule| rule.scope != scope);
    }
}

/// Internal entry tracking a pending permission with its oneshot response channel.
pub(crate) struct PendingPermissionEntry {
    pub(crate) tool_call_id: String,
    pub(crate) tool_name: String,
    pub(crate) sender: oneshot::Sender<PermissionDecision>,
}
