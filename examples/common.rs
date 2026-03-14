use dotenvy::dotenv;
use mentra::{BuiltinProvider, ModelInfo, ProviderId, Runtime, RuntimePolicy};

#[allow(dead_code)]
pub fn openai_runtime() -> Result<Runtime, Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY must be set before running this example")?;

    Ok(Runtime::builder()
        .with_provider(BuiltinProvider::OpenAI, api_key)
        .with_policy(RuntimePolicy::permissive())
        .build()?)
}

pub async fn openai_model(runtime: &Runtime) -> Result<ModelInfo, Box<dyn std::error::Error>> {
    if let Ok(model) = std::env::var("MENTRA_MODEL") {
        return Ok(ModelInfo::new(model, BuiltinProvider::OpenAI));
    }

    let provider_id = ProviderId::from(BuiltinProvider::OpenAI);
    let mut models = runtime.list_models(Some(&provider_id)).await?;
    models.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    models
        .into_iter()
        .next()
        .ok_or_else(|| "OpenAI did not return any models".into())
}

#[allow(dead_code)]
pub fn first_arg_or(default: &str) -> String {
    std::env::args()
        .nth(1)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}
