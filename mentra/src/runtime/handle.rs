mod agents;
mod construction;
mod execution;
mod tooling;

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use tokio::sync::{broadcast, watch};

use crate::{
    agent::{AgentEvent, AgentSnapshot},
    runtime::{
        background::{BackgroundNotification, BackgroundTaskManager, BackgroundTaskSummary},
        control::{
            AuditHook, CommandOutput, CommandRequest, CommandSpec, LocalRuntimeExecutor,
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
