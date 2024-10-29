use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
use tatzelwurm::codec::{Codec, TMessage};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::oneshot,
    time::{self, sleep},
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

async fn perform_task() {
    let x = {
        let mut rng = rand::thread_rng();
        rng.gen_range(1..10)
    };
    sleep(Duration::from_secs(x)).await;
    println!("Task {x} complete!");
}

fn run_task_with_ack() -> oneshot::Receiver<()> {
    let (ack_tx, ack_rx) = oneshot::channel();

    // XXX: what is the different if this is not a spawned task?
    tokio::spawn(async move {
        perform_task().await;
        let _ = ack_tx.send(());
    });

    ack_rx
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    if let Some(Ok(message)) = framed_reader.next().await {
        if message.content == "Who you are?" {
            framed_writer.send(TMessage::new("worker")).await?;
        } else {
            eprintln!("unknown message: {message:#?}");
        }
    }

    if let Some(Ok(message)) = framed_reader.next().await {
        if message.content == "Go" {
            println!("handshake successful!");
        } else {
            framed_writer.get_mut().shutdown().await?;
            anyhow::bail!("handshake fail");
        }
    }

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(2000)); // 2000 ms

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                match message {
                    TMessage { id: 1003, .. } => {
                        framed_writer.send(TMessage::new("I got a task!! run it man!!")).await?;
                        let msg = TMessage {
                            id: 8,
                            content: "running".to_owned(),
                        };
                        framed_writer.send(msg).await?;

                        let ack_rx = run_task_with_ack();

                        if let Ok(()) = ack_rx.await {
                                let msg = TMessage {
                                    id: 6,
                                    content: "complete".to_owned(),
                                };
                                framed_writer.send(msg).await?;
                        }
                        else {
                            let msg = TMessage {
                                id: 7,
                                content: "except".to_owned(),
                            };
                            framed_writer.send(msg).await?;
                        }
                    }
                    _ => {
                        println!("worker.rs narrate {message:?}");
                    }
                }
            }

            _ = interval.tick() => {
                println!("Sending heartbeat to server");
                let message = TMessage {
                    id: 5,
                    content: "client alive".to_owned(),
                };
                framed_writer.send(message).await?;
            }
        }
    }
}
