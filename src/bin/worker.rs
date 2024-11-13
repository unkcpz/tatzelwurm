use chrono::Utc;
use evalexpr::eval;
use std::thread;
use std::time::Duration;

use futures::SinkExt;
use tatzelwurm::codec::{Codec, XMessage};
use tatzelwurm::interface::{handshake, ClientType};
use tatzelwurm::task::State as TaskState;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

use std::sync::LazyLock;
use surrealdb::engine::remote::ws::Client;
use surrealdb::sql::Datetime;

/// For dev and test purpose
/// Could move to example or as independent crates depend on how to support it in future.
/// The surrealdb crate dependency should only used by actioner/worker bins.
/// The surrealdb crate should not be used by the main crate.

// static singleton for DB access per worker.
static DB: LazyLock<Surreal<Client>> = LazyLock::new(Surreal::init);

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MockTask {
    expr: String,

    // in milliseconds
    snooze: u64,

    is_block: bool,
    create_at: Datetime,
    start_at: Option<Datetime>,
    end_at: Option<Datetime>,
    res: Option<String>,
}

async fn perform_async(snooze: u64, expr: &str) -> (String, Datetime, Datetime) {
    let start_at = Utc::now();
    let res = eval(expr);
    tokio::time::sleep(Duration::from_millis(snooze)).await;
    let end_at = Utc::now();

    let res = match res {
        Ok(value) => value.to_string(),
        Err(err) => err.to_string(),
    };
    (res, start_at.into(), end_at.into())
}

fn perform_sync(snooze: u64, expr: &str) -> (String, Datetime, Datetime) {
    let start_at = Utc::now();
    let res = eval(expr);
    thread::sleep(Duration::from_millis(snooze));
    let end_at = Utc::now();

    let res = match res {
        Ok(value) => value.to_string(),
        Err(err) => err.to_string(),
    };
    (res, start_at.into(), end_at.into())
}

async fn run_task_with_ack(record_id: String) -> oneshot::Receiver<()> {
    let (ack_tx, ack_rx) = oneshot::channel();

    let resource = record_id.split_once(':');

    let mocktask: Option<MockTask> = if let Some(resource) = resource {
        DB.select(resource).await.unwrap()
    } else {
        None
    };

    if let Some(mut mocktask) = mocktask {
        let block = mocktask.is_block;
        let x = mocktask.snooze;
        let expr = mocktask.clone().expr;

        let (res, start_at, end_at) = if block {
            let (res, start_at, end_at) =
                tokio::task::spawn_blocking(move || perform_sync(x, &expr))
                    .await
                    .unwrap();
            tokio::task::yield_now().await;
            (res, start_at, end_at)
        } else {
            perform_async(x, &expr).await
        };

        mocktask.start_at = Some(start_at);
        mocktask.end_at = Some(end_at);
        mocktask.res = Some(res);

        if let Some(resource) = resource {
            let _: Option<MockTask> = DB.update(resource).content(mocktask).await.unwrap();
        }
        let _ = ack_tx.send(());
    } else {
        // ack not found except
        todo!()
    }

    ack_rx
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let socket_addr = stream.local_addr()?;

    let Ok(mut stream) = handshake(stream, ClientType::Worker).await else {
        anyhow::bail!("handshak failed");
    };

    let (rhalf, whalf) = stream.split();

    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(2000)); // 2000 ms

    let max_slots = 2000;
    let (tx, mut rx) = mpsc::channel(max_slots);

    // DB part for prototype
    DB.connect::<Ws>("localhost:8000").await?;
    // Sign in to the server
    DB.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    DB.use_ns("test").use_db("test").await?;

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                match message {
                    XMessage::TaskLaunch { task_id, record_id } => {
                        // dummy message to be printed in coordinator side
                        framed_writer
                            .send(XMessage::BulkMessage(format!(
                                "Processing on {task_id}."
                            )))
                            .await?;

                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            let _ = run_task(task_id, record_id, tx_clone).await;
                        });
                    }
                    XMessage::HeartBeat(port) => {
                        println!("coordinator {port} alive!");
                    }
                    _ => {
                        println!("worker.rs narrate {message:?}");
                    }
                }
            }

            // relay msg from task runner to coordinator
            Some(msg) = rx.recv() => {
                framed_writer.send(msg).await?;
            }

            _ = interval.tick() => {
                framed_writer.send(XMessage::HeartBeat(socket_addr.port())).await?;
            }
        }
    }
}

// task_id for communication back to communicator
// record_id for se/de the record from task pool
async fn run_task<'a>(
    task_id: Uuid,
    record_id: String,
    tx: mpsc::Sender<XMessage>,
) -> anyhow::Result<()> {
    // Send to tell start processing on the task
    let msg = XMessage::TaskStateChange {
        id: task_id,
        from: TaskState::Submit,
        to: TaskState::Run,
    };
    tx.send(msg).await?;

    // The way to fire a task is:
    // 0. Run the task and get the ack_rx
    // 1. send a message and ask to add the item into the task table.
    // 2. send a message to ask for table look up.
    let ack_rx = run_task_with_ack(record_id).await;

    // rx ack resolved means the task is complete
    if let Ok(()) = ack_rx.await {
        let msg = XMessage::TaskStateChange {
            id: task_id,
            from: TaskState::Run,
            to: TaskState::Terminated(0),
        };
        tx.send(msg).await?;
    } else {
        let msg = XMessage::TaskStateChange {
            id: task_id,
            from: TaskState::Run,
            to: TaskState::Terminated(1), // TODO: exit_code from ack_rx
        };
        tx.send(msg).await?;
    }

    Ok(())
}
