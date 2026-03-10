use std::io::Write;

use mentra::{
    provider::model::{ContentBlock, ModelInfo, ModelProviderKind},
    runtime::{Agent, AgentConfig, AgentEvent, Runtime, ToolCall},
};

#[tokio::main]
async fn main() {
    let mut runtime = Runtime::default();

    runtime.register_provider(
        ModelProviderKind::Anthropic,
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set"),
    );

    let mut agent = runtime
        .spawn_with_config(
            "Foo",
            pick_model(&runtime).await,
            AgentConfig {
                system: Some("You are a helpful CLI agent.".to_string()),
                ..AgentConfig::default()
            },
        )
        .expect("Failed to spawn agent");

    let _cli_watcher = subscribe_events(&agent);

    loop {
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");
        let input = input.trim();
        if input == "exit" {
            break;
        }

        agent
            .send(vec![ContentBlock::Text {
                text: input.to_string(),
            }])
            .await
            .expect("Failed to send message");
    }
}

async fn pick_model(runtime: &Runtime) -> ModelInfo {
    let model = runtime
        .list_models(Some(ModelProviderKind::Anthropic))
        .await
        .expect("Failed to list models")
        .first()
        .expect("No models found")
        .clone();
    println!("Picked model: {:?}", model);
    model
}

fn subscribe_events(agent: &Agent) -> tokio::task::JoinHandle<()> {
    let mut events = agent.subscribe_events();

    tokio::spawn(async move {
        let mut assistant_line_open = false;

        while let Ok(event) = events.recv().await {
            match event {
                AgentEvent::TextDelta { delta, .. } => {
                    assistant_line_open = true;
                    print!("{delta}");
                    std::io::stdout().flush().expect("Failed to flush stdout");
                }
                AgentEvent::ToolUseReady { call, .. } => {
                    end_assistant_line(&mut assistant_line_open);
                    println!("\x1b[33m$ {}\x1b[0m", describe_tool_call(&call));
                }
                AgentEvent::RunFinished => {
                    end_assistant_line(&mut assistant_line_open);
                }
                AgentEvent::RunFailed { error } => {
                    end_assistant_line(&mut assistant_line_open);
                    eprintln!("Agent failed: {error}");
                }
                _ => {}
            }
        }
    })
}

fn describe_tool_call(call: &ToolCall) -> String {
    if call.name == "bash"
        && let Some(command) = call.input.get("command").and_then(|value| value.as_str())
    {
        return command.to_string();
    }

    if call.name == "read_file"
        && let Some(path) = call.input.get("path").and_then(|value| value.as_str())
    {
        if let Some(lines) = call.input.get("lines").and_then(|value| value.as_u64()) {
            return format!("read_file {} ({lines} lines)", path);
        }

        return format!("read_file {} (all lines)", path);
    }

    format!("{} {}", call.name, call.input)
}

fn end_assistant_line(assistant_line_open: &mut bool) {
    if *assistant_line_open {
        println!();
        *assistant_line_open = false;
    }
}
