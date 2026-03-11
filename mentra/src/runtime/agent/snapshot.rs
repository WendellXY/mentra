use crate::{provider::model::Message, runtime::AgentEvent};

use super::{Agent, AgentStatus, PendingAssistantTurn};

impl Agent {
    pub(crate) fn push_history(&mut self, message: Message) {
        self.history.push(message);
        self.sync_history_len();
    }

    pub(crate) fn replace_history(&mut self, history: Vec<Message>) {
        self.history = history;
        self.sync_history_len();
    }

    pub(crate) fn clear_pending_turn(&mut self) {
        self.snapshot.current_text.clear();
        self.snapshot.pending_tool_uses.clear();
        self.publish_snapshot();
    }

    pub(crate) fn publish_pending_turn(&mut self, pending: &PendingAssistantTurn) {
        self.snapshot.current_text = pending.current_text().to_string();
        self.snapshot.pending_tool_uses = pending.pending_tool_use_summaries();
        self.publish_snapshot();
    }

    pub(crate) fn emit_event(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event);
    }

    pub(crate) fn set_status(&mut self, status: AgentStatus) {
        self.snapshot.status = status;
        self.publish_snapshot();
    }

    pub(super) fn restore_history(&mut self, history: Vec<Message>) {
        self.history = history;
        self.sync_history_len();
    }

    fn sync_history_len(&mut self) {
        self.snapshot.history_len = self.history.len();
        self.publish_snapshot();
    }

    pub(super) fn publish_snapshot(&self) {
        self.snapshot_tx.send_replace(self.snapshot.clone());
    }
}
