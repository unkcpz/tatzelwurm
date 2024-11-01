use std::time::Duration;

use futures::SinkExt;
use rand::{self, Rng};
use tatzelwurm::codec::{Codec, XMessage};
use tatzelwurm::task::State as TaskState;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::{
    net::TcpStream,
    sync::oneshot,
    time::{self, sleep},
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

async fn perform_task() {
    let x = {
        let mut rng = rand::thread_rng();
        rng.gen_range(1..10)
    };
    sleep(Duration::from_secs(x)).await;
    println!("Task that sleep {x}s complete!");
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
                    XMessage::TaskLaunch(id) => {
                        run_task(id, &mut framed_reader, &mut framed_writer).await?;
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

async fn run_task<'a>(
    id: Uuid,
    framed_reader: &mut FramedRead<ReadHalf<'a>, Codec<XMessage>>,
    framed_writer: &mut FramedWrite<WriteHalf<'a>, Codec<XMessage>>,
) -> anyhow::Result<()> {
    // dummy message to be printed in coordinator side
    framed_writer
        .send(XMessage::BulkMessage(format!(
            "I got the task {id}, Sir! Working on it!"
        )))
        .await?;

    // The way to fire a task is:
    // 0. Run the task and get the ack_rx
    // 1. send a message and ask to add the item into the task table.
    // 2. send a message to ask for table look up.
    let ack_rx = run_task_with_ack();

    let msg = XMessage::TaskStateChange {
        id,
        from: TaskState::Submit,
        to: TaskState::Run,
    };
    framed_writer.send(msg).await?;

    // rx ack resolved means the task is complete
    if let Ok(()) = ack_rx.await {
        let msg = XMessage::TaskStateChange {
            id,
            from: TaskState::Run,
            to: TaskState::Complete,
        };
        framed_writer.send(msg).await?;
    } else {
        // TODO: distinguish from the successful complete
        let msg = XMessage::TaskStateChange {
            id,
            from: TaskState::Run,
            to: TaskState::Except,
        };
        framed_writer.send(msg).await?;
    }

    Ok(())
}
