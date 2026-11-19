//! Non-authorizing observation over the exact production exchange and renderer.
use super::*;
use edgeagent_research_application::explanation_evaluation::Input;

/// Payload-free result, deliberately not a model Reply or a qualification record.
#[derive(serde::Serialize)]
pub struct Measurement {
    /// Closed application-owned status, never provider text.
    pub status: &'static str,
    /// Last transport stage, including on timeout.
    pub stage: ModelStage,
    /// HTTP/identity/validation/render duration; excludes model startup.
    pub elapsed_ms: u64,
    /// Bounded status/capacity metadata; never provider messages or response bytes.
    pub response: ResponseDiagnostic,
}

impl<V: DeploymentVerifier> HttpModel<V> {
    /// Observe a separately invented calibration under a bounded measurement cap.
    /// Uses the existing transport without changing its normal profile deadline.
    /// Drops every reply and rendered text before returning payload-free metrics.
    pub async fn measure(
        &self,
        request: &Request<'_>,
        input: &Input,
        ceiling: Duration,
    ) -> Measurement {
        let started = Instant::now();
        let mut response = ResponseDiagnostic::default();
        let status = async {
            let Ok(_slot) = self.busy.try_lock() else {
                return "unavailable";
            };
            self.stage.store(0, Ordering::Relaxed);
            let calibration = matches!(input.artifact(), edgeagent_contracts::research::ResearchRecord::Artifact(a)
                if a.request.scope == "performance" && ["performance-P01", "performance-P02", "performance-P03"].contains(&a.request.request_id.as_str()));
            if !calibration || ceiling.is_zero()
                || ceiling > Duration::from_secs(180)
                || input.prepared_input() != Some(request.input())
            {
                return "invalid_request";
            }
            self.stage.store(1, Ordering::Relaxed);
            match tokio::time::timeout(ceiling, async {
                self.exchange(request, Some(&mut response)).await.map(|reply| {
                    // The model reply and deterministic presentation cannot leave here.
                    if input.render_candidate(reply.proposal()).is_ok() {
                        "valid"
                    } else {
                        "invalid_proposal"
                    }
                })
            })
            .await
            {
                Err(_) | Ok(Err(Failure::Timeout)) => "timeout",
                Ok(Ok(status)) => status,
                Ok(Err(Failure::InvalidRequest)) => "invalid_request",
                Ok(Err(Failure::Unavailable)) => "unavailable",
                Ok(Err(Failure::Refused)) => "refused",
                Ok(Err(Failure::BudgetExhausted)) => "budget_exhausted",
                Ok(Err(Failure::DeploymentMismatch)) => "deployment_mismatch",
                Ok(Err(Failure::InvalidResponse)) => "invalid_response",
            }
        }
        .await;
        let status = if status == "valid" && started.elapsed() > ceiling {
            "timeout"
        } else {
            status
        };
        Measurement {
            status,
            response,
            stage: self.diagnostic_stage(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}
