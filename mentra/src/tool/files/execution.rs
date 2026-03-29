use serde_json::Value;

use crate::{runtime::RuntimeHandle, tool::ToolResult};

use super::{
    input::{ensure_files_have_operations, parse_files_input},
    workspace::WorkspaceEditor,
};

pub(crate) async fn execute_files_tool(
    agent_id: String,
    runtime: RuntimeHandle,
    default_working_directory: std::path::PathBuf,
    input: Value,
) -> ToolResult {
    let input = parse_files_input(&input)?;
    ensure_files_have_operations(&input)?;

    let working_directory = match input.working_directory.as_deref() {
        Some(directory) => runtime.resolve_working_directory(&agent_id, Some(directory))?,
        None => runtime
            .resolve_working_directory(&agent_id, None)
            .unwrap_or(default_working_directory),
    };
    let base_dir = runtime.agent_config(&agent_id)?.base_dir;

    tokio::task::spawn_blocking(move || {
        let mut editor = WorkspaceEditor::new(agent_id, runtime, base_dir, working_directory);
        let mut sections = Vec::with_capacity(input.operations.len());
        for operation in input.operations {
            sections.push(editor.apply_operation(operation)?);
        }
        editor.commit()?;

        Ok(sections.join("\n\n"))
    })
    .await
    .map_err(|error| format!("Files tool task failed: {error}"))?
}
