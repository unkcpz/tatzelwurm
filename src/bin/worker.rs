/// For dev and test purpose
/// Could move to example or as independent crates depend on how to support it in future.
/// The surrealdb crate dependency should only used by actioner/worker bins.
/// The surrealdb crate should not be used by the main crate.
use std::thread;
use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
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
use surrealdb::opt::Resource;
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::Value;

use surrealdb::sql::Datetime;

#[derive(Debug, Serialize)]
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

/// This is the dummy task that should be the interface for real async task
/// which can be constructed from persistence.
async fn perform_async_task() {
    let x = {
        let mut rng = rand::thread_rng();
        rng.gen_range(1..10)
    };
    tokio::time::sleep(Duration::from_secs(x)).await;
    println!("Task that sleep {x}s complete!");
}

// Dummy task that is sync blocked
fn perform_sync_task() {
    let x = {
        let mut rng = rand::thread_rng();
        rng.gen_range(1..10)
    };
    thread::sleep(Duration::from_secs(x));
    println!("Task that sleep {x}s complete!");
}

async fn run_task_with_ack(record_id: String) -> oneshot::Receiver<()> {
    let (ack_tx, ack_rx) = oneshot::channel();

    // XXX: this should be the info from parse and construct from id
    let block = false;

    if block {
        tokio::task::spawn_blocking(move || {
            println!("Mock the constructing of sync task {record_id} from persistence.");
            perform_sync_task();
        });
    } else {
        println!("Mock the constructing of async task {record_id} from persistence.");
        perform_async_task().await;
    }
  
    let _ = ack_tx.send(());

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
    let db = Surreal::new::<Ws>("127.0.0.1:8000").await?;

    db.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    db.use_ns("test").use_db("test").await?;

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
async fn run_task<'a>(task_id: Uuid, record_id: String, tx: mpsc::Sender<XMessage>) -> anyhow::Result<()> {
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
