//! Invented capacity calibration, never a qualification collector or worker.
use edgeagent_contracts::explanation::Binding;
use edgeagent_research::{
    explanation_model::HttpModel,
    explanation_owned::{Manifest, digest},
};
use edgeagent_research_application::{
    explanation::Allowlist,
    explanation_evaluation::{DevelopmentLayout, Suite},
    explanation_model::Request,
    explanation_runtime::{RENDERER_REVISION, VALIDATOR_REVISION},
};
use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn calibration(mode: &str) -> Result<(usize, &'static str, Duration)> {
    let (index, case, seconds) = match mode {
        "performance-P01" => (1, "P01", 180),
        "performance-P02" => (2, "P02", 180),
        "performance-P03" => (3, "P03", 180),
        "performance-P03-extended" => (3, "P03", 600),
        _ => return Err("invalid calibration".into()),
    };
    Ok((index, case, Duration::from_secs(seconds)))
}

fn p03_tokenizer_reference() -> Result<Vec<u8>> {
    let input = Suite::performance_input(3)?;
    let bytes = input.standard_proposal();
    input.render_candidate(bytes)?;
    Ok(bytes.to_vec())
}

const MAX_DEVELOPMENT_CHOICE_BYTES: usize = 1024;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Layout {
    Standard,
    ReaderFirst,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Choice {
    #[serde(rename = "research.explanation-development-choice.v1")]
    V1 {
        case: String,
        input: String,
        layout: Layout,
    },
}

fn select_development(expected_case: &str, response: &[u8]) -> Result<serde_json::Value> {
    if response.len() > MAX_DEVELOPMENT_CHOICE_BYTES {
        return Err("development choice exceeds byte limit".into());
    }
    let Choice::V1 {
        case,
        input,
        layout,
    } = serde_json::from_slice(response)?;
    let layout = match layout {
        Layout::Standard => DevelopmentLayout::Standard,
        Layout::ReaderFirst => DevelopmentLayout::ReaderFirst,
    };
    Ok(Suite::render_development_choice(
        expected_case,
        &case,
        &input,
        layout,
    )?)
}

async fn run() -> Result<()> {
    let mode = std::env::args().nth(1).ok_or("case required")?;
    if mode == "export-p03-tokenizer-reference" {
        std::io::stdout().write_all(&p03_tokenizer_reference()?)?;
        return Ok(());
    }
    let selection = match mode.as_str() {
        "compare-development-presentations" => Some(Suite::development_presentation_comparison()?),
        "select-development-presentation" => {
            let case = std::env::args().nth(2).ok_or("development case required")?;
            let mut response = Vec::new();
            std::io::stdin()
                .take((MAX_DEVELOPMENT_CHOICE_BYTES + 1) as u64)
                .read_to_end(&mut response)?;
            Some(select_development(&case, &response)?)
        }
        _ => None,
    };
    if let Some(report) = selection {
        let mut bytes = edgeagent_contracts::canonical_json_bytes(&report)?;
        bytes.push(b'\n');
        std::io::stdout().write_all(&bytes)?;
        return Ok(());
    }
    let (index, id, observation_limit) = calibration(&mode)?;
    let (manifest, _) = Manifest::environment()?;
    let profile = manifest.profile()?;
    let prompt = manifest.verify(&profile)?;
    if profile.renderer != digest(RENDERER_REVISION.as_bytes())
        || profile.validator != digest(VALIDATOR_REVISION.as_bytes())
    {
        return Err("runtime protocol required".into());
    }
    let observation = manifest.observation()?;
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, observation.port);
    let verifier = manifest.verifier(observation, profile.deployment.clone())?;
    let model = HttpModel::new(endpoint, verifier.clone(), prompt)?;
    let started = Instant::now();
    let input = Suite::performance_input(index)?;
    let bytes = input.prepared_input().ok_or("preparation required")?;
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: digest(bytes),
    };
    let request = Request::new(&profile, &binding, bytes)?;
    let measurement = model.measure(&request, &input, observation_limit).await;
    verifier.verify()?;
    let report = serde_json::json!({"kind":"research.explanation-performance.attempt.v3","case":id,"profile":profile.id()?,"calibration":digest(bytes),"input_bytes":bytes.len(),"total_ms":started.elapsed().as_millis(),"measurement":measurement});
    std::io::stdout().write_all(&edgeagent_contracts::canonical_json_bytes(&report)?)?;
    Ok(())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("performance probe refused");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_candidate_selects_only_a_layout_bound_to_the_requested_fixture() -> Result<()> {
        use serde_json::json;

        let report = Suite::development_presentation_comparison()?;
        let cases = report["cases"].as_array().ok_or("cases")?;
        for row in cases {
            let case = row["id"].as_str().ok_or("id")?;
            let mut response = json!({
                "kind": "research.explanation-development-choice.v1",
                "case": case,
                "input": row["choice_binding"],
                "layout": "reader_first"
            });
            let render = |value: &serde_json::Value| -> Result<serde_json::Value> {
                select_development(case, &serde_json::to_vec(value)?)
            };
            let selected: serde_json::Value = render(&response)?;
            assert_eq!(selected["presentation"], row["reader_first"]);
            assert_eq!(selected["layout"], "reader_first");
            response["layout"] = "standard".into();
            let selected: serde_json::Value = render(&response)?;
            assert_eq!(selected["presentation"], row["standard"]);
            assert_eq!(selected["input"], row["choice_binding"]);

            for (field, value) in [
                ("layout", json!("plain_language")),
                ("case", json!("U01")),
                (
                    "input",
                    cases[if case == "D01" { 1 } else { 0 }]["choice_binding"].clone(),
                ),
                ("claims", json!([])),
                ("kind", json!("research.explanation-development-choice.v2")),
            ] {
                let mut foreign = response.clone();
                foreign[field] = value;
                assert!(render(&foreign).is_err(), "{field}");
            }

            let mut oversized = serde_json::to_vec(&response)?;
            oversized.resize(1025, b' ');
            assert!(select_development(case, &oversized).is_err());
            let duplicate = serde_json::to_string(&response)?
                .replace("\"layout\":", "\"layout\":\"reader_first\",\"layout\":");
            assert!(select_development(case, duplicate.as_bytes()).is_err());
        }
        Ok(())
    }

    #[test]
    fn reader_first_places_verified_policy_and_sources_before_unchanged_details() -> Result<()> {
        let report = Suite::development_presentation_comparison()?;
        let rows = report["cases"].as_array().ok_or("cases")?;
        for row in rows {
            let reader_first = row["reader_first"].as_str().ok_or("reader-first layout")?;
            let (summary, details) = reader_first
                .split_once("\nFull validated presentation\n")
                .ok_or("missing detail boundary")?;
            assert!(summary.starts_with("research-explanation-reader-first.v1\n"));
            assert_eq!(details, row["standard"].as_str().ok_or("standard")?);
        }
        let exclusion = rows[1]["reader_first"].as_str().ok_or("D02")?;
        let summary = exclusion
            .split_once("\nFull validated presentation\n")
            .ok_or("details")?
            .0;
        assert!(summary.contains(&format!(
            "Instrument {:032x} is below the liquidity floor: 10025 USD cents observed; inclusive minimum 500000 USD cents.\nEvidence: {}",
            2, "06".repeat(32)
        )));
        assert_eq!(summary.matches("is below the liquidity floor").count(), 1);
        let source = rows[2]["reader_first"].as_str().ok_or("D03")?;
        let summary = source
            .split_once("\nFull validated presentation\n")
            .ok_or("details")?
            .0;
        assert!(summary.contains(&format!(
            "Leading instrument {:032x}, latest selected close\nEvidence: {}\nEvent time: 91 ms. Observation time: 95 ms. Availability time: 100 ms.",
            1, "05".repeat(32)
        )));
        Ok(())
    }

    #[test]
    fn development_comparison_is_separate_and_exposes_the_actual_render_delta() -> Result<()> {
        let report = Suite::development_presentation_comparison()?;
        assert_eq!(
            report["kind"],
            "research.explanation-development-comparison.v1"
        );
        let rows = report["cases"].as_array().ok_or("cases")?;
        assert_eq!(rows.len(), 3);
        for (row, id) in rows.iter().zip(["D01", "D02", "D03"]) {
            assert_eq!(row["id"], id);
            assert_eq!(row["scope"], "presentation-development");
            assert_eq!(row["changed_rank_lines"], 2);
            assert_eq!(row["changed_other_lines"], 0);
            let standard = row["standard"].as_str().ok_or("standard")?;
            let plain = row["plain_language"].as_str().ok_or("plain language")?;
            assert!(plain.contains("parts per million"));
            assert_ne!(standard, plain);
            for (before, after) in standard.lines().zip(plain.lines()) {
                if let Some(rank) = before.strip_prefix("rank=") {
                    let (ordinal, rest) = rank.split_once(" instrument=").ok_or("instrument")?;
                    let (instrument, rest) = rest.split_once(" score_ppm=").ok_or("score")?;
                    let (score, citations) = rest.split_once(" evidence=").ok_or("evidence")?;
                    assert_eq!(
                        after,
                        format!(
                            "Rank {ordinal}: {instrument} scored {score} parts per million. evidence={citations}"
                        )
                    );
                } else {
                    assert_eq!(before, after);
                }
            }
            assert!(
                plain.contains("Research only. This explanation confers no trading authority.")
            );
        }
        let exclusion = rows[1]["plain_language"].as_str().ok_or("exclusion")?;
        assert!(exclusion.contains("policy_reason=liquidity_below_minimum"));
        assert!(exclusion.contains("minimum_inclusive=500000 below_minimum=true"));
        let chronology = rows[2]["plain_language"].as_str().ok_or("chronology")?;
        assert!(chronology.contains("event_at_ms=91 observed_at_ms=95 available_at_ms=100"));
        Ok(())
    }

    #[test]
    fn tokenizer_reference_is_the_validated_p03_proposal() -> Result<()> {
        let bytes = p03_tokenizer_reference()?;
        let input = Suite::performance_input(3)?;
        assert_eq!(bytes, input.standard_proposal());
        assert!(!input.render_candidate(&bytes)?.is_empty());
        assert!(edgeagent_contracts::explanation::Proposal::decode(&bytes).is_ok());
        Ok(())
    }

    #[test]
    fn only_extended_p03_gets_the_longer_observation_window() -> Result<()> {
        assert_eq!(
            calibration("performance-P03-extended")?,
            (3, "P03", Duration::from_secs(600))
        );
        for (index, case) in [(1, "P01"), (2, "P02"), (3, "P03")] {
            assert_eq!(
                calibration(&format!("performance-{case}"))?,
                (index, case, Duration::from_secs(180))
            );
        }
        assert!(calibration("performance-P01-extended").is_err());
        Ok(())
    }
}
