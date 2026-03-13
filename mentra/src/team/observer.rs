use std::{path::PathBuf, sync::Arc};

use crate::agent::AgentEvent;

use super::{TeamMemberSummary, TeamProtocolRequestSummary};

pub(crate) trait TeamObserverSink: Send + Sync {
    fn publish_snapshot(
        &self,
        members: &[TeamMemberSummary],
        requests: &[TeamProtocolRequestSummary],
        unread_count: usize,
    );

    fn publish_event(&self, event: AgentEvent);
}

#[derive(Clone)]
pub(crate) struct TeamRegistration {
    pub(crate) agent_name: String,
    pub(crate) team_dir: PathBuf,
    pub(crate) observer: Arc<dyn TeamObserverSink>,
}
