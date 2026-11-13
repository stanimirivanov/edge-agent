//! Explanation execution composes the sealed validator and the bounded model port.
use edgeagent_contracts::{
    explanation_profile::{Deployment, Profile, Settings, Tools},
    research::ResearchRecord,
    research_decision::DecisionProvenance,
};
use edgeagent_research_application::{
    explanation_model::{ExplanationModel, Failure, Reply, Request},
    explanation_worker::Prepared,
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Model(Option<Failure>, bool);
impl ExplanationModel for Model {
    async fn propose(&self, request: &Request<'_>) -> std::result::Result<Reply, Failure> {
        if let Some(failure) = self.0 {
            return Err(failure);
        }
        let mut input: serde_json::Value =
            serde_json::from_slice(request.input()).map_err(|_| Failure::InvalidRequest)?;
        if self.1 {
            input["standard_proposal"]["data"]["claims"][0]["citation"] = "f".repeat(64).into();
        }
        let bytes = format!(
            "{}\n",
            serde_json::to_string(&input["standard_proposal"])
                .map_err(|_| Failure::InvalidRequest)?
        )
        .into_bytes();
        request.accept(request.profile().deployment.clone(), bytes)
    }
}
fn profile() -> Profile {
    Profile {
        deployment: Deployment {
            runtime: "a".repeat(64),
            weights: "b".repeat(64),
        },
        settings: Settings {
            seed: 1,
            temperature_milli: 0,
            max_output_tokens: 512,
            timeout_ms: 1000,
        },
        prompt: "c".repeat(64),
        validator: edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(
            b"research-explanation-allowlist-v1",
        )
        .as_str()
        .into(),
        renderer: edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(
            b"research-explanation-trace.v1",
        )
        .as_str()
        .into(),
        tools: Tools::None,
    }
}
#[tokio::test]
async fn model_failures_render_the_exact_baseline_and_leave_original_bytes_unchanged() -> Result {
    let record = ResearchRecord::decode(include_bytes!(
        "../../../crates/contracts/fixtures/research/v1/artifact.json"
    ))?;
    let decision = DecisionProvenance::decode(include_bytes!(
        "../../../crates/contracts/fixtures/research/decision-v1.json"
    ))?;
    let before = (record.canonical_bytes()?, decision.canonical_bytes()?);
    let profile = profile();
    let prepared = Prepared::new(&record, &decision, &profile)?;
    let mut changed = profile.clone();
    changed.validator = "f".repeat(64);
    assert!(Prepared::new(&record, &decision, &changed).is_err());
    let expected = serde_json::to_vec(
        &serde_json::json!({"artifact":serde_json::from_slice::<serde_json::Value>(&record.canonical_bytes()?)?,"decision":serde_json::from_slice::<serde_json::Value>(&decision.canonical_bytes()?)?,"standard_proposal":serde_json::from_slice::<serde_json::Value>(&prepared.disabled()?.explanation.proposal().canonical_bytes()?)?,"untrusted_context":null}),
    )?;
    assert_eq!(prepared.input(), expected);
    let invalid = prepared.infer(&profile, &Model(None, true)).await?;
    assert!(matches!(
        invalid.reason,
        Some(edgeagent_contracts::explanation::Fallback::InvalidProposal)
    ));
    let baseline = prepared.disabled()?.explanation.rendered().to_owned();
    let valid = prepared.infer(&profile, &Model(None, false)).await?;
    assert!(valid.reason.is_none());
    assert_eq!(valid.explanation.rendered(), baseline);
    for failure in [
        Failure::Unavailable,
        Failure::Timeout,
        Failure::BudgetExhausted,
        Failure::InvalidResponse,
    ] {
        let result = prepared
            .infer(&profile, &Model(Some(failure), false))
            .await?;
        assert!(result.reason.is_some());
        assert_eq!(result.explanation.rendered(), baseline);
    }
    assert_eq!(
        before,
        (record.canonical_bytes()?, decision.canonical_bytes()?)
    );
    Ok(())
}

#[path = "support/database.rs"]
mod database;
#[path = "process_telemetry/explanation.rs"]
mod explanation_telemetry;
#[allow(dead_code)]
mod support;

async fn process(
    profile: &support::Profile,
    schema: &str,
    mode: &str,
    input: Option<&[u8]>,
    job: Option<&str>,
) -> Result<std::process::Output> {
    use tokio::io::AsyncWriteExt as _;
    let mut command = profile.command(schema, mode);
    command
        .env(
            "EDGEAGENT_RESEARCH_EXPLANATION_PROFILE",
            profile.root.join("explanation.json"),
        )
        .env(
            "EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE",
            "00000000000000000000000000000002",
        );
    if let Some(job) = job {
        command.env("EDGEAGENT_RESEARCH_EXPLANATION_JOB", job);
    }
    if mode == "offline-explanation-worker" {
        if let Ok(endpoint) = std::fs::read_to_string(profile.root.join("otlp-endpoint")) {
            command.env("EDGEAGENT_RESEARCH_OTLP_ENDPOINT", endpoint);
        }
        command
            .env_remove("EDGEAGENT_RESEARCH_OWNER_ACQUISITION")
            .env_remove("EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION");
    }
    let mut child = tokio::process::Command::from(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    if let Some(input) = input {
        stdin.write_all(input).await?;
    }
    drop(stdin);
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(45), child.wait_with_output())
            .await??,
    )
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
#[allow(
    clippy::disallowed_methods,
    reason = "Isolated schema inspection and commit fault injection."
)]
async fn durable_worker_recovers_before_owner_reads_and_commits_one_authorized_terminal() -> Result
{
    use edgeagent_contracts::{
        explanation::{Companion, Fallback, Outcome, ServingPermit},
        research_acquisition::ResearchEvidence,
    };
    use edgeagent_research_application::{
        explanation_context::{Grant, Purpose, Request},
        publication::decode_decision,
    };
    use edgeagent_research_postgres::{
        PostgresResearchStore,
        explanation_attempts::{Intent, ReservationOutcome},
    };
    use futures_util::FutureExt as _;
    let db = database::Database::new().await?;
    let scenario = async {
        support::approve(&db.schema).await?;
        let store = PostgresResearchStore::new(db.pool.clone());
        for label in [
            "success",
            "denied",
            "unavailable",
            "uncertain",
            "withdrawn",
            "revoked",
            "bad-permit",
        ] {
            let (event, bytes) = support::governed_event(label)?;
            let owner = support::Profile::new(&bytes)?;
            let mode = match label {
                "success" => 2,
                "denied" => 1,
                _ => 0,
            };
            let mut observer = explanation_telemetry::Observer::start(&owner.root, mode).await?;
            let initial = process(
                &owner,
                &db.schema,
                "offline-once",
                Some(&event.to_json()?),
                None,
            )
            .await?;
            assert!(
                initial.status.success(),
                "{}",
                String::from_utf8_lossy(&initial.stderr)
            );
            let publication: Vec<u8> = sqlx::query_scalar(
                "SELECT publication FROM edgeagent_research_completions WHERE request_id=$1",
            )
            .bind(label)
            .fetch_one(&db.pool)
            .await?;
            let publication = edgeagent_contracts::MessageEnvelope::from_json(&publication)?;
            store
                .admit_explanation_job("operator", &publication)
                .await?;
            let job = store
                .explanation_job("operator", None)
                .await?
                .ok_or("job")?;
            let id = job.job.id()?;
            let original = (
                job.original.receipt.bytes().to_vec(),
                job.original
                    .receipt
                    .decision()
                    .ok_or("decision")?
                    .to_json()?,
            );
            let config = profile();
            std::fs::write(
                owner.root.join("explanation.json"),
                config.canonical_bytes()?,
            )?;
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
            )?;
            let serving = Grant {
                request: Request {
                    anchor: job.job.anchor.clone(),
                    audience: "00000000000000000000000000000002".into(),
                    purpose: Purpose::ExplanationServing,
                    document: None,
                },
                valid_from_ms: now - 120000,
                expires_at_ms: now + 120000,
                revoked: false,
            };
            store.set_explanation_grant(&serving, 1).await?;
            let ResearchEvidence::Ready { request, decision } =
                ResearchEvidence::from_envelope(&event)?
            else {
                return Err("ready".into());
            };
            let edgeagent_contracts::evidence::EvidenceOutcome::Ready { object } =
                &decision.outcome
            else {
                return Err("object".into());
            };
            let mut permit = ServingPermit {
                scope: request.scope,
                acquisition: decision.request.request.as_str().into(),
                evidence: object.sha256.as_str().into(),
                audience: serving.request.audience.clone(),
                checked_at_ms: now,
                expires_at_ms: now + 120000,
            };
            if label == "bad-permit" {
                permit.expires_at_ms = i64::MAX;
            }
            std::fs::write(owner.root.join("permit.json"), serde_json::to_vec(&permit)?)?;
            let original_owner = std::fs::read_to_string(&owner.owner)?;
            let exit = match label {
                "denied" => "exit 77".to_owned(),
                "unavailable" => "exit 75".to_owned(),
                "revoked" => format!(
                    "if [ -f '{}' ]; then exit 77; fi; touch '{}'; cat '{}'",
                    owner.root.join("checked").display(),
                    owner.root.join("checked").display(),
                    owner.root.join("permit.json").display()
                ),
                _ => format!("cat '{}'", owner.root.join("permit.json").display()),
            };
            std::fs::write(
                &owner.owner,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = offline-explanation-permit ]; then cat >/dev/null; {exit}; exit; fi\n{}",
                    original_owner.trim_start_matches("#!/bin/sh\n")
                ),
            )?;
            if label == "withdrawn" {
                store
                    .append_lifecycle(
                        "operator",
                        edgeagent_contracts::research::ResearchLifecycle {
                            scope: "operator".into(),
                            request_id: label.into(),
                            artifact: job.job.anchor.artifact.clone(),
                            revision: 1,
                            effective_at_ms: now,
                            change: edgeagent_contracts::research::LifecycleChange::Withdrawn,
                        },
                        &chrono::DateTime::from_timestamp_millis(now)
                            .ok_or("time")?
                            .to_rfc3339(),
                    )
                    .await?;
            }
            if label == "uncertain" {
                let provenance =
                    decode_decision(job.original.receipt.decision().ok_or("decision")?)?;
                let prepared = Prepared::new(job.original.receipt.record(), &provenance, &config)?;
                let model = Grant {
                    request: Request {
                        purpose: Purpose::ModelDisclosure,
                        ..serving.request.clone()
                    },
                    ..serving.clone()
                };
                store.set_explanation_grant(&model, 1).await?;
                let intent = Intent {
                    binding: prepared.binding().clone(),
                    input_digest: prepared.input_digest(),
                    audience: model.request.audience,
                    deadline_ms: now - 1000,
                };
                assert!(matches!(
                    store
                        .reserve_explanation_attempt(&intent, || now - 2000)
                        .await?,
                    ReservationOutcome::Reserved(_)
                ));
            }
            if label == "success" {
                sqlx::raw_sql("CREATE FUNCTION fail_worker_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'commit fault'; END $$; CREATE CONSTRAINT TRIGGER fail_worker_commit AFTER INSERT ON edgeagent_explanation_completions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_worker_commit();").execute(&db.pool).await?;
                assert!(
                    !process(
                        &owner,
                        &db.schema,
                        "offline-explanation-worker",
                        None,
                        Some(&id)
                    )
                    .await?
                    .status
                    .success()
                );
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT count(*) FROM edgeagent_explanation_completions"
                    )
                    .fetch_one(&db.pool)
                    .await?,
                    0
                );
                sqlx::raw_sql(
                    "DROP TRIGGER fail_worker_commit ON edgeagent_explanation_completions",
                )
                .execute(&db.pool)
                .await?;
            }
            let run = || {
                process(
                    &owner,
                    &db.schema,
                    "offline-explanation-worker",
                    None,
                    Some(&id),
                )
            };
            let (first, duplicate) = if label == "revoked" {
                let first = run().await?;
                (first, run().await?)
            } else {
                let (first, duplicate) = tokio::join!(run(), run());
                (first?, duplicate?)
            };
            assert!(
                first.status.success(),
                "label={label}: {}",
                String::from_utf8_lossy(&first.stderr)
            );
            assert!(
                duplicate.status.success(),
                "label={label}: {}",
                String::from_utf8_lossy(&duplicate.stderr)
            );
            assert_eq!(first.stdout, duplicate.stdout);
            let companion = Companion::decode(&first.stdout)?;
            assert!(matches!(
                (label, &companion.outcome),
                (
                    "success",
                    Outcome::Fallback {
                        reason: Fallback::Disabled,
                        ..
                    }
                ) | (
                    "uncertain",
                    Outcome::Fallback {
                        reason: Fallback::UncertainAttempt,
                        ..
                    }
                ) | (
                    "denied" | "unavailable" | "withdrawn" | "revoked" | "bad-permit",
                    Outcome::Refused { .. }
                )
            ));
            let record = store
                .explanation_job("operator", Some(&id))
                .await?
                .ok_or("recovery")?;
            assert_eq!(
                original,
                (
                    record.original.receipt.bytes().to_vec(),
                    record
                        .original
                        .receipt
                        .decision()
                        .ok_or("decision")?
                        .to_json()?
                )
            );
            std::fs::remove_file(&owner.owner)?;
            std::fs::remove_file(owner.root.join("explanation.json"))?;
            let recovered = process(
                &owner,
                &db.schema,
                "offline-explanation-worker",
                None,
                Some(&id),
            )
            .await?;
            assert!(recovered.status.success());
            assert_eq!(recovered.stdout, first.stdout);
            let owned_recovery = process(
                &owner,
                &db.schema,
                "offline-explanation-worker-owned",
                None,
                Some(&id),
            )
            .await?;
            assert!(owned_recovery.status.success());
            assert_eq!(owned_recovery.stdout, first.stdout);
            let outcome = match label {
                "success" => "deterministic",
                "uncertain" => "uncertain_attempt",
                "denied" | "withdrawn" | "revoked" => "disclosure_denied",
                _ => "authority_unavailable",
            };
            observer.assert_outcomes(&[outcome, "recovered"], mode == 0)?;
            observer.close().await?;
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_completions")
                .fetch_one(&db.pool)
                .await?,
            7
        );
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_message_outbox WHERE message_type='com.edgeagent.research.explanation-recorded.v1'").fetch_one(&db.pool).await?,2);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_attempts")
                .fetch_one(&db.pool)
                .await?,
            1
        );
        Ok(())
    };
    let result = std::panic::AssertUnwindSafe(scenario).catch_unwind().await;
    db.finish(result).await
}
