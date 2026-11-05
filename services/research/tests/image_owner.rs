//! Exercise the released Research filesystem against actual owner evidence.
#![allow(clippy::disallowed_methods, reason = "Owned qualification schemas.")]
#[allow(dead_code)]
mod process_support;
use edgeagent_contracts::research::ResearchRecord;
use futures_util::FutureExt;
use process_support::*;
use std::{panic::AssertUnwindSafe, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt as _;

async fn image(
    h: &Journey,
    input: &[u8],
    owner: &str,
    acquisition: &str,
) -> Result<std::process::Output> {
    let uid = std::env::var("EDGEAGENT_IMAGE_UID")?;
    if uid == "0" {
        return Err("image proof requires a non-root host identity".into());
    }
    let name = format!("edgeagent-owner-{}", h.research.name);
    let mut command = tokio::process::Command::new("docker");
    command
        .args([
            "run",
            "--rm",
            "--interactive",
            "--network=host",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true",
            "--user",
        ])
        .arg(format!("{uid}:{}", std::env::var("EDGEAGENT_IMAGE_GID")?))
        .args(["--name", &name]);
    // Mount only invented owner state and canonical configuration, never executables.
    for (path, options) in [
        (&h.root, ""),
        (&h.root.with_extension("execution.json"), ",readonly"),
    ] {
        command.arg("--mount").arg(format!(
            "type=bind,src={},dst={}{options}",
            path.display(),
            path.display()
        ));
    }
    let configured = h.command("research", "offline-once")?;
    for (key, value) in configured.as_std().get_envs() {
        if let Some(value) = value {
            command.arg("--env").arg(key).env(key, value);
        }
    }
    command
        .env("EDGEAGENT_RESEARCH_OWNER_BINARY", owner)
        .env("EDGEAGENT_RESEARCH_OWNER_ACQUISITION", acquisition)
        .arg(std::env::var("EDGEAGENT_RESEARCH_IMAGE")?)
        .arg("offline-once")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let result = tokio::time::timeout(Duration::from_secs(45), async move {
        let written = child.stdin.take().ok_or("stdin")?.write_all(input).await;
        let output = child.wait_with_output().await?;
        if let Err(error) = written
            && (output.status.success() || error.kind() != std::io::ErrorKind::BrokenPipe)
        {
            return Err(error.into());
        }
        Result::Ok(output)
    })
    .await;
    // The bounded service normally exits first. A killed Docker CLI must not
    // leave its separately managed container alive.
    let _ = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("docker")
            .args(["rm", "-f", &name])
            .output(),
    )
    .await;
    result?
}

#[tokio::test]
#[ignore = "requires built Research image and isolated PostgreSQL/NATS through image-smoke"]
async fn packaged_owner_read_completes_and_foreign_evidence_fails_closed() -> Result {
    for foreign in [false, true] {
        let mut h = Journey::new().await?;
        let tested = AssertUnwindSafe(async {
            h.seed_history().await?;
            h.start_servers().await?;
            let request = h.intent(5)?;
            assert_eq!(h.request("POST", "/v1/research-requests", &serde_json::to_vec(&request)?).await?.0, 202);
            h.gateway_relay().await?;
            h.consume_market().await?;
            let stream = h.context.get_stream(&h.stream).await?;
            let input = stream.get_last_raw_message_by_subject(
                &edgeagent_contracts::research_acquisition::DECIDED.subject()?
            ).await?.payload;
            let missing = image(&h, &input, "/missing-owner", &format!("{:032x}", 1)).await?;
            assert!(!missing.status.success());
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_completions")
                .fetch_one(&h.research.pool).await?, 0);
            let acquisition = format!("{:032x}", if foreign { 99 } else { 1 });
            let output = image(&h, &input, "/edgeagent-owner/edgeagent-market-data", &acquisition).await?;
            if foreign {
                // Owner denial may be a canonical abstention; it must never produce an artifact.
                if output.status.success() {
                    assert!(matches!(ResearchRecord::decode(&output.stdout)?, ResearchRecord::Abstention(_)));
                }
                assert!(!sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM edgeagent_research_completions WHERE convert_from(outcome,'UTF8') LIKE '%research.artifact.v1%')"
                ).fetch_one(&h.research.pool).await?);
            } else {
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                let ResearchRecord::Artifact(artifact) = ResearchRecord::decode(&output.stdout)? else {
                    return Err("packaged owner must produce an artifact".into());
                };
                assert_eq!(artifact.ranking[0].score_ppm, 40502);
                let retained: Vec<u8> = sqlx::query_scalar("SELECT outcome FROM edgeagent_research_completions")
                    .fetch_one(&h.research.pool).await?;
                assert_eq!(retained, output.stdout);
                let retry = image(&h, &input, "/missing-owner", &acquisition).await?;
                assert!(retry.status.success());
                assert_eq!(retry.stdout, retained);
                assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_completions")
                    .fetch_one(&h.research.pool).await?, 1);
                assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_outbox")
                    .fetch_one(&h.research.pool).await?, 2);
            }
            Ok(())
        }).catch_unwind().await;
        h.finish(tested).await?;
    }
    Ok(())
}
