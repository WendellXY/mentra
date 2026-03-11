use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    TaskAccess, TaskError,
    graph::{
        add_dependency, apply_status_change, find_task, remove_dependency, sort_and_dedup_ids,
        sort_tasks, validate_loaded_tasks,
    },
    input::{TaskCreateInput, TaskUpdateInput},
    render::{TaskListOutput, TaskUpdateOutput, serialize_pretty},
    types::{TaskItem, TaskStatus},
};

#[derive(Debug, Clone)]
pub(crate) struct TaskDiskState {
    existed: bool,
    files: Vec<(String, String)>,
}

pub(crate) struct TaskStore {
    dir: PathBuf,
}

impl TaskStore {
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub(crate) fn load_all(&self) -> Result<Vec<TaskItem>, TaskError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut tasks = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_task_file(&path) {
                continue;
            }

            let content = fs::read_to_string(&path)?;
            let mut task = serde_json::from_str::<TaskItem>(&content)?;
            sort_and_dedup_ids(&mut task.blocked_by);
            sort_and_dedup_ids(&mut task.blocks);
            tasks.push(task);
        }

        tasks.sort_by_key(|task| task.id);
        validate_loaded_tasks(&tasks)?;
        Ok(tasks)
    }

    pub(crate) fn capture_disk_state(&self) -> Result<TaskDiskState, TaskError> {
        if !self.dir.exists() {
            return Ok(TaskDiskState {
                existed: false,
                files: Vec::new(),
            });
        }

        let mut files = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_task_file(&path) {
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            files.push((name.to_string(), fs::read_to_string(&path)?));
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));

        Ok(TaskDiskState {
            existed: true,
            files,
        })
    }

    pub(crate) fn restore_disk_state(&self, state: &TaskDiskState) -> Result<(), TaskError> {
        if self.dir.exists() {
            for path in self.task_file_paths()? {
                fs::remove_file(path)?;
            }
        }

        if !state.existed && state.files.is_empty() {
            if self.dir.exists() {
                match fs::remove_dir(&self.dir) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                    Err(error) => return Err(TaskError::Io(error)),
                }
            }
            return Ok(());
        }

        fs::create_dir_all(&self.dir)?;
        for (name, content) in &state.files {
            self.write_raw_file(&self.dir.join(name), content)?;
        }
        Ok(())
    }

    pub(crate) fn create(&self, input: TaskCreateInput) -> Result<String, TaskError> {
        let mut tasks = self.load_all()?;
        let task_id = tasks.iter().map(|task| task.id).max().unwrap_or(0) + 1;
        tasks.push(TaskItem {
            id: task_id,
            subject: input.subject.trim().to_string(),
            description: input.description,
            status: TaskStatus::Pending,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            owner: input.owner,
            execution_context_id: None,
        });

        for blocker_id in input.blocked_by {
            add_dependency(&mut tasks, blocker_id, task_id)?;
        }

        self.write_all(&tasks)?;
        let created = find_task(&tasks, task_id)?.clone();
        serialize_pretty(&created)
    }

    pub(crate) fn claim(
        &self,
        task_id: Option<u64>,
        owner: Option<&str>,
    ) -> Result<String, TaskError> {
        let owner = owner
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                TaskError::Validation("Only named teammates can claim tasks".to_string())
            })?
            .trim()
            .to_string();
        let mut tasks = self.load_all()?;
        let claimed = match task_id {
            Some(task_id) => {
                let task = find_task_mut(&mut tasks, task_id)?;
                validate_claimable(task, &owner)?;
                task.owner = owner;
                task.clone()
            }
            None => {
                let task = tasks
                    .iter_mut()
                    .find(|task| is_claimable(task))
                    .ok_or_else(|| {
                        TaskError::Validation(
                            "No ready unowned tasks are available to claim".to_string(),
                        )
                    })?;
                task.owner = owner;
                task.clone()
            }
        };

        self.write_all(&tasks)?;
        serialize_pretty(&claimed)
    }

    pub(crate) fn update(
        &self,
        input: TaskUpdateInput,
        access: TaskAccess<'_>,
    ) -> Result<String, TaskError> {
        let mut tasks = self.load_all()?;
        let task_id = input.task_id;
        let original_status = find_task(&tasks, task_id)?.status.clone();
        validate_update_access(find_task(&tasks, task_id)?, &input, access)?;

        {
            let task = find_task_mut(&mut tasks, task_id)?;
            if let Some(subject) = input.subject {
                task.subject = subject.trim().to_string();
            }
            if let Some(description) = input.description {
                task.description = description;
            }
            if let Some(owner) = input.owner {
                task.owner = owner;
            }
        }

        for blocker_id in input.add_blocked_by {
            add_dependency(&mut tasks, blocker_id, task_id)?;
        }
        for blocker_id in input.remove_blocked_by {
            remove_dependency(&mut tasks, blocker_id, task_id)?;
        }
        for dependent_id in input.add_blocks {
            add_dependency(&mut tasks, task_id, dependent_id)?;
        }
        for dependent_id in input.remove_blocks {
            remove_dependency(&mut tasks, task_id, dependent_id)?;
        }

        let mut unblocked = Vec::new();
        let mut reblocked = Vec::new();
        if let Some(status) = input.status {
            apply_status_change(
                &mut tasks,
                task_id,
                original_status,
                status,
                &mut unblocked,
                &mut reblocked,
            )?;
        } else {
            validate_unblocked_status(find_task(&tasks, task_id)?)?;
        }

        self.write_all(&tasks)?;
        sort_tasks(&mut unblocked);
        sort_tasks(&mut reblocked);

        serialize_pretty(&TaskUpdateOutput {
            task: find_task(&tasks, task_id)?.clone(),
            unblocked,
            reblocked,
        })
    }

    pub(crate) fn get(&self, task_id: u64) -> Result<String, TaskError> {
        let tasks = self.load_all()?;
        serialize_pretty(find_task(&tasks, task_id)?)
    }

    pub(crate) fn get_task(&self, task_id: u64) -> Result<TaskItem, TaskError> {
        let tasks = self.load_all()?;
        Ok(find_task(&tasks, task_id)?.clone())
    }

    pub(crate) fn bind_execution_context(
        &self,
        task_id: u64,
        context_id: &str,
        access: TaskAccess<'_>,
    ) -> Result<TaskItem, TaskError> {
        let mut tasks = self.load_all()?;
        let task = find_task(&tasks, task_id)?.clone();
        validate_context_access(&task, access)?;
        if let Some(existing) = task.execution_context_id.as_deref()
            && existing != context_id
        {
            return Err(TaskError::Validation(format!(
                "Task {task_id} is already bound to execution context '{existing}'"
            )));
        }
        if !task.blocked_by.is_empty() {
            return Err(TaskError::Validation(format!(
                "Task {task_id} is blocked by {:?} and cannot bind an execution context",
                task.blocked_by
            )));
        }

        let task = find_task_mut(&mut tasks, task_id)?;
        task.execution_context_id = Some(context_id.to_string());
        if task.status == TaskStatus::Pending {
            task.status = TaskStatus::InProgress;
        }
        let bound = task.clone();
        self.write_all(&tasks)?;
        Ok(bound)
    }

    pub(crate) fn clear_execution_context(
        &self,
        task_id: u64,
        expected_context_id: Option<&str>,
        complete_task: bool,
        access: TaskAccess<'_>,
    ) -> Result<TaskItem, TaskError> {
        let mut tasks = self.load_all()?;
        let task = find_task(&tasks, task_id)?.clone();
        validate_context_access(&task, access)?;
        if let Some(expected) = expected_context_id
            && task.execution_context_id.as_deref() != Some(expected)
        {
            return Err(TaskError::Validation(format!(
                "Task {task_id} is not bound to execution context '{expected}'"
            )));
        }

        let task = find_task_mut(&mut tasks, task_id)?;
        task.execution_context_id = None;
        if complete_task {
            task.status = TaskStatus::Completed;
        }
        let updated = task.clone();
        self.write_all(&tasks)?;
        Ok(updated)
    }

    pub(crate) fn list(&self) -> Result<String, TaskError> {
        let tasks = self.load_all()?;
        let mut ready = Vec::new();
        let mut blocked = Vec::new();
        let mut in_progress = Vec::new();
        let mut completed = Vec::new();

        for task in &tasks {
            match task.status {
                TaskStatus::Pending if task.blocked_by.is_empty() => ready.push(task.clone()),
                TaskStatus::Pending => blocked.push(task.clone()),
                TaskStatus::InProgress => in_progress.push(task.clone()),
                TaskStatus::Completed => completed.push(task.clone()),
            }
        }

        serialize_pretty(&TaskListOutput {
            tasks,
            ready,
            blocked,
            in_progress,
            completed,
        })
    }

    fn write_all(&self, tasks: &[TaskItem]) -> Result<(), TaskError> {
        if tasks.is_empty() {
            return Ok(());
        }

        fs::create_dir_all(&self.dir)?;
        for task in tasks {
            self.write_raw_file(
                &self.task_path(task.id),
                &serde_json::to_string_pretty(task)?,
            )?;
        }
        Ok(())
    }

    fn write_raw_file(&self, path: &Path, content: &str) -> Result<(), TaskError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let temp_path = path.with_extension(format!("json.tmp-{unique}"));
        fs::write(&temp_path, content)?;
        fs::rename(&temp_path, path)?;
        Ok(())
    }

    fn task_path(&self, task_id: u64) -> PathBuf {
        self.dir.join(format!("task_{task_id}.json"))
    }

    fn task_file_paths(&self) -> Result<Vec<PathBuf>, TaskError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if is_task_file(&path) {
                paths.push(path);
            }
        }
        Ok(paths)
    }
}

fn validate_unblocked_status(task: &TaskItem) -> Result<(), TaskError> {
    if matches!(task.status, TaskStatus::InProgress | TaskStatus::Completed)
        && !task.blocked_by.is_empty()
    {
        return Err(TaskError::Validation(format!(
            "Task {} cannot be {:?} while blocked by {:?}",
            task.id, task.status, task.blocked_by
        )));
    }

    Ok(())
}

fn validate_claimable(task: &TaskItem, owner: &str) -> Result<(), TaskError> {
    if !task.owner.is_empty() {
        return Err(TaskError::Validation(format!(
            "Task {} is already owned by '{}'",
            task.id, task.owner
        )));
    }
    if !task.blocked_by.is_empty() {
        return Err(TaskError::Validation(format!(
            "Task {} is blocked by {:?} and cannot be claimed",
            task.id, task.blocked_by
        )));
    }
    if task.status != TaskStatus::Pending {
        return Err(TaskError::Validation(format!(
            "Task {} is {:?} and cannot be claimed by '{}'",
            task.id, task.status, owner
        )));
    }

    Ok(())
}

fn validate_update_access(
    task: &TaskItem,
    input: &TaskUpdateInput,
    access: TaskAccess<'_>,
) -> Result<(), TaskError> {
    match access {
        TaskAccess::Lead => Ok(()),
        TaskAccess::Teammate(name) if task.owner == name => {
            if updates_dependencies(input) {
                return Err(TaskError::Validation(format!(
                    "Teammate '{name}' cannot edit dependencies for task {}",
                    task.id
                )));
            }
            if let Some(owner) = &input.owner
                && owner != name
            {
                return Err(TaskError::Validation(format!(
                    "Teammate '{name}' cannot reassign task {} to '{}'",
                    task.id, owner
                )));
            }
            Ok(())
        }
        TaskAccess::Teammate(name) => Err(TaskError::Validation(format!(
            "Teammate '{name}' cannot update task {} owned by '{}'",
            task.id, task.owner
        ))),
    }
}

fn validate_context_access(task: &TaskItem, access: TaskAccess<'_>) -> Result<(), TaskError> {
    match access {
        TaskAccess::Lead => Ok(()),
        TaskAccess::Teammate(name) if task.owner == name => Ok(()),
        TaskAccess::Teammate(name) => Err(TaskError::Validation(format!(
            "Teammate '{name}' cannot manage execution context for task {} owned by '{}'",
            task.id, task.owner
        ))),
    }
}

fn updates_dependencies(input: &TaskUpdateInput) -> bool {
    !input.add_blocked_by.is_empty()
        || !input.remove_blocked_by.is_empty()
        || !input.add_blocks.is_empty()
        || !input.remove_blocks.is_empty()
}

fn is_claimable(task: &TaskItem) -> bool {
    task.status == TaskStatus::Pending && task.blocked_by.is_empty() && task.owner.is_empty()
}

fn find_task_mut(tasks: &mut [TaskItem], task_id: u64) -> Result<&mut TaskItem, TaskError> {
    tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .ok_or_else(|| TaskError::Validation(format!("Task {task_id} does not exist")))
}

fn is_task_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("task_") && name.ends_with(".json"))
        .unwrap_or(false)
}
