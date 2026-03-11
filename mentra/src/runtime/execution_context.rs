mod input;
mod intrinsic;
mod render;
mod store;
mod types;

use std::{fmt, io, path::Path};

use serde_json::Value;

use crate::runtime::task::{TaskAccess, TaskStore};

pub(crate) const CONTEXT_CREATE_TOOL_NAME: &str = "context_create";
pub(crate) const CONTEXT_GET_TOOL_NAME: &str = "context_get";
pub(crate) const CONTEXT_LIST_TOOL_NAME: &str = "context_list";
pub(crate) const CONTEXT_KEEP_TOOL_NAME: &str = "context_keep";
pub(crate) const CONTEXT_REMOVE_TOOL_NAME: &str = "context_remove";
pub(crate) const CONTEXT_REMINDER_TEXT: &str = "Reminder: if you own an unfinished task and need to run shell commands, create or reuse a bound execution context first with context_create or context_list.";

pub(crate) use intrinsic::{execute_intrinsic, intrinsic_specs};
pub(crate) use store::ExecutionContextStore;
pub use types::{ExecutionContextBackendKind, ExecutionContextItem, ExecutionContextStatus};

#[derive(Debug)]
pub(crate) enum ExecutionContextError {
    Io(io::Error),
    Serde(serde_json::Error),
    Validation(String),
}

impl fmt::Display for ExecutionContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Execution context I/O failed: {error}"),
            Self::Serde(error) => write!(f, "Execution context serialization failed: {error}"),
            Self::Validation(message) => f.write_str(message),
        }
    }
}

impl From<io::Error> for ExecutionContextError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ExecutionContextError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionContextDiskState {
    pub(crate) existed: bool,
    pub(crate) index: Option<String>,
    pub(crate) events: Option<String>,
    pub(crate) contexts: Vec<ExecutionContextItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutionContextCommandOutput {
    pub(crate) content: String,
    pub(crate) context: Option<ExecutionContextItem>,
    pub(crate) touched_task: bool,
}

pub(crate) fn is_execution_context_tool(name: &str) -> bool {
    matches!(
        name,
        CONTEXT_CREATE_TOOL_NAME
            | CONTEXT_GET_TOOL_NAME
            | CONTEXT_LIST_TOOL_NAME
            | CONTEXT_KEEP_TOOL_NAME
            | CONTEXT_REMOVE_TOOL_NAME
    )
}

pub(crate) fn execute(
    tool_name: &str,
    input: Value,
    base_dir: &Path,
    contexts_dir: &Path,
    tasks_dir: &Path,
    access: TaskAccess<'_>,
) -> Result<ExecutionContextCommandOutput, String> {
    let store = ExecutionContextStore::new(base_dir.to_path_buf(), contexts_dir.to_path_buf());
    let task_store = TaskStore::new(tasks_dir.to_path_buf());
    match tool_name {
        CONTEXT_CREATE_TOOL_NAME => input::parse_context_create_input(input)
            .and_then(|parsed| {
                store
                    .create(parsed, &task_store, access)
                    .map_err(|error| error.to_string())
            })
            .and_then(|context| {
                serde_json::to_string_pretty(&context)
                    .map_err(|error| error.to_string())
                    .map(|content| ExecutionContextCommandOutput {
                        content,
                        touched_task: context.task_id.is_some(),
                        context: Some(context),
                    })
            }),
        CONTEXT_GET_TOOL_NAME => input::parse_context_get_input(input)
            .and_then(|parsed| store.get(&parsed.name).map_err(|error| error.to_string()))
            .and_then(|context| {
                serde_json::to_string_pretty(&context)
                    .map_err(|error| error.to_string())
                    .map(|content| ExecutionContextCommandOutput {
                        content,
                        context: Some(context),
                        touched_task: false,
                    })
            }),
        CONTEXT_LIST_TOOL_NAME => input::parse_context_list_input(input).and_then(|()| {
            store
                .list()
                .map(|content| ExecutionContextCommandOutput {
                    content,
                    context: None,
                    touched_task: false,
                })
                .map_err(|error| error.to_string())
        }),
        CONTEXT_KEEP_TOOL_NAME => input::parse_context_keep_input(input)
            .and_then(|parsed| store.keep(&parsed.name).map_err(|error| error.to_string()))
            .and_then(|context| {
                serde_json::to_string_pretty(&context)
                    .map_err(|error| error.to_string())
                    .map(|content| ExecutionContextCommandOutput {
                        content,
                        context: Some(context),
                        touched_task: false,
                    })
            }),
        CONTEXT_REMOVE_TOOL_NAME => input::parse_context_remove_input(input)
            .and_then(|parsed| {
                store
                    .remove(
                        &parsed.name,
                        parsed.force,
                        parsed.complete_task,
                        &task_store,
                        access,
                    )
                    .map_err(|error| error.to_string())
                    .map(|context| (context, parsed.complete_task))
            })
            .and_then(|(context, complete_task)| {
                serde_json::to_string_pretty(&context)
                    .map_err(|error| error.to_string())
                    .map(|content| ExecutionContextCommandOutput {
                        content,
                        touched_task: complete_task || context.task_id.is_some(),
                        context: Some(context),
                    })
            }),
        _ => Err(format!(
            "Tool '{tool_name}' is not an execution context tool"
        )),
    }
}
