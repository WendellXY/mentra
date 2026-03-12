use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use crate::{
    runtime::{
        AgentEvent, AgentSnapshot,
        background::{BackgroundNotification, BackgroundTaskManager, BackgroundTaskSummary},
        control::{
            AuditHook, CommandRequest, CommandSpec, CommandOutput, LocalRuntimeExecutor,
            RuntimeExecutor, RuntimeHookEvent, RuntimeHooks, RuntimePolicy, read_limited_file,
        },
        error::RuntimeError,
        store::{RuntimeStore, SqliteRuntimeStore},
        task::{self, TaskAccess},
        team::{
            TeamDispatch, TeamManager, TeamMemberSummary, TeamMessage, TeamProtocolRequestSummary,
            TeamRequestFilter,
        },
    },
    tool::{ExecutableTool, ToolRegistry, ToolSpec},
};
use tokio::sync::{broadcast, watch};

use super::skill::SkillLoader;

#[derive(Clone)]
pub struct RuntimeHandle {
    pub(crate) tool_registry: Arc<RwLock<ToolRegistry>>,
    pub(crate) skill_loader: Arc<RwLock<Option<SkillLoader>>>,
    pub(crate) background_tasks: BackgroundTaskManager,
    pub(crate) team: TeamManager,
    pub(crate) store: Arc<dyn RuntimeStore>,
    pub(crate) executor: Arc<dyn RuntimeExecutor>,
    pub(crate) policy: Arc<RuntimePolicy>,
    pub(crate) hooks: RuntimeHooks,
    pub(crate) runtime_intrinsics_enabled: bool,
    runtime_instance_id: String,
    agent_contexts: Arc<RwLock<HashMap<String, AgentExecutionConfig>>>,
}

#[derive(Clone)]
pub(crate) struct AgentObserver {
    pub(crate) events: broadcast::Sender<AgentEvent>,
    pub(crate) snapshot_tx: watch::Sender<AgentSnapshot>,
    pub(crate) snapshot: Arc<Mutex<AgentSnapshot>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentExecutionConfig {
    pub(crate) name: String,
    pub(crate) team_dir: PathBuf,
    pub(crate) tasks_dir: PathBuf,
    pub(crate) base_dir: PathBuf,
    pub(crate) auto_route_shell: bool,
    pub(crate) is_teammate: bool,
}

impl RuntimeHandle {
    pub fn new() -> Self {
        Self::with_components(
            Arc::new(SqliteRuntimeStore::default()),
            Arc::new(LocalRuntimeExecutor),
            Arc::new(RuntimePolicy::default()),
            RuntimeHooks::new().with_hook(AuditHook),
            true,
        )
    }

    pub fn new_empty() -> Self {
        Self::with_components(
            Arc::new(SqliteRuntimeStore::default()),
            Arc::new(LocalRuntimeExecutor),
            Arc::new(RuntimePolicy::default()),
            RuntimeHooks::new().with_hook(AuditHook),
            false,
        )
    }

    fn with_components(
        store: Arc<dyn RuntimeStore>,
        executor: Arc<dyn RuntimeExecutor>,
        policy: Arc<RuntimePolicy>,
        hooks: RuntimeHooks,
        runtime_intrinsics_enabled: bool,
    ) -> Self {
        let _ = store.prepare_recovery();
        let runtime_instance_id = format!("runtime-{}", std::process::id());
        let mut tool_registry = if runtime_intrinsics_enabled {
            ToolRegistry::default()
        } else {
            ToolRegistry::new_empty()
        };
        if runtime_intrinsics_enabled {
            crate::runtime::intrinsic::register_tools(&mut tool_registry);
        }
        let handle = Self {
            tool_registry: Arc::new(RwLock::new(tool_registry)),
            skill_loader: Arc::new(RwLock::new(None)),
            background_tasks: BackgroundTaskManager::new(store.clone(), executor.clone(), hooks.clone()),
            team: TeamManager::new(store.clone()),
            store,
            executor,
            policy,
            hooks,
            runtime_intrinsics_enabled,
            runtime_instance_id,
            agent_contexts: Arc::new(RwLock::new(HashMap::new())),
        };
        let _ = handle.emit_hook(RuntimeHookEvent::RecoveryPrepared {
            runtime_instance_id: handle.runtime_instance_id.clone(),
        });
        handle
    }

    pub fn rebind_store(&self, store: Arc<dyn RuntimeStore>) -> Self {
        let _ = store.prepare_recovery();
        let handle = Self {
            tool_registry: Arc::new(RwLock::new(
                self.tool_registry
                    .read()
                    .expect("tool registry poisoned")
                    .clone(),
            )),
            skill_loader: Arc::new(RwLock::new(
                self.skill_loader
                    .read()
                    .expect("skill loader poisoned")
                    .clone(),
            )),
            background_tasks: BackgroundTaskManager::new(
                store.clone(),
                self.executor.clone(),
                self.hooks.clone(),
            ),
            team: TeamManager::new(store.clone()),
            store,
            executor: self.executor.clone(),
            policy: self.policy.clone(),
            hooks: self.hooks.clone(),
            runtime_intrinsics_enabled: self.runtime_intrinsics_enabled,
            runtime_instance_id: format!("runtime-{}", std::process::id()),
            agent_contexts: Arc::new(RwLock::new(HashMap::new())),
        };
        let _ = handle.emit_hook(RuntimeHookEvent::RecoveryPrepared {
            runtime_instance_id: handle.runtime_instance_id.clone(),
        });
        handle
    }

    pub fn with_executor(&self, executor: Arc<dyn RuntimeExecutor>) -> Self {
        Self {
            tool_registry: Arc::new(RwLock::new(
                self.tool_registry
                    .read()
                    .expect("tool registry poisoned")
                    .clone(),
            )),
            skill_loader: Arc::new(RwLock::new(
                self.skill_loader
                    .read()
                    .expect("skill loader poisoned")
                    .clone(),
            )),
            background_tasks: BackgroundTaskManager::new(
                self.store.clone(),
                executor.clone(),
                self.hooks.clone(),
            ),
            team: self.team.clone(),
            store: self.store.clone(),
            executor,
            policy: self.policy.clone(),
            hooks: self.hooks.clone(),
            runtime_intrinsics_enabled: self.runtime_intrinsics_enabled,
            runtime_instance_id: format!("runtime-{}", std::process::id()),
            agent_contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_policy(&self, policy: RuntimePolicy) -> Self {
        Self {
            tool_registry: Arc::new(RwLock::new(
                self.tool_registry
                    .read()
                    .expect("tool registry poisoned")
                    .clone(),
            )),
            skill_loader: Arc::new(RwLock::new(
                self.skill_loader
                    .read()
                    .expect("skill loader poisoned")
                    .clone(),
            )),
            background_tasks: BackgroundTaskManager::new(
                self.store.clone(),
                self.executor.clone(),
                self.hooks.clone(),
            ),
            team: self.team.clone(),
            store: self.store.clone(),
            executor: self.executor.clone(),
            policy: Arc::new(policy),
            hooks: self.hooks.clone(),
            runtime_intrinsics_enabled: self.runtime_intrinsics_enabled,
            runtime_instance_id: format!("runtime-{}", std::process::id()),
            agent_contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_hooks(&self, hooks: RuntimeHooks) -> Self {
        Self {
            tool_registry: Arc::new(RwLock::new(
                self.tool_registry
                    .read()
                    .expect("tool registry poisoned")
                    .clone(),
            )),
            skill_loader: Arc::new(RwLock::new(
                self.skill_loader
                    .read()
                    .expect("skill loader poisoned")
                    .clone(),
            )),
            background_tasks: BackgroundTaskManager::new(
                self.store.clone(),
                self.executor.clone(),
                hooks.clone(),
            ),
            team: self.team.clone(),
            store: self.store.clone(),
            executor: self.executor.clone(),
            policy: self.policy.clone(),
            hooks,
            runtime_intrinsics_enabled: self.runtime_intrinsics_enabled,
            runtime_instance_id: format!("runtime-{}", std::process::id()),
            agent_contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_tool<T>(&self, tool: T)
    where
        T: ExecutableTool + 'static,
    {
        self.tool_registry
            .write()
            .expect("tool registry poisoned")
            .register_tool(tool);
    }

    pub fn register_skill_loader(&self, loader: SkillLoader) {
        *self.skill_loader.write().expect("skill loader poisoned") = Some(loader);
        self.tool_registry
            .write()
            .expect("tool registry poisoned")
            .register_tool(crate::tool::builtin::LoadSkillTool);
    }

    pub fn tools(&self) -> Arc<[ToolSpec]> {
        self.tool_registry
            .read()
            .expect("tool registry poisoned")
            .tools()
    }

    pub fn store(&self) -> Arc<dyn RuntimeStore> {
        self.store.clone()
    }

    pub fn skill_descriptions(&self) -> Option<String> {
        self.skill_loader
            .read()
            .expect("skill loader poisoned")
            .as_ref()
            .map(SkillLoader::get_descriptions)
            .filter(|descriptions| !descriptions.is_empty())
    }

    pub fn load_skill(&self, name: &str) -> Result<String, String> {
        let skills = self.skill_loader.read().expect("skill loader poisoned");
        let Some(loader) = skills.as_ref() else {
            return Err("Skill loader is not available".to_string());
        };

        loader.get_content(name)
    }

    pub fn register_agent(
        &self,
        agent_id: &str,
        agent_name: &str,
        config: AgentExecutionConfig,
        observer: &AgentObserver,
    ) -> Result<(), RuntimeError> {
        self.acquire_agent_lease(agent_id)?;
        self.background_tasks.register_agent(agent_id, observer);
        self.team.register_agent(agent_name, &config, observer)?;
        self.agent_contexts
            .write()
            .expect("agent context registry poisoned")
            .insert(agent_id.to_string(), config);
        Ok(())
    }

    pub fn start_background_task(
        &self,
        agent_id: &str,
        command: String,
        cwd: PathBuf,
    ) -> Result<BackgroundTaskSummary, String> {
        let config = self.agent_config(agent_id)?;
        if let Err(detail) = self.policy.authorize_command(&config.base_dir, &cwd, true) {
            let _ = self.emit_hook(RuntimeHookEvent::AuthorizationDenied {
                agent_id: agent_id.to_string(),
                action: "background_command".to_string(),
                detail: detail.clone(),
            });
            return Err(detail);
        }

        if let Some(limit) = self.policy.background_task_limit
            && self.background_tasks.running_task_count(agent_id) >= limit
        {
            let detail = format!("Background task limit of {limit} reached");
            let _ = self.emit_hook(RuntimeHookEvent::AuthorizationDenied {
                agent_id: agent_id.to_string(),
                action: "background_limit".to_string(),
                detail: detail.clone(),
            });
            return Err(detail);
        }

        self.background_tasks.start_task(
            agent_id,
            CommandRequest {
                spec: CommandSpec::Shell { command },
                cwd,
                timeout: self.policy.command_timeout,
            },
        )
    }

    pub fn check_background_task(
        &self,
        agent_id: &str,
        task_id: Option<&str>,
    ) -> Result<String, String> {
        self.background_tasks.check_task(agent_id, task_id)
    }

    pub fn drain_background_notifications(&self, agent_id: &str) -> Vec<BackgroundNotification> {
        self.background_tasks.drain_notifications(agent_id)
    }

    pub fn requeue_background_notifications(
        &self,
        agent_id: &str,
        notifications: Vec<BackgroundNotification>,
    ) {
        self.background_tasks
            .requeue_notifications(agent_id, notifications);
    }

    pub fn acknowledge_background_notifications(&self, agent_id: &str) {
        self.background_tasks.acknowledge_notifications(agent_id);
    }

    pub fn team_manager(&self) -> TeamManager {
        self.team.clone()
    }

    pub fn register_teammate(
        &self,
        team_dir: &Path,
        summary: TeamMemberSummary,
        wake_tx: tokio::sync::mpsc::UnboundedSender<()>,
        task: std::thread::JoinHandle<()>,
    ) -> Result<TeamMemberSummary, RuntimeError> {
        self.team.spawn_teammate(team_dir, summary, wake_tx, task)
    }

    pub fn wake_teammate(&self, team_dir: &Path, teammate_name: &str) -> Result<(), RuntimeError> {
        self.team.wake_teammate(team_dir, teammate_name)
    }

    pub fn send_team_message(
        &self,
        team_dir: &Path,
        sender: &str,
        to: &str,
        content: String,
    ) -> Result<TeamDispatch, RuntimeError> {
        self.team.send_message(team_dir, sender, to, content)
    }

    pub fn broadcast_team_message(
        &self,
        team_dir: &Path,
        sender: &str,
        content: String,
    ) -> Result<Vec<TeamDispatch>, RuntimeError> {
        self.team.broadcast_message(team_dir, sender, content)
    }

    pub fn read_team_inbox(
        &self,
        team_dir: &Path,
        agent_name: &str,
    ) -> Result<Vec<TeamMessage>, RuntimeError> {
        self.team.read_inbox(team_dir, agent_name)
    }

    pub fn requeue_team_messages(
        &self,
        team_dir: &Path,
        agent_name: &str,
        messages: Vec<TeamMessage>,
    ) -> Result<(), RuntimeError> {
        self.team.requeue_messages(team_dir, agent_name, messages)
    }

    pub fn acknowledge_team_messages(
        &self,
        team_dir: &Path,
        agent_name: &str,
    ) -> Result<(), RuntimeError> {
        self.team.acknowledge_messages(team_dir, agent_name)
    }

    pub fn create_team_request(
        &self,
        team_dir: &Path,
        sender: &str,
        to: &str,
        protocol: String,
        content: String,
    ) -> Result<TeamProtocolRequestSummary, RuntimeError> {
        self.team
            .create_request(team_dir, sender, to, protocol, content)
    }

    pub fn resolve_team_request(
        &self,
        team_dir: &Path,
        responder: &str,
        request_id: &str,
        approve: bool,
        reason: Option<String>,
    ) -> Result<TeamProtocolRequestSummary, RuntimeError> {
        self.team
            .resolve_request(team_dir, responder, request_id, approve, reason)
    }

    pub fn list_team_requests(
        &self,
        team_dir: &Path,
        agent_name: &str,
        filter: TeamRequestFilter,
    ) -> Result<Vec<TeamProtocolRequestSummary>, RuntimeError> {
        self.team.list_requests(team_dir, agent_name, filter)
    }

    pub fn execute_task_mutation(
        &self,
        tool_name: &str,
        input: serde_json::Value,
        dir: &Path,
        access: TaskAccess<'_>,
    ) -> Result<String, String> {
        task::execute_with_store(self.store.as_ref(), tool_name, input, dir, access)
    }

    pub async fn execute_shell_command(
        &self,
        agent_id: &str,
        command: String,
        cwd: PathBuf,
    ) -> Result<CommandOutput, String> {
        let config = self.agent_config(agent_id)?;
        if let Err(detail) = self.policy.authorize_command(&config.base_dir, &cwd, false) {
            let _ = self.emit_hook(RuntimeHookEvent::AuthorizationDenied {
                agent_id: agent_id.to_string(),
                action: "shell_command".to_string(),
                detail: detail.clone(),
            });
            return Err(detail);
        }

        self.executor
            .run(CommandRequest {
                spec: CommandSpec::Shell { command },
                cwd,
                timeout: self.policy.command_timeout,
            })
            .await
    }

    pub async fn read_file(
        &self,
        agent_id: &str,
        path: &str,
        max_lines: Option<usize>,
    ) -> Result<String, String> {
        let config = self.agent_config(agent_id)?;
        let resolved = match self
            .policy
            .authorize_file_read(&config.base_dir, Path::new(path))
        {
            Ok(path) => path,
            Err(detail) => {
                let _ = self.emit_hook(RuntimeHookEvent::AuthorizationDenied {
                    agent_id: agent_id.to_string(),
                    action: "read_file".to_string(),
                    detail: detail.clone(),
                });
                return Err(detail);
            }
        };

        read_limited_file(&resolved, max_lines).await
    }

    pub fn resolve_working_directory(
        &self,
        agent_id: &str,
        explicit_directory: Option<&str>,
    ) -> Result<PathBuf, String> {
        let config = self
            .agent_contexts
            .read()
            .expect("agent context registry poisoned")
            .get(agent_id)
            .cloned()
            .ok_or_else(|| format!("Unknown agent '{agent_id}'"))?;

        if let Some(directory) = explicit_directory {
            return Ok(resolve_path(&config.base_dir, directory));
        }

        if !config.auto_route_shell {
            return Ok(config.base_dir);
        }

        let tasks = self
            .store
            .load_tasks(&config.tasks_dir)
            .map_err(|error| format!("{error:?}"))?;
        let owned = tasks
            .into_iter()
            .filter(|task| {
                config.is_teammate
                    && task.owner == config.name
                    && !matches!(task.status, crate::runtime::TaskStatus::Completed)
            })
            .collect::<Vec<_>>();

        let directories = owned
            .iter()
            .filter_map(|task| task.working_directory.as_deref())
            .map(|path| resolve_path(&config.base_dir, path))
            .collect::<BTreeSet<_>>();

        if directories.is_empty() {
            return Ok(config.base_dir);
        }

        if directories.len() > 1 {
            return Err(
                "Multiple owned task directories are active. Pass workingDirectory explicitly."
                    .to_string(),
            );
        }

        Ok(directories.into_iter().next().expect("one directory"))
    }

    pub fn default_working_directory(&self, agent_id: &str) -> PathBuf {
        self.agent_contexts
            .read()
            .expect("agent context registry poisoned")
            .get(agent_id)
            .map(|config| config.base_dir.clone())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn emit_hook(&self, event: RuntimeHookEvent) -> Result<(), RuntimeError> {
        self.hooks.emit(self.store.as_ref(), &event)
    }

    pub fn get_tool(&self, name: &str) -> Option<std::sync::Arc<dyn ExecutableTool>> {
        self.tool_registry
            .read()
            .expect("tool registry poisoned")
            .get_tool(name)
    }

    pub fn acquire_agent_lease(&self, agent_id: &str) -> Result<(), RuntimeError> {
        let key = format!("agent:{agent_id}");
        let acquired = self
            .store
            .acquire_lease(&key, &self.runtime_instance_id, Duration::from_secs(3600))?;
        if acquired {
            Ok(())
        } else {
            Err(RuntimeError::LeaseUnavailable(format!(
                "Agent '{agent_id}' is already leased by another runtime"
            )))
        }
    }

    fn agent_config(&self, agent_id: &str) -> Result<AgentExecutionConfig, String> {
        self.agent_contexts
            .read()
            .expect("agent context registry poisoned")
            .get(agent_id)
            .cloned()
            .ok_or_else(|| format!("Unknown agent '{agent_id}'"))
    }
}

fn resolve_path(base_dir: &Path, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    }
}

impl Default for RuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}
