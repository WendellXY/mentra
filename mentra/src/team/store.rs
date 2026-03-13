use std::path::Path;

use crate::error::RuntimeError;

use super::{TeamMemberSummary, TeamMessage, TeamProtocolRequestSummary};

pub trait TeamStore: Send + Sync {
    fn unread_team_count(&self, team_dir: &Path, agent_name: &str) -> Result<usize, RuntimeError>;
    fn load_team_members(&self, team_dir: &Path) -> Result<Vec<TeamMemberSummary>, RuntimeError>;
    fn upsert_team_member(
        &self,
        team_dir: &Path,
        summary: &TeamMemberSummary,
    ) -> Result<(), RuntimeError>;
    fn read_team_inbox(
        &self,
        team_dir: &Path,
        agent_name: &str,
    ) -> Result<Vec<TeamMessage>, RuntimeError>;
    fn ack_team_inbox(&self, team_dir: &Path, agent_name: &str) -> Result<(), RuntimeError>;
    fn requeue_team_inbox(&self, team_dir: &Path, agent_name: &str) -> Result<(), RuntimeError>;
    fn append_team_message(
        &self,
        team_dir: &Path,
        recipient: &str,
        message: &TeamMessage,
    ) -> Result<(), RuntimeError>;
    fn load_team_requests(
        &self,
        team_dir: &Path,
    ) -> Result<Vec<TeamProtocolRequestSummary>, RuntimeError>;
    fn upsert_team_request(
        &self,
        team_dir: &Path,
        request: &TeamProtocolRequestSummary,
    ) -> Result<(), RuntimeError>;
    fn list_team_agent_names(&self, team_dir: &Path) -> Result<Vec<String>, RuntimeError>;
}
