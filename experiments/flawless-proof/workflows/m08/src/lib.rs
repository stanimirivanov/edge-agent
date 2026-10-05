use flawless::{idempotent::Idempotence, workflow, workflow::Input};

flawless::module! { name = "edgeagent_m08", version = "0.0.1" }

#[workflow("m08")]
pub fn m08(input: Input<String>) {
    let base = input.as_str();

    // This is a recorded side-effect boundary, not a pure local computation.
    // Flawless re-executes deterministic local code during recovery.
    flawless_http::post(format!("{base}/a").as_str())
        .send()
        .expect("synthetic A effect");

    // The synthetic model endpoint is explicitly safe to retry.
    flawless_http::post(format!("{base}/b").as_str())
        .idempotent()
        .send()
        .expect("synthetic B effect");

    flawless_http::post(format!("{base}/c").as_str())
        .send()
        .expect("synthetic C effect");
}
