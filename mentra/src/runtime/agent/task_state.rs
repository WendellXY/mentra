use std::borrow::Cow;

use crate::runtime::execution_context::ExecutionContextStore;
use crate::runtime::{
    ExecutionContextDiskState, TaskDiskState,
    error::RuntimeError,
    execution_context::CONTEXT_REMINDER_TEXT,
    task::{TASK_CLAIM_TOOL_NAME, TASK_REMINDER_TEXT, TaskAccess, TaskStore, has_unfinished_tasks},
};

use super::Agent;

impl Agent {
    pub(crate) fn effective_system_prompt(&self) -> Option<Cow<'_, str>> {
        let mut sections = Vec::new();

        if self.rounds_since_task >= self.config.task.reminder_threshold
            && has_unfinished_tasks(&self.tasks)
        {
            sections.push(TASK_REMINDER_TEXT.to_string());
        }
        if self.should_remind_missing_execution_context() {
            sections.push(CONTEXT_REMINDER_TEXT.to_string());
        }

        if let Some(system) = &self.config.system {
            sections.push(system.clone());
        }

        if let Some(skills) = self.runtime.skill_descriptions() {
            sections.push(skills);
        }

        if sections.is_empty() {
            None
        } else {
            Some(Cow::Owned(sections.join("\n\n")))
        }
    }

    pub(crate) fn note_round_without_task(&mut self) {
        if has_unfinished_tasks(&self.tasks) {
            self.rounds_since_task += 1;
        }
    }

    pub(crate) fn record_task_activity(&mut self) {
        self.rounds_since_task = 0;
    }

    pub(crate) fn refresh_tasks_from_disk(&mut self) -> Result<(), RuntimeError> {
        let tasks = TaskStore::new(self.config.task.tasks_dir.clone())
            .load_all()
            .map_err(map_task_error_for_load)?;
        self.tasks = tasks;
        let tasks = self.tasks.clone();
        self.mutate_snapshot(|snapshot| {
            snapshot.tasks = tasks;
        });
        Ok(())
    }

    pub(crate) fn refresh_execution_contexts_from_disk(&mut self) -> Result<(), RuntimeError> {
        let contexts = ExecutionContextStore::new(
            self.config.execution_context.base_dir.clone(),
            self.config.execution_context.contexts_dir.clone(),
        )
        .load_all()
        .map_err(map_execution_context_error_for_load)?;
        self.mutate_snapshot(|snapshot| {
            snapshot.execution_contexts = contexts;
        });
        Ok(())
    }

    pub(crate) fn task_access(&self) -> TaskAccess<'_> {
        match &self.teammate_identity {
            Some(_) => TaskAccess::Teammate(self.name.as_str()),
            None => TaskAccess::Lead,
        }
    }

    pub(crate) fn try_claim_ready_task(
        &mut self,
    ) -> Result<Option<crate::runtime::TaskItem>, RuntimeError> {
        self.refresh_tasks_from_disk()?;
        if self.owns_unfinished_tasks() {
            return Ok(None);
        }

        match self.execute_task_mutation(TASK_CLAIM_TOOL_NAME, serde_json::json!({})) {
            Ok(content) => {
                self.refresh_tasks_from_disk()?;
                serde_json::from_str::<crate::runtime::TaskItem>(&content)
                    .map(Some)
                    .map_err(RuntimeError::FailedToSerializeTasks)
            }
            Err(error) if error == "No ready unowned tasks are available to claim" => Ok(None),
            Err(error) => Err(RuntimeError::InvalidTask(error)),
        }
    }

    pub(crate) fn execute_task_mutation(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<String, String> {
        self.runtime.execute_task_mutation(
            tool_name,
            input,
            self.config.task.tasks_dir.as_path(),
            self.task_access(),
        )
    }

    pub(crate) fn execute_execution_context_mutation(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<crate::runtime::execution_context::ExecutionContextCommandOutput, String> {
        self.runtime.execute_execution_context_mutation(
            tool_name,
            input,
            self.config.execution_context.base_dir.as_path(),
            self.config.execution_context.contexts_dir.as_path(),
            self.config.task.tasks_dir.as_path(),
            self.task_access(),
        )
    }

    pub(super) fn capture_task_disk_state(&self) -> Result<TaskDiskState, RuntimeError> {
        TaskStore::new(self.config.task.tasks_dir.clone())
            .capture_disk_state()
            .map_err(map_task_error_for_load)
    }

    pub(super) fn capture_execution_context_disk_state(
        &self,
    ) -> Result<ExecutionContextDiskState, RuntimeError> {
        ExecutionContextStore::new(
            self.config.execution_context.base_dir.clone(),
            self.config.execution_context.contexts_dir.clone(),
        )
        .capture_disk_state()
        .map_err(map_execution_context_error_for_load)
    }

    fn owns_unfinished_tasks(&self) -> bool {
        self.tasks.iter().any(|task| {
            task.owner == self.name && !matches!(task.status, crate::runtime::TaskStatus::Completed)
        })
    }

    pub(super) fn restore_task_state(
        &mut self,
        tasks: Vec<crate::runtime::TaskItem>,
        rounds_since_task: usize,
        disk_state: &TaskDiskState,
    ) -> Result<(), RuntimeError> {
        TaskStore::new(self.config.task.tasks_dir.clone())
            .restore_disk_state(disk_state)
            .map_err(map_task_error_for_restore)?;
        self.tasks = tasks;
        self.rounds_since_task = rounds_since_task;
        let tasks = self.tasks.clone();
        self.mutate_snapshot(|snapshot| {
            snapshot.tasks = tasks;
        });
        Ok(())
    }

    pub(super) fn restore_execution_context_state(
        &mut self,
        disk_state: &ExecutionContextDiskState,
    ) -> Result<(), RuntimeError> {
        ExecutionContextStore::new(
            self.config.execution_context.base_dir.clone(),
            self.config.execution_context.contexts_dir.clone(),
        )
        .restore_disk_state(disk_state)
        .map_err(map_execution_context_error_for_restore)?;
        self.refresh_execution_contexts_from_disk()
    }

    fn should_remind_missing_execution_context(&self) -> bool {
        self.config.execution_context.auto_route_shell
            && self.teammate_identity.is_some()
            && self.tasks.iter().any(|task| {
                task.owner == self.name
                    && !matches!(task.status, crate::runtime::TaskStatus::Completed)
                    && task.execution_context_id.is_none()
            })
    }
}

fn map_task_error_for_load(error: crate::runtime::TaskError) -> RuntimeError {
    match error {
        crate::runtime::TaskError::Io(error) => RuntimeError::FailedToLoadTasks(error),
        crate::runtime::TaskError::Serde(error) => RuntimeError::FailedToSerializeTasks(error),
        crate::runtime::TaskError::Validation(message) => RuntimeError::InvalidTask(message),
    }
}

fn map_task_error_for_restore(error: crate::runtime::TaskError) -> RuntimeError {
    match error {
        crate::runtime::TaskError::Io(error) => RuntimeError::FailedToRestoreTasks(error),
        crate::runtime::TaskError::Serde(error) => RuntimeError::FailedToSerializeTasks(error),
        crate::runtime::TaskError::Validation(message) => RuntimeError::InvalidTask(message),
    }
}

fn map_execution_context_error_for_load(
    error: crate::runtime::execution_context::ExecutionContextError,
) -> RuntimeError {
    match error {
        crate::runtime::execution_context::ExecutionContextError::Io(error) => {
            RuntimeError::FailedToLoadExecutionContexts(error)
        }
        crate::runtime::execution_context::ExecutionContextError::Serde(error) => {
            RuntimeError::FailedToSerializeExecutionContexts(error)
        }
        crate::runtime::execution_context::ExecutionContextError::Validation(message) => {
            RuntimeError::InvalidExecutionContext(message)
        }
    }
}

fn map_execution_context_error_for_restore(
    error: crate::runtime::execution_context::ExecutionContextError,
) -> RuntimeError {
    match error {
        crate::runtime::execution_context::ExecutionContextError::Io(error) => {
            RuntimeError::FailedToRestoreExecutionContexts(error)
        }
        crate::runtime::execution_context::ExecutionContextError::Serde(error) => {
            RuntimeError::FailedToSerializeExecutionContexts(error)
        }
        crate::runtime::execution_context::ExecutionContextError::Validation(message) => {
            RuntimeError::InvalidExecutionContext(message)
        }
    }
}
