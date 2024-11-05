use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
use tatzelwurm::codec::{Codec, XMessage};
use tatzelwurm::task::State as TaskState;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{self, sleep},
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, FramedRead, FramedWrite};
use uuid::Uuid;

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

async fn handshake(stream: TcpStream) -> anyhow::Result<TcpStream> {
    let mut framed = Framed::new(stream, Codec::<XMessage>::new());

    loop {
        if let Some(Ok(message)) = framed.next().await {
            match message {
                XMessage::HandShake(info) => match info.as_str() {
                    "Go" => {
                        println!("handshake successful!");
                        break;
                    }
                    "Who you are?" => {
                        framed
                            .send(XMessage::HandShake("worker".to_string()))
                            .await?;
                    }
                    _ => eprintln!("unknown handshake info: {info}"),
                },
                _ => eprintln!("unknown message: {message:#?}"),
            }
        }
    }

    Ok(framed.into_inner())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let socket_addr = stream.local_addr()?;

    let Ok(mut stream) = handshake(stream).await else {
        anyhow::bail!("handshak failed");
    };

    let (rhalf, whalf) = stream.split();

    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(2000)); // 2000 ms

    let max_slots = 2000;
    let (tx, mut rx) = mpsc::channel(max_slots);

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
