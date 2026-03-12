use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::provider::ProviderError;

use super::{error::RuntimeError, store::RuntimeStore};

#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

pub type CancellationFlag = CancellationToken;

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
pub struct RunOptions {
    pub cancellation: Option<CancellationToken>,
    pub deadline: Option<SystemTime>,
    pub retry_budget: usize,
    pub tool_budget: Option<usize>,
    pub model_budget: Option<usize>,
}

impl RunOptions {
    pub(crate) fn check_limits(&self) -> Result<(), RuntimeError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(RuntimeError::Cancelled);
        }

        if self.deadline.is_some_and(|deadline| SystemTime::now() >= deadline) {
            return Err(RuntimeError::DeadlineExceeded);
        }

        Ok(())
    }

    pub(crate) fn tool_budget(&self) -> usize {
        self.tool_budget.unwrap_or(usize::MAX)
    }

    pub(crate) fn model_budget(&self) -> usize {
        self.model_budget.unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Clone)]
pub struct RuntimePolicy {
    allow_shell_commands: bool,
    allow_background_commands: bool,
    allowed_working_roots: Vec<PathBuf>,
    allowed_read_roots: Vec<PathBuf>,
    allowed_env_vars: Vec<String>,
    pub(crate) background_task_limit: Option<usize>,
    pub(crate) command_timeout: Option<Duration>,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            allow_shell_commands: false,
            allow_background_commands: false,
            allowed_working_roots: Vec::new(),
            allowed_read_roots: Vec::new(),
            allowed_env_vars: Vec::new(),
            background_task_limit: Some(8),
            command_timeout: Some(Duration::from_secs(30)),
        }
    }
}

impl RuntimePolicy {
    pub fn permissive() -> Self {
        Self {
            allow_shell_commands: true,
            allow_background_commands: true,
            ..Self::default()
        }
    }

    pub fn allow_shell_commands(mut self, allow: bool) -> Self {
        self.allow_shell_commands = allow;
        self
    }

    pub fn allow_background_commands(mut self, allow: bool) -> Self {
        self.allow_background_commands = allow;
        self
    }

    pub fn with_allowed_working_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowed_working_roots.push(path.into());
        self
    }

    pub fn with_allowed_read_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowed_read_roots.push(path.into());
        self
    }

    pub fn with_allowed_env_var(mut self, name: impl Into<String>) -> Self {
        self.allowed_env_vars.push(name.into());
        self
    }

    pub fn with_max_background_tasks(mut self, limit: usize) -> Self {
        self.background_task_limit = Some(limit);
        self
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = Some(timeout);
        self
    }

    pub(crate) fn authorize_command(
        &self,
        base_dir: &Path,
        cwd: &Path,
        background: bool,
    ) -> Result<(), String> {
        if !self.allow_shell_commands {
            return Err(
                "Shell command execution is disabled by the runtime policy. Use RuntimeBuilder::with_policy(...) to opt in."
                    .to_string(),
            );
        }
        if background && !self.allow_background_commands {
            return Err("Background command execution is disabled by the runtime policy.".to_string());
        }

        if !path_is_allowed(
            cwd,
            base_dir,
            self.allowed_working_roots.as_slice(),
        ) {
            return Err(format!(
                "Working directory '{}' is outside the runtime policy roots",
                cwd.display()
            ));
        }

        Ok(())
    }

    pub(crate) fn authorize_file_read(
        &self,
        base_dir: &Path,
        path: &Path,
    ) -> Result<PathBuf, String> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base_dir.join(path)
        };

        if path_is_allowed(resolved.as_path(), base_dir, self.allowed_read_roots.as_slice()) {
            Ok(resolved)
        } else {
            Err(format!(
                "Path '{}' is outside the runtime policy read roots",
                resolved.display()
            ))
        }
    }
}

fn path_is_allowed(path: &Path, default_root: &Path, extra_roots: &[PathBuf]) -> bool {
    path.starts_with(default_root) || extra_roots.iter().any(|root| path.starts_with(root))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub status_code: Option<i32>,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.success
    }

    pub fn foreground_result(self) -> Result<String, String> {
        if self.success {
            Ok(self.stdout)
        } else {
            let stderr = self.stderr.trim();
            if stderr.is_empty() {
                Err(match self.status_code {
                    Some(code) => format!("Command exited with status {code}"),
                    None => "Command exited unsuccessfully".to_string(),
                })
            } else {
                Err(self.stderr)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandSpec {
    Shell { command: String },
}

impl CommandSpec {
    pub fn display(&self) -> &str {
        match self {
            Self::Shell { command } => command,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub spec: CommandSpec,
    pub cwd: PathBuf,
    pub timeout: Option<Duration>,
}

#[async_trait]
pub trait RuntimeExecutor: Send + Sync {
    async fn run(&self, request: CommandRequest) -> Result<CommandOutput, String>;

    async fn run_command(
        &self,
        command: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, String> {
        self.run(CommandRequest {
            spec: CommandSpec::Shell {
                command: command.to_string(),
            },
            cwd: cwd.to_path_buf(),
            timeout,
        })
        .await
    }
}

pub struct LocalRuntimeExecutor;

#[async_trait]
impl RuntimeExecutor for LocalRuntimeExecutor {
    async fn run(&self, request: CommandRequest) -> Result<CommandOutput, String> {
        let CommandRequest { spec, cwd, timeout } = request;
        let command = match spec {
            CommandSpec::Shell { command } => command,
        };
        let mut process = Command::new("bash");
        process.arg("-c").arg(&command).current_dir(&cwd);

        let output = if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, process.output()).await {
                Ok(result) => result.map_err(|error| format!("Failed to execute command: {error}"))?,
                Err(_) => {
                    return Err(format!(
                        "Command timed out after {}s",
                        timeout.as_secs_f64()
                    ));
                }
            }
        } else {
            process
                .output()
                .await
                .map_err(|error| format!("Failed to execute command: {error}"))?
        };

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
            status_code: output.status.code(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeHookEvent {
    AuthorizationDenied {
        agent_id: String,
        action: String,
        detail: String,
    },
    RecoveryPrepared {
        runtime_instance_id: String,
    },
    ModelRequestStarted {
        agent_id: String,
        model: String,
        attempt: usize,
    },
    ModelRequestFinished {
        agent_id: String,
        model: String,
        attempt: usize,
        success: bool,
        error: Option<String>,
    },
    ToolExecutionStarted {
        agent_id: String,
        tool_name: String,
        tool_call_id: String,
    },
    ToolExecutionFinished {
        agent_id: String,
        tool_name: String,
        tool_call_id: String,
        is_error: bool,
        error: Option<String>,
        output_preview: String,
    },
    PolicyDenied {
        agent_id: String,
        tool_name: String,
        reason: String,
    },
    BackgroundTaskStarted {
        agent_id: String,
        task_id: String,
        command: String,
        cwd: PathBuf,
    },
    BackgroundTaskFinished {
        agent_id: String,
        task_id: String,
        status: String,
    },
    RunAborted {
        agent_id: String,
        reason: String,
    },
}

impl RuntimeHookEvent {
    fn scope(&self) -> String {
        match self {
            Self::AuthorizationDenied { agent_id, .. } => agent_id.clone(),
            Self::RecoveryPrepared { runtime_instance_id } => runtime_instance_id.clone(),
            Self::ModelRequestStarted { agent_id, .. }
            | Self::ModelRequestFinished { agent_id, .. }
            | Self::ToolExecutionStarted { agent_id, .. }
            | Self::ToolExecutionFinished { agent_id, .. }
            | Self::PolicyDenied { agent_id, .. }
            | Self::BackgroundTaskStarted { agent_id, .. }
            | Self::BackgroundTaskFinished { agent_id, .. }
            | Self::RunAborted { agent_id, .. } => agent_id.clone(),
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            Self::AuthorizationDenied { .. } => "authorization_denied",
            Self::RecoveryPrepared { .. } => "recovery_prepared",
            Self::ModelRequestStarted { .. } => "model_request_started",
            Self::ModelRequestFinished { .. } => "model_request_finished",
            Self::ToolExecutionStarted { .. } => "tool_execution_started",
            Self::ToolExecutionFinished { .. } => "tool_execution_finished",
            Self::PolicyDenied { .. } => "policy_denied",
            Self::BackgroundTaskStarted { .. } => "background_task_started",
            Self::BackgroundTaskFinished { .. } => "background_task_finished",
            Self::RunAborted { .. } => "run_aborted",
        }
    }
}

pub trait RuntimeHook: Send + Sync {
    fn on_event(
        &self,
        store: &dyn RuntimeStore,
        event: &RuntimeHookEvent,
    ) -> Result<(), RuntimeError>;
}

pub struct AuditHook;
pub type AuditLogHook = AuditHook;

impl RuntimeHook for AuditHook {
    fn on_event(
        &self,
        store: &dyn RuntimeStore,
        event: &RuntimeHookEvent,
    ) -> Result<(), RuntimeError> {
        store.record_audit_event(
            &event.scope(),
            event.event_type(),
            serde_json::to_value(event).map_err(|error| RuntimeError::Store(error.to_string()))?,
        )
    }
}

#[derive(Clone, Default)]
pub struct RuntimeHooks {
    hooks: Vec<Arc<dyn RuntimeHook>>,
}

impl RuntimeHooks {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn with_hook<H>(mut self, hook: H) -> Self
    where
        H: RuntimeHook + 'static,
    {
        self.hooks.push(Arc::new(hook));
        self
    }

    pub fn extend<I>(mut self, hooks: I) -> Self
    where
        I: IntoIterator<Item = Arc<dyn RuntimeHook>>,
    {
        self.hooks.extend(hooks);
        self
    }

    pub fn emit(
        &self,
        store: &dyn RuntimeStore,
        event: &RuntimeHookEvent,
    ) -> Result<(), RuntimeError> {
        for hook in &self.hooks {
            hook.on_event(store, event)?;
        }
        Ok(())
    }
}

pub async fn read_limited_file(path: &Path, max_lines: Option<usize>) -> Result<String, String> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("Failed to open file: {error}"))?;
    let mut lines = tokio::io::BufReader::new(file).lines();
    let mut content = Vec::new();

    loop {
        if let Some(limit) = max_lines
            && content.len() >= limit
        {
            break;
        }

        match lines.next_line().await {
            Ok(Some(line)) => content.push(line),
            Ok(None) => break,
            Err(error) => return Err(format!("Failed to read file: {error}")),
        }
    }

    Ok(content.join("\n"))
}

pub(crate) fn is_transient_provider_error(error: &ProviderError) -> bool {
    match error {
        ProviderError::Transport(_) | ProviderError::Decode(_) => true,
        ProviderError::Http { status, .. } => {
            status.is_server_error()
                || *status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || *status == reqwest::StatusCode::REQUEST_TIMEOUT
        }
        ProviderError::Serialize(_)
        | ProviderError::Deserialize(_)
        | ProviderError::InvalidRequest(_)
        | ProviderError::InvalidResponse(_)
        | ProviderError::MalformedStream(_) => false,
    }
}
