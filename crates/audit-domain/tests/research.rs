//! Pure Research replay/corruption proofs; no broker, persistence or clock.
use edgeagent_audit_domain::{
    ReplayError as Error, ReplayOutcome,
    research::{MAX_CHECKPOINT_BYTES, ResearchProjection, ResearchStatus},
};
use edgeagent_contracts::research::{
    AbstentionReason, LifecycleChange, ResearchLifecycle, ResearchRecord, ResearchRequest,
};
use sha2::{Digest, Sha256};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn artifact() -> Result<ResearchRecord> {
    Ok(ResearchRecord::decode(include_bytes!(
        "../../contracts/fixtures/research/v1/artifact.json"
    ))?)
}
fn request(record: &ResearchRecord) -> Result<ResearchRequest> {
    match record {
        ResearchRecord::Artifact(a) => Ok(a.request.clone()),
        ResearchRecord::Abstention(a) => Ok(a.request.clone()),
        _ => Err("terminal fixture required".into()),
    }
}
fn lifecycle(record: &ResearchRecord, revision: u64) -> Result<ResearchRecord> {
    let request = request(record)?;
    Ok(ResearchRecord::Lifecycle(ResearchLifecycle {
        scope: request.scope,
        request_id: request.request_id,
        artifact: record.digest()?,
        revision,
        effective_at_ms: 1000,
        change: LifecycleChange::Expired,
    }))
}
#[test]
fn pending_terminal_replay_and_checkpoint_resume_preserve_exact_owner_bytes() -> Result {
    for record in [
        artifact()?,
        ResearchRecord::decode(include_bytes!(
            "../../contracts/fixtures/research/v1/abstention.json"
        ))?,
    ] {
        let req = request(&record)?;
        let mut view = ResearchProjection::new(req.clone())?;
        assert_eq!(view.status(), ResearchStatus::Pending);
        assert_eq!(view.revision(), 0);
        assert!(view.outcome_bytes().is_none());
        let initial = view.checkpoint()?;
        assert_eq!(
            ResearchProjection::restore(req.clone(), &initial)?.checkpoint()?,
            initial
        );
        assert_eq!(
            view.apply(ResearchRecord::Request(req.clone()))?,
            ReplayOutcome::Duplicate
        );
        assert_eq!(view.apply(record.clone())?, ReplayOutcome::Applied);
        assert_eq!(view.revision(), 1);
        assert_eq!(
            view.status(),
            if matches!(record, ResearchRecord::Artifact(_)) {
                ResearchStatus::Artifact
            } else {
                ResearchStatus::Abstained
            }
        );
        assert_eq!(
            view.outcome_bytes(),
            Some(record.canonical_bytes()?.as_slice())
        );
        let committed = view.checkpoint()?;
        let mut resumed = ResearchProjection::restore(req.clone(), &committed)?;
        assert_eq!(resumed.apply(record.clone())?, ReplayOutcome::Duplicate);
        assert_eq!(resumed.checkpoint()?, committed);
        let mut rebuilt = ResearchProjection::new(req)?;
        rebuilt.apply(record)?;
        assert_eq!(rebuilt.checkpoint()?, committed);
    }
    Ok(())
}
#[test]
fn conflicting_terminal_intent_foreign_scope_and_invalid_records_never_mutate() -> Result {
    let record = artifact()?;
    let req = request(&record)?;
    let mut view = ResearchProjection::new(req.clone())?;
    view.apply(record.clone())?;
    let before = view.checkpoint()?;
    let mut changed = record.clone();
    if let ResearchRecord::Artifact(a) = &mut changed {
        a.expires_at_ms += 1;
    }
    assert_eq!(view.apply(changed), Err(Error::Conflict));
    let alternative =
        ResearchRecord::Abstention(edgeagent_contracts::research::ResearchAbstention {
            request: req.clone(),
            reason: AbstentionReason::InvalidEvidence,
        });
    assert_eq!(view.apply(alternative), Err(Error::Conflict));
    for field in 0..4 {
        let mut changed = req.clone();
        match field {
            0 => changed.scope = "another".into(),
            1 => changed.request_id = "another".into(),
            2 => changed.configuration = "09".repeat(32),
            _ => changed.lookback_sessions += 1,
        };
        assert_eq!(
            view.apply(ResearchRecord::Request(changed)),
            Err(if field < 2 {
                Error::Scope
            } else {
                Error::Conflict
            })
        );
    }
    let mut invalid = req;
    invalid.lookback_sessions = 0;
    assert_eq!(
        view.apply(ResearchRecord::Request(invalid)),
        Err(Error::Invalid)
    );
    assert_eq!(view.checkpoint()?, before);
    Ok(())
}
#[test]
fn reordered_lifecycle_records_remain_explicit_gaps_without_cursor_or_state_advance() -> Result {
    let record = artifact()?;
    let req = request(&record)?;
    let change = lifecycle(&record, 2)?;
    let mut view = ResearchProjection::new(req.clone())?;
    let pending = view.checkpoint()?;
    assert_eq!(view.apply(change.clone()), Err(Error::Gap));
    assert_eq!(view.checkpoint()?, pending);
    view.apply(record.clone())?;
    let terminal = view.checkpoint()?;
    assert_eq!(view.apply(change.clone()), Err(Error::Gap));
    let mut wrong = change;
    if let ResearchRecord::Lifecycle(c) = &mut wrong {
        c.artifact = "09".repeat(32);
    }
    assert_eq!(view.apply(wrong), Err(Error::Conflict));
    assert_eq!(view.checkpoint()?, terminal);
    let mut restored = ResearchProjection::restore(req, &terminal)?;
    assert_eq!(restored.apply(record)?, ReplayOutcome::Duplicate);
    assert_eq!(restored.checkpoint()?, terminal);
    Ok(())
}
fn checkpoint(value: &serde_json::Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.extend(Sha256::digest(&bytes));
    Ok(bytes)
}
#[test]
fn restore_rejects_corruption_wrong_anchor_versions_and_rechecks_sealed_outcomes() -> Result {
    let record = artifact()?;
    let req = request(&record)?;
    let mut view = ResearchProjection::new(req.clone())?;
    view.apply(record)?;
    let bytes = view.checkpoint()?;
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    assert!(matches!(
        ResearchProjection::restore(req.clone(), &corrupt),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        ResearchProjection::restore(req.clone(), &vec![0; MAX_CHECKPOINT_BYTES + 1]),
        Err(Error::Capacity)
    ));
    let mut foreign = req.clone();
    foreign.scope = "foreign".into();
    assert!(matches!(
        ResearchProjection::restore(foreign, &bytes),
        Err(Error::Scope)
    ));
    let value: serde_json::Value = serde_json::from_slice(&bytes[..bytes.len() - 32])?;
    for field in 0..4 {
        let mut changed = value.clone();
        match field {
            0 => changed["version"] = "EARP2".into(),
            1 => {
                changed["outcome"] =
                    String::from_utf8(ResearchRecord::Request(req.clone()).canonical_bytes()?)?
                        .into()
            }
            2 => changed["unknown"] = true.into(),
            _ => {
                let mut outcome: serde_json::Value =
                    serde_json::from_str(changed["outcome"].as_str().ok_or("outcome")?)?;
                outcome["data"]["request"]["configuration"] = "09".repeat(32).into();
                changed["outcome"] = (serde_json::to_string(&outcome)? + "\n").into();
            }
        }
        assert!(ResearchProjection::restore(req.clone(), &checkpoint(&changed)?).is_err());
    }
    // A recomputed checksum is no substitute for canonical checkpoint framing.
    let mut noncanonical = serde_json::to_vec_pretty(&value)?;
    noncanonical.extend(Sha256::digest(&noncanonical));
    assert!(ResearchProjection::restore(req, &noncanonical).is_err());
    Ok(())
}
#[test]
fn lifecycle_replay_duplicates_gaps_and_checkpoint_versions_preserve_original_bytes() -> Result {
    let record = artifact()?;
    let req = request(&record)?;
    let mut view = ResearchProjection::new(req.clone())?;
    view.apply(record.clone())?;
    let original = view.outcome_bytes().ok_or("outcome")?.to_vec();
    let mut first = lifecycle(&record, 1)?;
    if let ResearchRecord::Lifecycle(e) = &mut first {
        e.change = LifecycleChange::Invalidated;
    }
    let mut second = lifecycle(&record, 2)?;
    if let ResearchRecord::Lifecycle(e) = &mut second {
        e.change = LifecycleChange::Withdrawn;
    }
    assert_eq!(view.apply(second.clone()), Err(Error::Gap));
    assert_eq!(view.apply(first.clone())?, ReplayOutcome::Applied);
    let after_first = view.checkpoint()?;
    assert_eq!(view.status(), ResearchStatus::Invalidated);
    assert_eq!(view.apply(first.clone())?, ReplayOutcome::Duplicate);
    assert_eq!(view.checkpoint()?, after_first);
    view = ResearchProjection::restore(req.clone(), &after_first)?;
    assert_eq!(view.apply(second.clone())?, ReplayOutcome::Applied);
    assert_eq!(view.status(), ResearchStatus::Withdrawn);
    assert_eq!(view.revision(), 3);
    assert_eq!(view.outcome_bytes(), Some(original.as_slice()));
    let final_bytes = view.checkpoint()?;
    assert_eq!(view.apply(first)?, ReplayOutcome::Duplicate);
    assert_eq!(view.apply(second)?, ReplayOutcome::Duplicate);
    assert_eq!(view.apply(lifecycle(&record, 2)?), Err(Error::Conflict));
    assert_eq!(view.apply(lifecycle(&record, 3)?), Err(Error::Conflict));
    assert_eq!(view.checkpoint()?, final_bytes);
    assert_eq!(
        ResearchProjection::restore(req, &final_bytes)?.checkpoint()?,
        final_bytes
    );
    Ok(())
}

#[test]
fn decision_provenance_replays_with_gaps_conflicts_and_legacy_checkpoint_compatibility() -> Result {
    use edgeagent_contracts::research_decision::DecisionProvenance;
    let provenance = DecisionProvenance::decode(include_bytes!(
        "../../contracts/fixtures/research/decision-v1.json"
    ))?;
    let record = artifact()?;
    let req = request(&record)?;
    let mut view = ResearchProjection::new(req.clone())?;
    let pending = view.checkpoint()?;
    assert_eq!(view.apply_provenance(provenance.clone()), Err(Error::Gap));
    assert_eq!(view.checkpoint()?, pending);
    assert!(view.provenance_bytes().is_none());
    view.apply(record.clone())?;
    let legacy = view.checkpoint()?;
    assert!(
        ResearchProjection::restore(req.clone(), &legacy)?
            .provenance_bytes()
            .is_none()
    );
    let mut legacy_bytes = format!(
        "{{\"version\":\"EARP1\",\"request\":{},\"outcome\":{}}}",
        serde_json::to_string(&String::from_utf8(
            ResearchRecord::Request(req.clone()).canonical_bytes()?
        )?)?,
        serde_json::to_string(&String::from_utf8(record.canonical_bytes()?)?)?
    )
    .into_bytes();
    legacy_bytes.extend(Sha256::digest(&legacy_bytes));
    assert_eq!(legacy, legacy_bytes);
    let outcome = view.outcome_bytes().ok_or("outcome")?.to_vec();
    assert_eq!(
        view.apply_provenance(provenance.clone())?,
        ReplayOutcome::Applied
    );
    assert_eq!(view.status(), ResearchStatus::Artifact);
    assert_eq!(view.revision(), 2);
    assert_eq!(view.outcome_bytes(), Some(outcome.as_slice()));
    let expected = provenance.canonical_bytes()?;
    assert_eq!(view.provenance_bytes(), Some(expected.as_slice()));
    let retained = view.checkpoint()?;
    assert_eq!(
        view.apply_provenance(provenance.clone())?,
        ReplayOutcome::Duplicate
    );
    for field in 0..5 {
        let mut changed = provenance.clone();
        match field {
            0 => changed.request.scope = "foreign".into(),
            1 => changed.request.configuration = "09".repeat(32),
            2 => changed.outcome = "09".repeat(32),
            3 => changed.policy = "09".repeat(32),
            _ => changed.validator = "other-v1".into(),
        }
        assert_eq!(
            view.apply_provenance(changed),
            Err(if field == 0 {
                Error::Scope
            } else {
                Error::Conflict
            })
        );
        assert_eq!(view.checkpoint()?, retained);
    }
    let mut restored = ResearchProjection::restore(req.clone(), &retained)?;
    assert_eq!(restored.checkpoint()?, retained);
    assert_eq!(
        restored.apply_provenance(provenance.clone())?,
        ReplayOutcome::Duplicate
    );
    // Lifecycle and provenance can arrive in either order after their shared outcome.
    let change = lifecycle(&record, 1)?;
    restored.apply(change.clone())?;
    view.apply(change.clone())?;
    let mut reordered = ResearchProjection::new(req.clone())?;
    reordered.apply(record)?;
    reordered.apply(change)?;
    let old_lifecycle = reordered.checkpoint()?;
    assert!(
        ResearchProjection::restore(req.clone(), &old_lifecycle)?
            .provenance_bytes()
            .is_none()
    );
    reordered.apply_provenance(provenance)?;
    assert_eq!(view.checkpoint()?, reordered.checkpoint()?);
    assert_eq!(view.checkpoint()?, restored.checkpoint()?);
    assert_eq!(view.revision(), 3);
    assert_eq!(view.status(), ResearchStatus::Expired);
    let bytes = view.checkpoint()?;
    let abstention =
        ResearchRecord::Abstention(edgeagent_contracts::research::ResearchAbstention {
            request: req.clone(),
            reason: AbstentionReason::StaleEvidence,
        });
    let mut denied = DecisionProvenance::decode(include_bytes!(
        "../../contracts/fixtures/research/decision-v1.json"
    ))?;
    denied.outcome = abstention.digest()?;
    denied.assessment = edgeagent_contracts::research_decision::Assessment::NotEvaluated {
        reason: AbstentionReason::StaleEvidence,
    };
    let mut abstained = ResearchProjection::new(req.clone())?;
    abstained.apply(abstention)?;
    abstained.apply_provenance(denied)?;
    let resumed = ResearchProjection::restore(req.clone(), &abstained.checkpoint()?)?;
    assert_eq!(resumed.status(), ResearchStatus::Abstained);
    assert_eq!(resumed.provenance_bytes(), abstained.provenance_bytes());
    let original: serde_json::Value = serde_json::from_slice(&bytes[..bytes.len() - 32])?;
    for field in 0..4 {
        let mut value = original.clone();
        match field {
            0 => value["version"] = "EARP2".into(),
            1 => value["provenance"] = serde_json::Value::Null,
            2 => {
                let mut p: serde_json::Value =
                    serde_json::from_str(value["provenance"].as_str().ok_or("provenance")?)?;
                p["data"]["outcome"] = "09".repeat(32).into();
                value["provenance"] = (serde_json::to_string(&p)? + "\n").into();
            }
            _ => value["unknown"] = true.into(),
        }
        assert!(ResearchProjection::restore(req.clone(), &checkpoint(&value)?).is_err());
    }
    Ok(())
}
