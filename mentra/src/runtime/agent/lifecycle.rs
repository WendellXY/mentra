use crate::{ContentBlock, Message, Role, runtime::error::RuntimeError};

use super::{Agent, AgentEvent, AgentStatus, TurnRunner};

impl Agent {
    pub async fn send(
        &mut self,
        content: impl Into<Vec<ContentBlock>>,
    ) -> Result<(), RuntimeError> {
        self.idle_requested = false;
        self.refresh_tasks_from_disk()?;
        let history_before_run = self.history.clone();
        let tasks_before_run = self.tasks.clone();
        let rounds_before_run = self.rounds_since_task;
        let task_disk_state = self.capture_task_disk_state()?;
        self.push_history(Message {
            role: Role::User,
            content: content.into(),
        });
        self.emit_event(AgentEvent::RunStarted);

        match TurnRunner::new(self).run().await {
            Ok(()) => {
                self.clear_inflight_team_messages();
                self.clear_inflight_background_notifications();
                self.clear_pending_turn();
                self.set_status(AgentStatus::Finished);
                self.emit_event(AgentEvent::RunFinished);
                Ok(())
            }
            Err(error) => {
                self.idle_requested = false;
                self.requeue_inflight_team_messages()?;
                self.requeue_inflight_background_notifications();
                self.restore_history(history_before_run);
                self.restore_task_state(tasks_before_run, rounds_before_run, &task_disk_state)?;
                self.clear_pending_turn();
                let message = format!("{error:?}");
                self.set_status(AgentStatus::Failed(message.clone()));
                self.emit_event(AgentEvent::RunFailed { error: message });
                Err(error)
            }
        }
    }

    pub(crate) fn request_idle(&mut self) {
        self.idle_requested = true;
    }

    pub(crate) fn take_idle_requested(&mut self) -> bool {
        std::mem::take(&mut self.idle_requested)
    }
}
