use serde::Serialize;

use super::ExecutionContextError;
use super::types::ExecutionContextItem;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContextListOutput {
    pub(super) contexts: Vec<ExecutionContextItem>,
    pub(super) active: Vec<ExecutionContextItem>,
    pub(super) kept: Vec<ExecutionContextItem>,
    pub(super) removed: Vec<ExecutionContextItem>,
}

pub(super) fn serialize_pretty<T>(value: &T) -> Result<String, ExecutionContextError>
where
    T: Serialize,
{
    serde_json::to_string_pretty(value).map_err(ExecutionContextError::Serde)
}
