use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::SinkExt;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        mpsc::{self},
        Mutex,
    },
    time,
};

use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, FramedRead, FramedWrite};
use uuid::Uuid;

use tatzelwurm::task::{dispatch, Task};
use tatzelwurm::{
    codec::{Codec, TMessage},
    task,
    worker::{self, Worker},
};

enum Client {
    Worker { id: u32, stream: TcpStream },
    Actioner { id: u32, stream: TcpStream },
}

// Perform handshake, decide client type (worker or actioner) and protocol (always messagepack)
// XXX: hyperequeue seems doesn't have handshake stage, how??
async fn handshake(mut stream: TcpStream) -> anyhow::Result<Client> {
    let mut frame = Framed::new(stream, Codec::<TMessage>::new());

    let message = TMessage::new("Who you are?");
    frame.send(message).await?;

    let client = if let Some(Ok(message)) = frame.next().await {
        dbg!(&message);
        match message {
            TMessage { content: ref c, .. } if c == "worker" => {
                frame.send(TMessage::new("Go")).await?;
                let stream = frame.into_inner();
                Client::Worker { id: 0, stream }
            }
            TMessage { content: ref c, .. } if c == "actioner" => {
                frame.send(TMessage::new("Go")).await?;
                let stream = frame.into_inner();
                Client::Actioner { id: 0, stream }
            }
            _ => anyhow::bail!("unknown client: {message:#?}"),
        }
    } else {
        anyhow::bail!("fail handshake");
    };

    Ok(client)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;
    let worker_table: worker::Table = Arc::new(Mutex::new(HashMap::new()));
    let task_table: task::Table = Arc::new(Mutex::new(HashMap::new()));

    // spawn a load balance lookup task and send process to run on worker
    let worker_table_clone = Arc::clone(&worker_table);
    let task_table_clone = Arc::clone(&task_table);
    tokio::spawn(async move {
        let _ = dispatch(worker_table_clone, task_table_clone).await;
    });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");

                match handshake(stream).await {
                    Ok(Client::Worker { stream, .. }) => {
                        let worker_table_clone = Arc::clone(&worker_table);
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ = handle_worker(stream, worker_table_clone).await;
                        });
                    }
                    Ok(Client::Actioner { stream, .. }) => {
                        let worker_table_clone = Arc::clone(&worker_table);
                        let task_table_clone = Arc::clone(&task_table);
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ =
                                handle_actioner(stream, worker_table_clone, task_table_clone).await;
                        });
                    }
                    _ => {
                        todo!()
                    }
                }
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn handle_worker(stream: TcpStream, worker_table: worker::Table) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    let mut interval = time::interval(Duration::from_millis(2000));

    // TODO: The handshake is the guardian for security, the authentication should
    // happend here.
    let client_id = Uuid::new_v4();

    // XXX: different client type: (1) worker (2) actioner from handshaking
    let (tx, mut rx) = mpsc::channel(100);
    let worker = Worker { tx, load: 0 };
    worker_table.lock().await.insert(client_id, worker);

    loop {
        tokio::select! {
            // message from worker client
            // this contains heartbeat (only access table when worker dead, the mission then
            // re-dispateched to other live worker. It should handle timeout for bad network condition,
            // but that can be complex, not under consideration in the POC implementation)
            // TODO:
            // - should reported from worker when the mission is finished
            // - should also get information from worker complain about the long running
            // block process if it runs on non-block worker.
            Some(Ok(message)) = framed_reader.next() => {
                // dbg!(&message);
                match message {
                    TMessage { id: 4, .. } => {
                        println!("server alive!");
                    }
                    TMessage { id: 8, .. } => {
                        framed_writer.send(TMessage::new("chhanging table to mark pro as running")).await?;
                    }
                    TMessage { id: 6, .. } => {
                        framed_writer.send(TMessage::new("changing table to mark proc as terminated (c)")).await?;
                    }
                    TMessage { id: 7, .. } => {
                        framed_writer.send(TMessage::new("changing table to mark proc as terminated (e)")).await?;
                    }
                    _ => {
                        println!("main.rs narrate {message:?}");
                    }
                }
            }

            // message from task dispatch table lookup
            // fast-forward to the real worker client
            // then update the worker booking
            Some(message) = rx.recv() => {
                framed_writer.send(message).await?;

                // TODO: this should move to above when get submit ack message
                // v.v load -1 should happened when get complete ack message
                let mut worker_table = worker_table.lock().await;
                let worker = worker_table.get_mut(&client_id).unwrap();

                worker.load += 1;
            }

            _ = interval.tick() => {
                println!("heartbeat Coordinator -> Worker");
                let message = TMessage {
                    id: 4,
                    content: "server alive".to_owned(),
                };
                framed_writer.send(message).await?;
            }
        }
    }
}

async fn handle_actioner(
    stream: TcpStream,
    worker_table: worker::Table,
    task_table: task::Table,
) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    // message from worker client
    // this contains heartbeat (only access table when worker dead, the mission then
    // re-dispateched to other live worker. It should handle timeout for bad network condition,
    // but that can be complex, not under consideration in the POC implementation)
    // TODO:
    // - should reported from worker when the mission is finished
    // - should also get information from worker complain about the long running
    // block process if it runs on non-block worker.
    if let Some(Ok(message)) = framed_reader.next().await {
        let msg = message.content;

        match msg.as_str() {
            "inspect" => {
                let resp_msg = TMessage::new(
                    format!(
                        "Good, I heard you say '{msg}' \n {worker_table:#?}, \n {task_table:#?}"
                    )
                    .as_str(),
                );
                framed_writer.send(resp_msg).await?;
            }
            // placeholder, add a random test task to table
            "submit" => {
                let mut task_table = task_table.lock().await;
                let task = Task::new(0);
                task_table.insert(task.id, task);
                let resp_msg = TMessage::new(
                    format!(
                        "Good, I heard you say '{msg}' \n {worker_table:#?}, \n {task_table:#?}"
                    )
                    .as_str(),
                );
                framed_writer.send(resp_msg).await?;
            }
            _ => {
                let resp_msg = TMessage::new(format!("Shutup, I heard you say '{msg}'").as_str());
                framed_writer.send(resp_msg).await?;
            }
        }
    }

    Ok(())
}
