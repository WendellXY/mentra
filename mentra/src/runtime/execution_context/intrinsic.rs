use serde_json::json;

use crate::{
    ContentBlock,
    runtime::{
        Agent, AgentEvent,
        execution_context::{self, ExecutionContextCommandOutput},
    },
    tool::{ToolCall, ToolSpec},
};

pub(crate) fn intrinsic_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: execution_context::CONTEXT_CREATE_TOOL_NAME.to_string(),
            description: Some(
                "Create a persisted isolated execution context, optionally bound to a task.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Stable execution context name"
                    },
                    "taskId": {
                        "type": "integer",
                        "description": "Optional task identifier to bind"
                    },
                    "backend": {
                        "type": "string",
                        "enum": ["git_worktree"],
                        "description": "Optional backend override"
                    },
                    "fromRef": {
                        "type": "string",
                        "description": "Optional git ref to branch from"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolSpec {
            name: execution_context::CONTEXT_GET_TOOL_NAME.to_string(),
            description: Some("Get one persisted execution context by name.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Execution context name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolSpec {
            name: execution_context::CONTEXT_LIST_TOOL_NAME.to_string(),
            description: Some("List persisted execution contexts grouped by status.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolSpec {
            name: execution_context::CONTEXT_KEEP_TOOL_NAME.to_string(),
            description: Some(
                "Mark an execution context as kept without removing its directory.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Execution context name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolSpec {
            name: execution_context::CONTEXT_REMOVE_TOOL_NAME.to_string(),
            description: Some(
                "Remove an execution context directory and optionally complete the bound task."
                    .into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Execution context name"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Force git worktree removal"
                    },
                    "completeTask": {
                        "type": "boolean",
                        "description": "Whether to complete the bound task while removing"
                    }
                },
                "required": ["name"]
            }),
        },
    ]
}

pub(crate) struct ExecutionContextIntrinsicResult {
    pub(crate) result: ContentBlock,
    pub(crate) touched_task: bool,
}

pub(crate) fn execute_intrinsic(
    agent: &mut Agent,
    call: ToolCall,
) -> Option<ExecutionContextIntrinsicResult> {
    let output = if matches!(
        call.name.as_str(),
        execution_context::CONTEXT_CREATE_TOOL_NAME
            | execution_context::CONTEXT_KEEP_TOOL_NAME
            | execution_context::CONTEXT_REMOVE_TOOL_NAME
    ) {
        agent.execute_execution_context_mutation(&call.name, call.input)
    } else if execution_context::is_execution_context_tool(&call.name) {
        execution_context::execute(
            &call.name,
            call.input,
            agent.config().execution_context.base_dir.as_path(),
            agent.config().execution_context.contexts_dir.as_path(),
            agent.config().task.tasks_dir.as_path(),
            agent.task_access(),
        )
    } else {
        return None;
    };

    match output {
        Ok(ExecutionContextCommandOutput {
            content,
            context,
            touched_task,
        }) => {
            if let Err(error) = agent.refresh_execution_contexts_from_disk() {
                return Some(ExecutionContextIntrinsicResult {
                    result: ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content: format!("Execution context refresh failed: {error:?}"),
                        is_error: true,
                    },
                    touched_task: false,
                });
            }
            if touched_task && agent.refresh_tasks_from_disk().is_err() {
                return Some(ExecutionContextIntrinsicResult {
                    result: ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content: "Execution context updated, but task refresh failed".to_string(),
                        is_error: true,
                    },
                    touched_task: false,
                });
            }

            if let Some(context) = context {
                let event = match call.name.as_str() {
                    execution_context::CONTEXT_CREATE_TOOL_NAME => {
                        AgentEvent::ExecutionContextCreated { context }
                    }
                    execution_context::CONTEXT_REMOVE_TOOL_NAME => {
                        AgentEvent::ExecutionContextRemoved { context }
                    }
                    _ => AgentEvent::ExecutionContextUpdated { context },
                };
                agent.emit_event(event);
            }

            Some(ExecutionContextIntrinsicResult {
                result: ContentBlock::ToolResult {
                    tool_use_id: call.id,
                    content,
                    is_error: false,
                },
                touched_task,
            })
        }
        Err(content) => Some(ExecutionContextIntrinsicResult {
            result: ContentBlock::ToolResult {
                tool_use_id: call.id,
                content,
                is_error: true,
            },
            touched_task: false,
        }),
    }
}
