use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::runtime::task::{TaskAccess, TaskStatus, TaskStore};

use super::{
    ExecutionContextDiskState, ExecutionContextError,
    input::ContextCreateInput,
    render::{ContextListOutput, serialize_pretty},
    types::{
        ExecutionContextBackendKind, ExecutionContextItem, ExecutionContextStatus,
        GitWorktreeMetadata,
    },
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextIndex {
    contexts: Vec<ExecutionContextItem>,
}

pub(crate) struct ExecutionContextStore {
    base_dir: PathBuf,
    dir: PathBuf,
}

impl ExecutionContextStore {
    pub(crate) fn new(base_dir: impl Into<PathBuf>, dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            dir: dir.into(),
        }
    }

    pub(crate) fn load_all(&self) -> Result<Vec<ExecutionContextItem>, ExecutionContextError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path)?;
        let mut contexts = serde_json::from_str::<ContextIndex>(&content)?.contexts;
        contexts.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(contexts)
    }

    pub(crate) fn capture_disk_state(
        &self,
    ) -> Result<ExecutionContextDiskState, ExecutionContextError> {
        if !self.dir.exists() {
            return Ok(ExecutionContextDiskState {
                existed: false,
                index: None,
                events: None,
                contexts: Vec::new(),
            });
        }

        let index = read_optional_string(&self.index_path())?;
        let events = read_optional_string(&self.events_path())?;
        let contexts = self.load_all()?;

        Ok(ExecutionContextDiskState {
            existed: true,
            index,
            events,
            contexts,
        })
    }

    pub(crate) fn restore_disk_state(
        &self,
        state: &ExecutionContextDiskState,
    ) -> Result<(), ExecutionContextError> {
        let current_contexts = self.load_all().unwrap_or_default();
        let target_by_name = state
            .contexts
            .iter()
            .map(|context| (context.name.clone(), context.clone()))
            .collect::<BTreeMap<_, _>>();

        for context in &current_contexts {
            let target = target_by_name.get(&context.name);
            if target.is_none()
                || matches!(
                    target.map(|value| &value.status),
                    Some(ExecutionContextStatus::Removed)
                )
            {
                self.ensure_backend_removed(context, true)?;
            }
        }

        for context in &state.contexts {
            match context.status {
                ExecutionContextStatus::Active | ExecutionContextStatus::Kept => {
                    self.ensure_backend_present(context)?
                }
                ExecutionContextStatus::Removed => self.ensure_backend_removed(context, true)?,
            }
        }

        if !state.existed {
            remove_if_exists(&self.index_path())?;
            remove_if_exists(&self.events_path())?;
            if self.dir.exists() {
                match fs::remove_dir(&self.dir) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(ExecutionContextError::Io(error)),
                }
            }
            return Ok(());
        }

        fs::create_dir_all(&self.dir)?;
        match &state.index {
            Some(index) => self.write_raw_file(&self.index_path(), index)?,
            None => self.write_index(&state.contexts)?,
        }
        if let Some(events) = &state.events {
            self.write_raw_file(&self.events_path(), events)?;
        } else {
            remove_if_exists(&self.events_path())?;
        }
        Ok(())
    }

    pub(crate) fn get(&self, name: &str) -> Result<ExecutionContextItem, ExecutionContextError> {
        self.load_all()?
            .into_iter()
            .find(|context| context.name == name)
            .ok_or_else(|| {
                ExecutionContextError::Validation(format!(
                    "Execution context '{name}' does not exist"
                ))
            })
    }

    pub(crate) fn list(&self) -> Result<String, ExecutionContextError> {
        let contexts = self.load_all()?;
        let mut active = Vec::new();
        let mut kept = Vec::new();
        let mut removed = Vec::new();

        for context in &contexts {
            match context.status {
                ExecutionContextStatus::Active => active.push(context.clone()),
                ExecutionContextStatus::Kept => kept.push(context.clone()),
                ExecutionContextStatus::Removed => removed.push(context.clone()),
            }
        }

        serialize_pretty(&ContextListOutput {
            contexts,
            active,
            kept,
            removed,
        })
    }

    pub(crate) fn create(
        &self,
        input: ContextCreateInput,
        task_store: &TaskStore,
        access: TaskAccess<'_>,
    ) -> Result<ExecutionContextItem, ExecutionContextError> {
        let mut contexts = self.load_all()?;
        if contexts.iter().any(|context| context.name == input.name) {
            return Err(ExecutionContextError::Validation(format!(
                "Execution context '{}' already exists",
                input.name
            )));
        }

        let backend = input
            .backend
            .unwrap_or(ExecutionContextBackendKind::GitWorktree);
        self.validate_backend(&backend)?;

        if let Some(task_id) = input.task_id {
            let task = task_store.get_task(task_id).map_err(map_task_error)?;
            validate_task_binding(&task, access)?;
        }

        self.append_event(
            "context.create.before",
            None,
            Some(json!({
                "name": input.name,
                "taskId": input.task_id,
                "backend": backend,
            })),
        )?;

        let item = match backend {
            ExecutionContextBackendKind::GitWorktree => {
                let branch = format!("wt/{}", input.name);
                let path = self.dir.join(&input.name);
                let from_ref = input.from_ref.unwrap_or_else(|| "HEAD".to_string());
                self.run_git([
                    "worktree",
                    "add",
                    "-b",
                    branch.as_str(),
                    path.to_string_lossy().as_ref(),
                    from_ref.as_str(),
                ])?;

                ExecutionContextItem {
                    name: input.name.clone(),
                    backend,
                    path,
                    task_id: input.task_id,
                    status: ExecutionContextStatus::Active,
                    git_worktree: Some(GitWorktreeMetadata { branch }),
                }
            }
        };

        if let Some(task_id) = item.task_id
            && let Err(error) = task_store
                .bind_execution_context(task_id, &item.name, access)
                .map_err(map_task_error)
        {
            let _ = self.ensure_backend_removed(&item, true);
            let _ = self.append_event(
                "context.create.failed",
                None,
                Some(json!({
                    "name": item.name,
                    "error": error.to_string(),
                })),
            );
            return Err(error);
        }

        contexts.push(item.clone());
        if let Err(error) = self.write_index(&contexts) {
            let _ = self.ensure_backend_removed(&item, true);
            if let Some(task_id) = item.task_id {
                let _ =
                    task_store.clear_execution_context(task_id, Some(&item.name), false, access);
            }
            let _ = self.append_event(
                "context.create.failed",
                Some(&item),
                Some(json!({ "error": error.to_string() })),
            );
            return Err(error);
        }

        let task = item
            .task_id
            .and_then(|task_id| task_store.get_task(task_id).ok());
        self.append_event(
            "context.create.after",
            Some(&item),
            task.as_ref().map(task_json),
        )?;
        Ok(item)
    }

    pub(crate) fn keep(&self, name: &str) -> Result<ExecutionContextItem, ExecutionContextError> {
        let mut contexts = self.load_all()?;
        let context = contexts
            .iter_mut()
            .find(|context| context.name == name)
            .ok_or_else(|| {
                ExecutionContextError::Validation(format!(
                    "Execution context '{name}' does not exist"
                ))
            })?;

        if context.status == ExecutionContextStatus::Removed {
            return Err(ExecutionContextError::Validation(format!(
                "Execution context '{name}' is already removed"
            )));
        }

        context.status = ExecutionContextStatus::Kept;
        let kept = context.clone();
        self.write_index(&contexts)?;
        self.append_event("context.keep", Some(&kept), None)?;
        Ok(kept)
    }

    pub(crate) fn remove(
        &self,
        name: &str,
        force: bool,
        complete_task: bool,
        task_store: &TaskStore,
        access: TaskAccess<'_>,
    ) -> Result<ExecutionContextItem, ExecutionContextError> {
        let mut contexts = self.load_all()?;
        let index = contexts
            .iter()
            .position(|context| context.name == name)
            .ok_or_else(|| {
                ExecutionContextError::Validation(format!(
                    "Execution context '{name}' does not exist"
                ))
            })?;

        if contexts[index].status == ExecutionContextStatus::Removed {
            return Err(ExecutionContextError::Validation(format!(
                "Execution context '{name}' is already removed"
            )));
        }

        let current = contexts[index].clone();
        self.append_event("context.remove.before", Some(&current), None)?;
        if let Err(error) = self.ensure_backend_removed(&current, force) {
            let _ = self.append_event(
                "context.remove.failed",
                Some(&current),
                Some(json!({ "error": error.to_string() })),
            );
            return Err(error);
        }

        if let Some(task_id) = current.task_id {
            task_store
                .clear_execution_context(task_id, Some(&current.name), complete_task, access)
                .map_err(map_task_error)?;
        }

        contexts[index].status = ExecutionContextStatus::Removed;
        let removed = contexts[index].clone();
        self.write_index(&contexts)?;
        let task = removed
            .task_id
            .and_then(|task_id| task_store.get_task(task_id).ok());
        self.append_event(
            "context.remove.after",
            Some(&removed),
            task.as_ref().map(task_json),
        )?;
        Ok(removed)
    }

    pub(crate) fn resolve_path(
        &self,
        name: &str,
    ) -> Result<ExecutionContextItem, ExecutionContextError> {
        let context = self.get(name)?;
        match context.status {
            ExecutionContextStatus::Active | ExecutionContextStatus::Kept => Ok(context),
            ExecutionContextStatus::Removed => Err(ExecutionContextError::Validation(format!(
                "Execution context '{name}' has been removed"
            ))),
        }
    }

    fn validate_backend(
        &self,
        backend: &ExecutionContextBackendKind,
    ) -> Result<(), ExecutionContextError> {
        match backend {
            ExecutionContextBackendKind::GitWorktree => {
                self.run_git(["rev-parse", "--show-toplevel"])?;
                Ok(())
            }
        }
    }

    fn ensure_backend_present(
        &self,
        context: &ExecutionContextItem,
    ) -> Result<(), ExecutionContextError> {
        if context.path.exists() {
            return Ok(());
        }

        match context.backend {
            ExecutionContextBackendKind::GitWorktree => {
                let branch = context
                    .git_worktree
                    .as_ref()
                    .map(|git| git.branch.as_str())
                    .ok_or_else(|| {
                        ExecutionContextError::Validation(format!(
                            "Execution context '{}' is missing git worktree metadata",
                            context.name
                        ))
                    })?;
                self.validate_backend(&context.backend)?;
                self.run_git([
                    "worktree",
                    "add",
                    context.path.to_string_lossy().as_ref(),
                    branch,
                ])?;
                Ok(())
            }
        }
    }

    fn ensure_backend_removed(
        &self,
        context: &ExecutionContextItem,
        force: bool,
    ) -> Result<(), ExecutionContextError> {
        if !context.path.exists() {
            return Ok(());
        }

        match context.backend {
            ExecutionContextBackendKind::GitWorktree => {
                self.validate_backend(&context.backend)?;
                let mut args = vec!["worktree", "remove"];
                if force {
                    args.push("--force");
                }
                let path = context.path.to_string_lossy().to_string();
                args.push(path.as_str());
                self.run_git(args)?;
                Ok(())
            }
        }
    }

    fn write_index(&self, contexts: &[ExecutionContextItem]) -> Result<(), ExecutionContextError> {
        fs::create_dir_all(&self.dir)?;
        let mut sorted = contexts.to_vec();
        sorted.sort_by(|left, right| left.name.cmp(&right.name));
        let content = serde_json::to_string_pretty(&ContextIndex { contexts: sorted })?;
        self.write_raw_file(&self.index_path(), &content)
    }

    fn append_event(
        &self,
        event: &str,
        context: Option<&ExecutionContextItem>,
        task: Option<serde_json::Value>,
    ) -> Result<(), ExecutionContextError> {
        fs::create_dir_all(&self.dir)?;
        let mut entry = json!({
            "event": event,
            "ts": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_secs(),
        });
        if let Some(context) = context {
            entry["context"] = serde_json::to_value(context)?;
        }
        if let Some(task) = task {
            entry["task"] = task;
        }
        let line = format!("{}\n", serde_json::to_string(&entry)?);
        let mut existing = read_optional_string(&self.events_path())?.unwrap_or_default();
        existing.push_str(&line);
        self.write_raw_file(&self.events_path(), &existing)
    }

    fn write_raw_file(&self, path: &Path, content: &str) -> Result<(), ExecutionContextError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let temp_path = path.with_extension(format!("tmp-{unique}"));
        fs::write(&temp_path, content)?;
        fs::rename(&temp_path, path)?;
        Ok(())
    }

    fn run_git<I, S>(&self, args: I) -> Result<String, ExecutionContextError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let args_vec = args
            .into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect::<Vec<_>>();
        let output = Command::new("git")
            .args(args_vec.iter().map(String::as_str))
            .current_dir(&self.base_dir)
            .output()
            .map_err(ExecutionContextError::Io)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("git {:?} exited with status {}", args_vec, output.status)
            } else {
                stderr
            };
            Err(ExecutionContextError::Validation(message))
        }
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }
}

fn validate_task_binding(
    task: &crate::runtime::TaskItem,
    access: TaskAccess<'_>,
) -> Result<(), ExecutionContextError> {
    match access {
        TaskAccess::Lead => {}
        TaskAccess::Teammate(name) if task.owner == name => {}
        TaskAccess::Teammate(name) => {
            return Err(ExecutionContextError::Validation(format!(
                "Teammate '{name}' cannot bind execution context for task {} owned by '{}'",
                task.id, task.owner
            )));
        }
    }

    if let Some(existing) = task.execution_context_id.as_deref() {
        return Err(ExecutionContextError::Validation(format!(
            "Task {} is already bound to execution context '{existing}'",
            task.id
        )));
    }
    if task.status == TaskStatus::Completed {
        return Err(ExecutionContextError::Validation(format!(
            "Task {} is already completed and cannot bind an execution context",
            task.id
        )));
    }
    if !task.blocked_by.is_empty() {
        return Err(ExecutionContextError::Validation(format!(
            "Task {} is blocked by {:?} and cannot bind an execution context",
            task.id, task.blocked_by
        )));
    }
    Ok(())
}

fn map_task_error(error: crate::runtime::TaskError) -> ExecutionContextError {
    match error {
        crate::runtime::TaskError::Io(error) => ExecutionContextError::Io(error),
        crate::runtime::TaskError::Serde(error) => ExecutionContextError::Serde(error),
        crate::runtime::TaskError::Validation(message) => {
            ExecutionContextError::Validation(message)
        }
    }
}

fn task_json(task: &crate::runtime::TaskItem) -> serde_json::Value {
    json!({
        "id": task.id,
        "status": task.status,
        "executionContextId": task.execution_context_id,
    })
}

fn read_optional_string(path: &Path) -> Result<Option<String>, ExecutionContextError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExecutionContextError::Io(error)),
    }
}

fn remove_if_exists(path: &Path) -> Result<(), ExecutionContextError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ExecutionContextError::Io(error)),
    }
}
