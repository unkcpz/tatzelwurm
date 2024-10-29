use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
use tatzelwurm::codec::{Codec, XMessage};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::{
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

async fn handshake<'a>(rhalf: &mut ReadHalf<'a>, whalf: &mut WriteHalf<'a>) -> anyhow::Result<()> {
    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    loop {
        if let Some(Ok(message)) = framed_reader.next().await {
            match message {
                XMessage::HandShake(info) => match info.as_str() {
                    "Go" => {
                        println!("handshake successful!");
                        break;
                    }
                    "Who you are?" => {
                        framed_writer
                            .send(XMessage::HandShake("worker".to_string()))
                            .await?;
                    }
                    _ => eprintln!("unknown handshake info: {info}"),
                },
                _ => eprintln!("unknown message: {message:#?}"),
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let socket_addr = stream.local_addr()?;

    let (mut rhalf, mut whalf) = stream.split();

    if let Err(err) = handshake(&mut rhalf, &mut whalf).await {
        anyhow::bail!(err);
    }


    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(2000)); // 2000 ms

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                match message {
                    XMessage::TaskDispatch(id) => {
                        framed_writer.send(
                            XMessage::Message {
                                content: format!("I got the task {id}, Sir! Working on it!"), 
                                id: 0
                            }).await?;
                        let msg = XMessage::Message { id: 8, content: "running".to_string() };
                        framed_writer.send(msg).await?;

                        let ack_rx = run_task_with_ack();

                        if let Ok(()) = ack_rx.await {
                            let msg = XMessage::Message { id: 6, content: "complete".to_string() };
                            framed_writer.send(msg).await?;
                        }
                        else {
                            let msg = XMessage::Message { id: 7, content: "except".to_string() };
                            framed_writer.send(msg).await?;
                        }
                    }
                    XMessage::HeartBeat(port) => {
                        println!("coordinator {port} alive!");
                    }
                    _ => {
                        println!("worker.rs narrate {message:?}");
                    }
                }
            }

            _ = interval.tick() => {
                println!("Sending heartbeat to server");
                framed_writer.send(XMessage::HeartBeat(socket_addr.port())).await?;
            }
        }
    }
}
