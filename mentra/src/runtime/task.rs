mod graph;
mod input;
mod intrinsic;
mod render;
mod store;
#[cfg(test)]
mod tests;
mod types;

use std::{fmt, io, path::Path};

use serde_json::Value;

pub(crate) const TASK_CREATE_TOOL_NAME: &str = "task_create";
pub(crate) const TASK_CLAIM_TOOL_NAME: &str = "task_claim";
pub(crate) const TASK_UPDATE_TOOL_NAME: &str = "task_update";
pub(crate) const TASK_LIST_TOOL_NAME: &str = "task_list";
pub(crate) const TASK_GET_TOOL_NAME: &str = "task_get";
pub(crate) const TASK_REMINDER_TEXT: &str = "Reminder: use task_create, task_claim, task_update, task_list, or task_get only for persisted project-task tracking. Do not use task tools to manage persistent teammates or team protocol flows.";

pub(crate) use graph::has_unfinished_tasks;
pub(crate) use intrinsic::{execute_intrinsic, intrinsic_specs};
pub(crate) use store::{TaskDiskState, TaskStore};
pub use types::{TaskItem, TaskStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskAccess<'a> {
    Lead,
    Teammate(&'a str),
}

#[derive(Debug)]
pub(crate) enum TaskError {
    Io(io::Error),
    Serde(serde_json::Error),
    Validation(String),
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Task storage I/O failed: {error}"),
            Self::Serde(error) => write!(f, "Task serialization failed: {error}"),
            Self::Validation(message) => f.write_str(message),
        }
    }
}

impl From<io::Error> for TaskError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for TaskError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub(crate) fn is_task_tool(name: &str) -> bool {
    matches!(
        name,
        TASK_CREATE_TOOL_NAME
            | TASK_CLAIM_TOOL_NAME
            | TASK_UPDATE_TOOL_NAME
            | TASK_LIST_TOOL_NAME
            | TASK_GET_TOOL_NAME
    )
}

pub(crate) fn execute(
    tool_name: &str,
    input: Value,
    dir: &Path,
    access: TaskAccess<'_>,
) -> Result<String, String> {
    let store = TaskStore::new(dir.to_path_buf());
    match tool_name {
        TASK_CREATE_TOOL_NAME => input::parse_task_create_input(input)
            .and_then(|parsed| store.create(parsed).map_err(|error| error.to_string())),
        TASK_CLAIM_TOOL_NAME => input::parse_task_claim_input(input).and_then(|parsed| {
            store
                .claim(parsed.task_id, access.actor_name())
                .map_err(|error| error.to_string())
        }),
        TASK_UPDATE_TOOL_NAME => input::parse_task_update_input(input).and_then(|parsed| {
            store
                .update(parsed, access)
                .map_err(|error| error.to_string())
        }),
        TASK_GET_TOOL_NAME => input::parse_task_get_input(input)
            .and_then(|parsed| store.get(parsed.task_id).map_err(|error| error.to_string())),
        TASK_LIST_TOOL_NAME => input::parse_task_list_input(input)
            .and_then(|()| store.list().map_err(|error| error.to_string())),
        _ => Err(format!("Tool '{tool_name}' is not a task tool")),
    }
}

impl<'a> TaskAccess<'a> {
    pub(crate) fn actor_name(self) -> Option<&'a str> {
        match self {
            Self::Lead => None,
            Self::Teammate(name) => Some(name),
        }
    }
}
