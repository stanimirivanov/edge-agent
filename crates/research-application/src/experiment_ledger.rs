//! Reconstruct a bounded, append-only experiment family from canonical owner facts.
use edgeagent_contracts::{
    research::ResearchRecord,
    research_experiment::{Action, Command, InvalidExperiment, Terminal},
};

/// Validated registration, terminal attempts and optional selection in commit order.
#[derive(Default)]
pub struct Ledger {
    entries: Vec<Command>,
}
impl Ledger {
    /// Retained facts in append order.
    pub fn entries(&self) -> &[Command] {
        &self.entries
    }

    /// Append one validated fact. Exact retries return false and retain the original order.
    pub fn apply(&mut self, command: &Command) -> Result<bool, InvalidExperiment> {
        command.bytes()?;
        let Command::V1 {
            scope,
            family,
            action,
        } = command;
        let Some(Command::V1 {
            scope: owner,
            family: id,
            action: Action::Register { trials, .. },
        }) = self.entries.first()
        else {
            if !matches!(action, Action::Register { .. }) {
                return Err(InvalidExperiment);
            }
            self.entries.push(command.clone());
            return Ok(true);
        };
        if scope != owner || family != id || matches!(action, Action::Read) {
            return Err(InvalidExperiment);
        }
        for previous in &self.entries {
            let Command::V1 {
                action: retained, ..
            } = previous;
            if same_slot(action, retained) {
                return if command == previous {
                    Ok(false)
                } else {
                    Err(InvalidExperiment)
                };
            }
            if matches!(retained, Action::Select { .. }) {
                return Err(InvalidExperiment);
            }
        }
        match action {
            Action::Finish { trial, outcome } => {
                let declared = trials.iter().find(|v| v.request.request_id == *trial).ok_or(InvalidExperiment)?;
                if let Terminal::Result { result } = outcome {
                    match result.record() {
                        ResearchRecord::Artifact(a) if a.request == declared.request && a.evidence == declared.evidence => {}
                        ResearchRecord::Abstention(a) if a.request == declared.request => {}
                        _ => return Err(InvalidExperiment),
                    }
                }
            }
            Action::Select { trial } => {
                if self.entries.len() != trials.len() + 1 || !self.entries.iter().any(|entry| {
                    let Command::V1 { action, .. } = entry;
                    matches!(action, Action::Finish { trial: id, outcome: Terminal::Result { result } }
                        if id == trial && matches!(result.record(), ResearchRecord::Artifact(_)))
                }) {
                    return Err(InvalidExperiment);
                }
            }
            Action::Register { .. } | Action::Read => return Err(InvalidExperiment),
        }
        self.entries.push(command.clone());
        Ok(true)
    }
}

fn same_slot(a: &Action, b: &Action) -> bool {
    match (a, b) {
        (Action::Register { .. }, Action::Register { .. })
        | (Action::Select { .. }, Action::Select { .. }) => true,
        (Action::Finish { trial: a, .. }, Action::Finish { trial: b, .. }) => a == b,
        _ => false,
    }
}

/// Check the exact selected artifact and the declared chronology against a retained family.
pub fn validate_promotion(
    ledger: &Ledger,
    evidence: &edgeagent_contracts::research_promotion::Promotion,
) -> Result<(), edgeagent_contracts::research_promotion::InvalidPromotion> {
    use edgeagent_contracts::research_promotion::InvalidPromotion;
    evidence.bytes()?;
    let entries = ledger.entries();
    let Some(Command::V1 {
        scope,
        family,
        action: Action::Register { trials, .. },
    }) = entries.first()
    else {
        return Err(InvalidPromotion);
    };
    let Some(Command::V1 {
        action: Action::Select { trial },
        ..
    }) = entries.last()
    else {
        return Err(InvalidPromotion);
    };
    if scope != &evidence.profile.scope
        || family != &evidence.family
        || trial != &evidence.trial
        || trials
            .iter()
            .any(|t| t.request.decision_at_ms >= evidence.holdout_start_ms)
    {
        return Err(InvalidPromotion);
    }
    for Command::V1 { action, .. } in entries {
        if let Action::Finish {
            trial: id,
            outcome: Terminal::Result { result },
        } = action
            && id == trial
            && let ResearchRecord::Artifact(artifact) = result.record()
        {
            let request = &artifact.request;
            if request.implementation == evidence.profile.implementation
                && request.configuration == evidence.profile.configuration
                && request.lookback_sessions == evidence.profile.lookback_sessions
                && result.record().digest().map_err(|_| InvalidPromotion)? == evidence.result
            {
                return Ok(());
            }
        }
    }
    Err(InvalidPromotion)
}
