use tokio::sync::watch;

use crate::{
    provider::model::{
        ContentBlock, ContentBlockDelta, ContentBlockStart, ModelProviderKind, ProviderEvent, Role,
    },
    runtime::{AgentSnapshot, AgentStatus, Runtime},
};

use super::support::{ScriptedProvider, controlled_stream, model_info};

#[tokio::test]
async fn snapshot_progresses_during_streaming() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let (script, tx) = controlled_stream();
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![script],
    );

    let runtime = Runtime::empty_builder()
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let agent = runtime.spawn("agent", model.clone()).unwrap();
    let mut snapshot = agent.watch_snapshot();

    let send_task = tokio::spawn(async move {
        let mut agent = agent;
        let result = agent
            .send(vec![ContentBlock::Text {
                text: "hello".to_string(),
            }])
            .await;
        (agent, result)
    });

    wait_for_status(&mut snapshot, AgentStatus::Streaming).await;

    tx.send(Ok(ProviderEvent::MessageStarted {
        id: "msg-1".to_string(),
        model: model.id,
        role: Role::Assistant,
    }))
    .unwrap();
    snapshot.changed().await.unwrap();

    tx.send(Ok(ProviderEvent::ContentBlockStarted {
        index: 0,
        kind: ContentBlockStart::Text,
    }))
    .unwrap();
    snapshot.changed().await.unwrap();

    tx.send(Ok(ProviderEvent::ContentBlockDelta {
        index: 0,
        delta: ContentBlockDelta::Text("Hel".to_string()),
    }))
    .unwrap();
    snapshot.changed().await.unwrap();
    assert_eq!(snapshot.borrow().current_text, "Hel");

    tx.send(Ok(ProviderEvent::ContentBlockDelta {
        index: 0,
        delta: ContentBlockDelta::Text("lo".to_string()),
    }))
    .unwrap();
    snapshot.changed().await.unwrap();
    assert_eq!(snapshot.borrow().current_text, "Hello");

    tx.send(Ok(ProviderEvent::ContentBlockStopped { index: 0 }))
        .unwrap();
    tx.send(Ok(ProviderEvent::MessageStopped)).unwrap();
    drop(tx);

    let (agent, result) = send_task.await.unwrap();
    result.unwrap();

    let snapshot = agent.watch_snapshot();
    assert_eq!(snapshot.borrow().status, AgentStatus::Finished);
    assert!(snapshot.borrow().current_text.is_empty());
    assert!(snapshot.borrow().pending_tool_uses.is_empty());
}

async fn wait_for_status(receiver: &mut watch::Receiver<AgentSnapshot>, status: AgentStatus) {
    loop {
        if receiver.borrow().status == status {
            return;
        }
        receiver.changed().await.unwrap();
    }
}
