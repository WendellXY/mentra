#![doc = include_str!("../README.md")]

mod default_paths;

pub use mentra_provider as provider_core;

/// Agent configuration, lifecycle, and event handling.
pub mod agent;
/// Optional OAuth helpers for provider authentication.
#[cfg(feature = "openai-oauth")]
pub mod auth;
/// Background task coordination types and services.
pub mod background;
/// Working-memory journal and long-term memory services.
pub mod memory;
/// Provider integrations and transport-neutral request/response types.
pub mod provider;
/// Runtime orchestration, persistence, policies, and agent APIs.
pub mod runtime;
/// Team coordination types and collaboration services.
pub mod team;
/// Optional test helpers for deterministic scripted runtimes.
#[cfg(any(test, feature = "test-utils"))]
pub mod test;
/// Tool traits, metadata, and builtin tools.
pub mod tool;

pub use mentra_provider::{
    AnthropicRequestOptions, BuiltinProvider, ContentBlock, ContentBlockDelta, ContentBlockStart,
    GeminiRequestOptions, ImageSource, Message, ModelInfo, ModelSelector, OpenAIRequestOptions,
    ProviderCapabilities, ProviderCredentials, ProviderDefinition, ProviderDescriptor,
    ProviderError, ProviderEvent, ProviderEventStream, ProviderId, ProviderRequestOptions,
    ReasoningEffort, ReasoningOptions, Request, ResponsesRequestOptions, RetryPolicy, Role,
    TokenUsage, ToolChoice, ToolSearchMode, WireApi, collect_response_from_stream,
    provider_event_stream_from_response,
};

pub use provider::{Provider, ProviderRegistry};

pub use agent::{Agent, AgentConfig};
pub use background::{BackgroundNotification, BackgroundTaskStatus, BackgroundTaskSummary};
pub use runtime::{
    AgentStore, AuditStore, HybridRuntimeStore, LeaseStore, RunStore, Runtime, RuntimePolicy,
    TaskStore,
};
pub use team::{
    TeamDispatch, TeamMemberStatus, TeamMemberSummary, TeamMessage, TeamMessageKind,
    TeamProtocolRequestSummary, TeamProtocolStatus,
};

pub mod error {
    pub use crate::provider::ProviderError;
    pub use crate::runtime::RuntimeError;
}
