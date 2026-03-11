use serde_json::json;

use crate::{
    ContentBlock,
    runtime::{
        Agent, TASK_CREATE_TOOL_NAME, TASK_GET_TOOL_NAME, TASK_LIST_TOOL_NAME,
        TASK_UPDATE_TOOL_NAME, task,
    },
    tool::{ToolCall, ToolSpec},
};

pub(crate) fn intrinsic_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: TASK_CREATE_TOOL_NAME.to_string(),
            description: Some(
                "Lead-oriented project planning tool. Create a persisted task.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "subject": {
                        "type": "string",
                        "description": "Short title for the task"
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional extra detail for the task"
                    },
                    "owner": {
                        "type": "string",
                        "description": "Optional owner label for the task"
                    },
                    "blockedBy": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Task IDs that must finish before this task is ready"
                    }
                },
                "required": ["subject"]
            }),
        },
        ToolSpec {
            name: task::TASK_CLAIM_TOOL_NAME.to_string(),
            description: Some(
                "Claim a ready unowned persisted task for the current teammate.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "integer",
                        "description": "Optional explicit task identifier to claim"
                    }
                }
            }),
        },
        ToolSpec {
            name: TASK_UPDATE_TOOL_NAME.to_string(),
            description: Some(
                "Lead-oriented project planning tool. Update a persisted task and its dependency edges."
                    .into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "integer",
                        "description": "Stable identifier for the task"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Updated task subject"
                    },
                    "description": {
                        "type": "string",
                        "description": "Updated task description"
                    },
                    "owner": {
                        "type": "string",
                        "description": "Updated task owner"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed"],
                        "description": "Updated task status"
                    },
                    "addBlockedBy": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Add dependency edges from blocker tasks into this task"
                    },
                    "removeBlockedBy": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Remove dependency edges from blocker tasks into this task"
                    },
                    "addBlocks": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Add dependency edges from this task into dependent tasks"
                    },
                    "removeBlocks": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Remove dependency edges from this task into dependent tasks"
                    }
                },
                "required": ["taskId"]
            }),
        },
        ToolSpec {
            name: TASK_LIST_TOOL_NAME.to_string(),
            description: Some("List persisted tasks grouped by readiness.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolSpec {
            name: TASK_GET_TOOL_NAME.to_string(),
            description: Some("Get one persisted task by ID.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "integer",
                        "description": "Stable identifier for the task"
                    }
                },
                "required": ["taskId"]
            }),
        },
    ]
}

pub(crate) struct TaskIntrinsicResult {
    pub(crate) result: ContentBlock,
    pub(crate) touched_task: bool,
}

pub(crate) fn execute_intrinsic(agent: &mut Agent, call: ToolCall) -> Option<TaskIntrinsicResult> {
    let output = if matches!(
        call.name.as_str(),
        task::TASK_CREATE_TOOL_NAME | task::TASK_CLAIM_TOOL_NAME | task::TASK_UPDATE_TOOL_NAME
    ) {
        agent.execute_task_mutation(&call.name, call.input)
    } else if task::is_task_tool(&call.name) {
        task::execute(
            &call.name,
            call.input,
            agent.config().task.tasks_dir.as_path(),
            agent.task_access(),
        )
    } else {
        return None;
    };

    Some(match output {
        Ok(content) => match agent.refresh_tasks_from_disk() {
            Ok(()) => TaskIntrinsicResult {
                result: ContentBlock::ToolResult {
                    tool_use_id: call.id,
                    content,
                    is_error: false,
                },
                touched_task: true,
            },
            Err(error) => TaskIntrinsicResult {
                result: ContentBlock::ToolResult {
                    tool_use_id: call.id,
                    content: format!("Task refresh failed: {error:?}"),
                    is_error: true,
                },
                touched_task: false,
            },
        },
        Err(content) => TaskIntrinsicResult {
            result: ContentBlock::ToolResult {
                tool_use_id: call.id,
                content,
                is_error: true,
            },
            touched_task: false,
        },
    })
}
