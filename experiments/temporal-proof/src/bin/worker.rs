use edgeagent_temporal_proof::{M08Activities, M08Workflow};
use std::{env, error::Error};
use temporalio_client::{Client, ClientOptions, Connection, ConnectionOptions, Url};
use temporalio_sdk::{Runtime, Worker, WorkerOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let endpoint = args.next().ok_or("missing server endpoint")?;
    let queue = args.next().ok_or("missing task queue")?;
    if args.next().is_some() {
        return Err("unexpected worker argument".into());
    }

    let runtime = Runtime::from_current_tokio(Default::default())?;
    let connection = Connection::connect(
        ConnectionOptions::new(Url::parse(&endpoint)?)
            .identity("m08-proof-worker".to_owned())
            .build(),
    )
    .await?;
    let client = Client::new(connection, ClientOptions::new("default").build())?;
    let options = WorkerOptions::new(queue)
        .register_workflow::<M08Workflow>()?
        .register_activities(M08Activities)
        .build();
    let mut worker = Worker::new(&runtime, client, options)?;
    worker.run().await?;
    Ok(())
}
