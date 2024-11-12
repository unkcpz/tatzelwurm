/// For dev and test purpose
/// Could move to example or as independent crates depend on how to support it in future.
/// The surrealdb crate dependency should only used by actioner/worker bins.
/// The surrealdb crate should not be used by the main crate.
use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
use tatzelwurm::codec::{Codec, XMessage};
use tatzelwurm::interface::{handshake, ClientType};
use tatzelwurm::task::State as TaskState;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{self, sleep},
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

/// This is the dummy task that should be the interface for real async task
/// which can be constructed from persistence.
async fn perform_task() {
    let x = {
        let mut rng = rand::thread_rng();
        rng.gen_range(1..10)
    };
    sleep(Duration::from_secs(x)).await;
    println!("Task that sleep {x}s complete!");
}

async fn run_task_with_ack(id: Uuid) -> oneshot::Receiver<()> {
    let (ack_tx, ack_rx) = oneshot::channel();

    println!("Mock the constructing of task {id} from persistence.");
    perform_task().await;
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
                    XMessage::TaskLaunch(id) => {
                        // dummy message to be printed in coordinator side
                        framed_writer
                            .send(XMessage::BulkMessage(format!(
                                "I got the task {id}, Sir! Working on it!"
                            )))
                            .await?;

                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            let _ = run_task(id, tx_clone).await;
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

async fn run_task<'a>(id: Uuid, tx: mpsc::Sender<XMessage>) -> anyhow::Result<()> {
    // Send to tell start processing on the task
    let msg = XMessage::TaskStateChange {
        id,
        from: TaskState::Submit,
        to: TaskState::Run,
    };
    tx.send(msg).await?;

    // The way to fire a task is:
    // 0. Run the task and get the ack_rx
    // 1. send a message and ask to add the item into the task table.
    // 2. send a message to ask for table look up.
    let ack_rx = run_task_with_ack(id).await;

    // rx ack resolved means the task is complete
    if let Ok(()) = ack_rx.await {
        let msg = XMessage::TaskStateChange {
            id,
            from: TaskState::Run,
            to: TaskState::Terminated(0),
        };
        tx.send(msg).await?;
    } else {
        let msg = XMessage::TaskStateChange {
            id,
            from: TaskState::Run,
            to: TaskState::Terminated(1), // TODO: exit_code from ack_rx
        };
        tx.send(msg).await?;
    }

    Ok(())
}
