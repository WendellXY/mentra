use serde::Deserialize;
use serde_json::Value;

use super::types::ExecutionContextBackendKind;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextCreateInput {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) task_id: Option<u64>,
    #[serde(default)]
    pub(crate) backend: Option<ExecutionContextBackendKind>,
    #[serde(default)]
    pub(crate) from_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextGetInput {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextKeepInput {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextRemoveInput {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) force: bool,
    #[serde(default)]
    pub(crate) complete_task: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextListInput {}

pub(crate) fn parse_context_create_input(input: Value) -> Result<ContextCreateInput, String> {
    let parsed = serde_json::from_value::<ContextCreateInput>(input)
        .map_err(|error| format!("Invalid context_create input: {error}"))?;
    validate_name(&parsed.name)?;
    Ok(parsed)
}

pub(crate) fn parse_context_get_input(input: Value) -> Result<ContextGetInput, String> {
    let parsed = serde_json::from_value::<ContextGetInput>(input)
        .map_err(|error| format!("Invalid context_get input: {error}"))?;
    validate_name(&parsed.name)?;
    Ok(parsed)
}

pub(crate) fn parse_context_keep_input(input: Value) -> Result<ContextKeepInput, String> {
    let parsed = serde_json::from_value::<ContextKeepInput>(input)
        .map_err(|error| format!("Invalid context_keep input: {error}"))?;
    validate_name(&parsed.name)?;
    Ok(parsed)
}

pub(crate) fn parse_context_remove_input(input: Value) -> Result<ContextRemoveInput, String> {
    let parsed = serde_json::from_value::<ContextRemoveInput>(input)
        .map_err(|error| format!("Invalid context_remove input: {error}"))?;
    validate_name(&parsed.name)?;
    Ok(parsed)
}

pub(crate) fn parse_context_list_input(input: Value) -> Result<(), String> {
    serde_json::from_value::<ContextListInput>(input)
        .map(|_| ())
        .map_err(|error| format!("Invalid context_list input: {error}"))
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Execution context name must not be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Execution context name must not be '.' or '..'".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("Execution context name must not contain path separators".to_string());
    }
    Ok(())
}
