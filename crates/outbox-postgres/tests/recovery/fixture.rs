use super::*;
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[allow(
    clippy::panic,
    reason = "Fault injection proves awaited cleanup after a panicking case."
)]
fn fail<T>() -> T {
    panic!("synthetic fixture failure")
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn owned_schema_cleanup_runs_after_error_panic_and_timeout() -> TestResult {
    let returned = Fixture::new()?;
    let error = returned
        .run(Duration::from_secs(5), |fixture| async move {
            assert!(fixture.exists().await?);
            Err(std::io::Error::other("synthetic returned failure").into())
        })
        .await
        .err()
        .ok_or("returned failure was swallowed")?;
    assert!(error.downcast_ref::<std::io::Error>().is_some());
    assert!(!returned.exists().await?);

    let synchronous = Fixture::new()?;
    let error = synchronous
        .run(Duration::from_secs(5), |_fixture| {
            fail::<std::future::Ready<TestResult>>()
        })
        .await
        .err()
        .ok_or("synchronous panic was swallowed")?;
    assert!(
        error
            .downcast_ref::<tokio::task::JoinError>()
            .is_some_and(tokio::task::JoinError::is_panic)
    );
    assert!(!synchronous.exists().await?);

    let asynchronous = Fixture::new()?;
    let error = asynchronous
        .run(Duration::from_secs(5), |fixture| async move {
            let _connection = fixture.connect().await?;
            fail::<TestResult>()
        })
        .await
        .err()
        .ok_or("asynchronous panic was swallowed")?;
    assert!(
        error
            .downcast_ref::<tokio::task::JoinError>()
            .is_some_and(tokio::task::JoinError::is_panic)
    );
    assert!(!asynchronous.exists().await?);

    let timed = Fixture::new()?;
    let entered = Arc::new(AtomicBool::new(false));
    let signal = entered.clone();
    let error = timed
        .run(Duration::ZERO, move |_fixture| async move {
            signal.store(true, Ordering::Release);
            pending::<TestResult>().await
        })
        .await
        .err()
        .ok_or("case timeout was swallowed")?;
    assert!(
        entered.load(Ordering::Acquire),
        "deadline did not poll the case"
    );
    assert!(
        error
            .downcast_ref::<tokio::time::error::Elapsed>()
            .is_some()
    );
    assert!(!timed.exists().await?);
    Ok(())
}
