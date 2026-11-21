//! Fixed, non-grading layout transport for the invented development cases.
use super::*;
use crate::explanation_owned::digest;

/// Exact system prompt for the development layout experiment.
pub const DEVELOPMENT_LAYOUT_PROMPT: &str =
    include_str!("../../fixtures/explanations/development-layout-prompt.txt");

impl<V: DeploymentVerifier> HttpModel<V> {
    /// Request a layout choice for one invented case. The caller independently
    /// checks the response against the fixture and renders the selected layout.
    ///
    /// # Errors
    /// Refuses foreign inputs or prompt pins, limit breaches and transport errors.
    pub async fn choose_development_layout(
        &self,
        request: &Request<'_>,
        case: &str,
        binding: &str,
    ) -> Result<Reply, Failure> {
        let (anchor, prepared) =
            edgeagent_research_application::explanation_evaluation::Suite::development_model_case(
                case,
            )
            .map_err(|_| Failure::InvalidRequest)?;
        let bytes = edgeagent_contracts::canonical_json_bytes(&prepared)
            .map_err(|_| Failure::InvalidRequest)?;
        if prepared["input"].as_str() != Some(binding)
            || request.binding().anchor != anchor
            || request.input() != bytes
            || request.binding().evidence != digest(&bytes)
            || self.prompt != DEVELOPMENT_LAYOUT_PROMPT
        {
            return Err(Failure::InvalidRequest);
        }
        let schema = json!({
            "type":"object","additionalProperties":false,
            "required":["kind","case","input","layout"],
            "properties":{
                "kind":{"const":"research.explanation-development-choice.v1"},
                "case":{"const":case},
                "input":{"const":binding},
                "layout":{"enum":["standard","reader_first"]}
            }
        });
        let _slot = self.busy.try_lock().map_err(|_| Failure::Unavailable)?;
        self.stage.store(1, Ordering::Relaxed);
        let timeout = Duration::from_millis(u64::from(
            request.profile().settings.timeout_ms.min(180_000),
        ));
        tokio::time::timeout(
            timeout,
            self.exchange_with(request, None, &schema, |content| {
                if content.is_empty() || content.len() > 1024 {
                    Err(Failure::InvalidResponse)
                } else {
                    Ok(content.into_bytes())
                }
            }),
        )
        .await
        .map_err(|_| Failure::Timeout)?
    }
}
