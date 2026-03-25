use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// High-level capability labels used for tool metadata and policy decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCapability {
    ReadOnly,
    FilesystemRead,
    FilesystemWrite,
    ProcessExec,
    BackgroundExec,
    TaskMutation,
    TeamCoordination,
    Delegation,
    ContextCompaction,
    SkillLoad,
    Custom(String),
}

/// Declares how much side effect a tool may have when executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolSideEffectLevel {
    #[default]
    None,
    LocalState,
    Process,
    External,
}

/// Declares whether a tool call is safe to replay or persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolDurability {
    #[default]
    Ephemeral,
    Persistent,
    ReplaySafe,
}

/// Declares whether a tool call may execute concurrently with other calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolExecutionMode {
    #[default]
    Exclusive,
    Parallel,
}

/// Declares whether a tool is loaded eagerly or deferred for provider-native tool search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolLoadingPolicy {
    #[default]
    Immediate,
    Deferred,
}

/// Provider-facing description of a tool and its input schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub capabilities: Vec<ToolCapability>,
    pub side_effect_level: ToolSideEffectLevel,
    pub durability: ToolDurability,
    #[serde(default)]
    pub loading_policy: ToolLoadingPolicy,
    pub execution_timeout: Option<Duration>,
}

impl ToolSpec {
    pub fn builder(name: impl Into<String>) -> ToolSpecBuilder {
        ToolSpecBuilder {
            name: name.into(),
            description: None,
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            capabilities: Vec::new(),
            side_effect_level: ToolSideEffectLevel::None,
            durability: ToolDurability::Ephemeral,
            loading_policy: ToolLoadingPolicy::Immediate,
            execution_timeout: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolSpecBuilder {
    name: String,
    description: Option<String>,
    input_schema: Value,
    capabilities: Vec<ToolCapability>,
    side_effect_level: ToolSideEffectLevel,
    durability: ToolDurability,
    loading_policy: ToolLoadingPolicy,
    execution_timeout: Option<Duration>,
}

impl ToolSpecBuilder {
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn input_schema(mut self, input_schema: Value) -> Self {
        self.input_schema = input_schema;
        self
    }

    pub fn capability(mut self, capability: ToolCapability) -> Self {
        self.capabilities.push(capability);
        self
    }

    pub fn capabilities(mut self, capabilities: impl IntoIterator<Item = ToolCapability>) -> Self {
        self.capabilities = capabilities.into_iter().collect();
        self
    }

    pub fn side_effect_level(mut self, side_effect_level: ToolSideEffectLevel) -> Self {
        self.side_effect_level = side_effect_level;
        self
    }

    pub fn durability(mut self, durability: ToolDurability) -> Self {
        self.durability = durability;
        self
    }

    pub fn loading_policy(mut self, loading_policy: ToolLoadingPolicy) -> Self {
        self.loading_policy = loading_policy;
        self
    }

    pub fn defer_loading(self, defer_loading: bool) -> Self {
        self.loading_policy(if defer_loading {
            ToolLoadingPolicy::Deferred
        } else {
            ToolLoadingPolicy::Immediate
        })
    }

    pub fn execution_timeout(mut self, execution_timeout: Duration) -> Self {
        self.execution_timeout = Some(execution_timeout);
        self
    }

    pub fn build(self) -> ToolSpec {
        ToolSpec {
            name: self.name,
            description: self.description,
            input_schema: self.input_schema,
            capabilities: self.capabilities,
            side_effect_level: self.side_effect_level,
            durability: self.durability,
            loading_policy: self.loading_policy,
            execution_timeout: self.execution_timeout,
        }
    }
}
