use serde_json::Value;

use crate::tool::ToolExecutionCategory;

use super::schema::{FileOperation, FilesInput};

pub(crate) fn parse_files_input(input: &Value) -> Result<FilesInput, String> {
    serde_json::from_value::<FilesInput>(input.clone())
        .map_err(|error| format!("Invalid files input: {error}"))
}

pub(crate) fn ensure_files_have_operations(input: &FilesInput) -> Result<(), String> {
    if input.operations.is_empty() {
        Err("At least one file operation is required".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn file_execution_category(input: &Value) -> ToolExecutionCategory {
    let Ok(input) = parse_files_input(input) else {
        return ToolExecutionCategory::ExclusiveLocalMutation;
    };

    if input.operations.iter().all(FileOperation::is_read_only) {
        ToolExecutionCategory::ReadOnlyParallel
    } else {
        ToolExecutionCategory::ExclusiveLocalMutation
    }
}
