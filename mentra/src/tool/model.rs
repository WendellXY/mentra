use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::{
    BackgroundTaskSummary, ContextCompactionDetails, ContextCompactionTrigger,
    SpawnedAgentStatus, SpawnedAgentSummary, TaskItem, TeamDispatch, TeamMemberSummary,
    TeamMessage, TeamProtocolRequestSummary,
};
use crate::runtime::RuntimeError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCapability {
    ReadOnly,
    FilesystemRead,
    FilesystemWrite,
    ProcessExec,
    BackgroundExec,
    TaskMutation,
    TeamCoordination,
    Delegation,
    ContextCompaction,
    SkillLoad,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolSideEffectLevel {
    #[default]
    None,
    LocalState,
    Process,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolDurability {
    #[default]
    Ephemeral,
    Persistent,
    ReplaySafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub capabilities: Vec<ToolCapability>,
    pub side_effect_level: ToolSideEffectLevel,
    pub durability: ToolDurability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

pub struct ToolContext<'a> {
    pub agent_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub(crate) working_directory: PathBuf,
    pub(crate) runtime: crate::runtime::RuntimeHandle,
    pub(crate) agent: &'a mut crate::runtime::Agent,
}

impl ToolContext<'_> {
    pub fn working_directory(&self) -> &Path {
        self.working_directory.as_path()
    }

    pub fn agent_name(&self) -> &str {
        self.agent.name()
    }

    pub fn model(&self) -> &str {
        self.agent.model()
    }

    pub fn history_len(&self) -> usize {
        self.agent.history().len()
    }

    pub fn tasks(&self) -> &[TaskItem] {
        self.agent.tasks()
    }

    pub fn resolve_working_directory(
        &self,
        working_directory: Option<&str>,
    ) -> Result<PathBuf, String> {
        self.runtime
            .resolve_working_directory(&self.agent_id, working_directory)
    }

    pub fn load_skill(&self, name: &str) -> Result<String, String> {
        self.runtime.load_skill(name)
    }

    pub fn skill_descriptions(&self) -> Option<String> {
        self.runtime.skill_descriptions()
    }

    pub async fn execute_shell_command(
        &self,
        command: String,
        cwd: PathBuf,
    ) -> Result<crate::runtime::CommandOutput, String> {
        self.runtime
            .execute_shell_command(&self.agent_id, command, cwd)
            .await
    }

    pub fn start_background_task(
        &self,
        command: String,
        cwd: PathBuf,
    ) -> Result<BackgroundTaskSummary, String> {
        self.runtime
            .start_background_task(&self.agent_id, command, cwd)
    }

    pub fn check_background_task(&self, task_id: Option<&str>) -> Result<String, String> {
        self.runtime.check_background_task(&self.agent_id, task_id)
    }

    pub fn request_idle(&mut self) {
        self.agent.request_idle();
    }

    pub async fn compact_history(
        &mut self,
    ) -> Result<Option<ContextCompactionDetails>, RuntimeError> {
        self.agent
            .compact_history(
                self.agent.history().len().saturating_sub(1),
                ContextCompactionTrigger::Manual,
            )
            .await
    }

    pub fn execute_task_tool(&self, tool_name: &str, input: Value) -> Result<String, String> {
        self.agent.execute_task_mutation(tool_name, input)
    }

    pub fn refresh_tasks(&mut self) -> Result<(), RuntimeError> {
        self.agent.refresh_tasks_from_disk()
    }

    pub async fn read_file(
        &self,
        path: &str,
        max_lines: Option<usize>,
    ) -> Result<String, String> {
        self.runtime.read_file(&self.agent_id, path, max_lines).await
    }

    pub fn spawn_subagent(&self) -> Result<crate::runtime::Agent, RuntimeError> {
        self.agent.spawn_subagent()
    }

    pub fn register_subagent(&mut self, agent: &crate::runtime::Agent) -> SpawnedAgentSummary {
        self.agent.register_subagent(agent)
    }

    pub fn finish_subagent(
        &mut self,
        id: &str,
        status: SpawnedAgentStatus,
    ) -> Option<SpawnedAgentSummary> {
        self.agent.finish_subagent(id, status)
    }

    pub async fn spawn_teammate(
        &mut self,
        name: impl Into<String>,
        role: impl Into<String>,
        prompt: Option<String>,
    ) -> Result<TeamMemberSummary, RuntimeError> {
        self.agent.spawn_teammate(name, role, prompt).await
    }

    pub fn send_team_message(
        &self,
        to: &str,
        content: impl Into<String>,
    ) -> Result<TeamDispatch, RuntimeError> {
        self.agent.send_team_message(to, content)
    }

    pub fn broadcast_team_message(
        &self,
        content: impl Into<String>,
    ) -> Result<Vec<TeamDispatch>, RuntimeError> {
        self.agent.broadcast_team_message(content)
    }

    pub fn read_team_inbox(&self) -> Result<Vec<TeamMessage>, RuntimeError> {
        self.agent.read_team_inbox()
    }

    pub fn request_team_protocol(
        &self,
        to: &str,
        protocol: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<TeamProtocolRequestSummary, RuntimeError> {
        self.agent.request_team_protocol(to, protocol, content)
    }

    pub fn respond_team_protocol(
        &self,
        request_id: &str,
        approve: bool,
        reason: Option<String>,
    ) -> Result<TeamProtocolRequestSummary, RuntimeError> {
        self.agent.respond_team_protocol(request_id, approve, reason)
    }

}

pub type ToolResult = Result<String, String>;

#[async_trait]
pub trait ExecutableTool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    async fn execute(&self, ctx: ToolContext<'_>, input: Value) -> ToolResult;
}
