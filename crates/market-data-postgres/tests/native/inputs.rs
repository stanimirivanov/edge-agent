//! Reuse the exact whole-series acceptance fixture; only its injected clock changes.
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
#[allow(dead_code)]
#[path = "../../../market-data-application/tests/support/mod.rs"]
mod support;
use support::*;
#[allow(dead_code)]
#[path = "../../../market-data-application/tests/native_acceptance/fixture.rs"]
mod fixture;
pub use fixture::NativeFixture;
