use std::time::Duration;

use futures::SinkExt;
use tokio::{
    net::{tcp::WriteHalf, TcpListener, TcpStream},
    sync::mpsc::{self},
    time,
};

use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, FramedRead, FramedWrite};

use tatzelwurm::{
    assign::assign,
    codec::Operation::{Inspect, Submit},
};
use tatzelwurm::{
    codec::Codec,
    task,
    worker::{self},
};
use tatzelwurm::{
    codec::{IMessage, XMessage},
    task::Task,
};
use uuid::Uuid;

use crate::worker::Worker;

enum Client {
    Worker { id: u32, stream: TcpStream },
    Actioner { id: u32, stream: TcpStream },
}

// Perform handshake, decide client type (worker or actioner) and protocol (always messagepack)
// XXX: hyperequeue seems doesn't have handshake stage, how??
async fn handshake(stream: TcpStream) -> anyhow::Result<Client> {
    let mut frame = Framed::new(stream, Codec::<XMessage>::new());

    frame
        .send(XMessage::HandShake("Who you are?".to_string()))
        .await?;

    let client = if let Some(Ok(message)) = frame.next().await {
        match message {
            XMessage::HandShake(info) => match info.as_str() {
                "worker" => {
                    frame.send(XMessage::HandShake("Go".to_string())).await?;
                    let stream = frame.into_inner();
                    Client::Worker { id: 0, stream }
                }
                "actioner" => {
                    frame.send(XMessage::HandShake("Go".to_string())).await?;
                    let stream = frame.into_inner();
                    Client::Actioner { id: 0, stream }
                }
                _ => anyhow::bail!("unknown client: {info:#?}"),
            },
            _ => anyhow::bail!("unknown message: {message:#?}"),
        }
    } else {
        anyhow::bail!("fail handshake");
    };

    Ok(client)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;
    let worker_table = worker::Table::new();
    let task_table = task::Table::new();

    // spawn a load balance lookup task and send process to run on worker
    let worker_table_clone = worker_table.clone();
    let task_table_clone = task_table.clone();
    tokio::spawn(async move {
        let _ = assign(worker_table_clone, task_table_clone).await;
    });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");
                let worker_table_clone = worker_table.clone();
                let task_table_clone = task_table.clone();

                match handshake(stream).await {
                    Ok(Client::Worker { stream, .. }) => {
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ =
                                handle_worker(stream, worker_table_clone, task_table_clone).await;
                        });
                    }
                    Ok(Client::Actioner { stream, .. }) => {
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ =
                                handle_actioner(stream, worker_table_clone, task_table_clone).await;
                        });
                    }
                    _ => {
                        // Hand shake failed, refuse connection and close stream
                        eprintln!("Hand shake refuse for {addr}");
                        // XXX: can not use mut since it is moved, how??
                        // stream.shutdown().await?;
                    }
                }
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn handle_worker(
    mut stream: TcpStream,
    worker_table: worker::Table,
    task_table: task::Table,
) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<XMessage>::new());

    let mut interval = time::interval(Duration::from_millis(2000));

    // TODO: The handshake is the guardian for security, the authentication should
    // happend here.

    // XXX: different client type: (1) worker (2) actioner from handshaking
    let (tx, mut rx) = mpsc::channel(100);
    let worker = Worker::new(tx);

    let worker_id = worker_table.create(worker).await;

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
            Some(Ok(msg)) = framed_reader.next() => {
                handle_worker_xmessage(&msg, &mut framed_writer, &worker_table, &task_table, &worker_id).await?;
            }

            // message from task assign table lookup
            // fast-forward to the real worker client
            // then update the worker booking
            Some(imsg) = rx.recv() => {
                if let IMessage::TaskLaunch(id) = imsg {
                    let xmsg = XMessage::TaskLaunch(id);
                    framed_writer.send(xmsg).await?;
                } else {
                    anyhow::bail!("unknown msg {imsg:?} from assigner.");
                }
            }

            _ = interval.tick() => {
                println!("heartbeat Coordinator -> Worker");
                framed_writer.send(XMessage::HeartBeat(0)).await?;
            }
        }
    }
}

async fn handle_worker_xmessage<'a>(
    msg: &XMessage,
    framed_writer: &mut FramedWrite<WriteHalf<'a>, Codec<XMessage>>,
    worker_table: &worker::Table,
    task_table: &task::Table,
    worker_id: &Uuid,
) -> anyhow::Result<()> {
    match msg {
        XMessage::HeartBeat(port) => {
            println!("worker {port} alive!");
        }

        //
        XMessage::TaskStateChange {
            id,
            from: task::State::Submit,
            to: task::State::Run,
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Run;
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.incr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }
        XMessage::TaskStateChange {
            id,
            from: task::State::Run,
            to: task::State::Complete,
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Complete;
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.decr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }
        XMessage::TaskStateChange {
            id,
            from: task::State::Run,
            to: task::State::Except,
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Except;
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.decr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }
        _ => {
            println!("main.rs narrate {msg:?}");
        }
    }

    Ok(())
}

async fn handle_actioner(
    mut stream: TcpStream,
    worker_table: worker::Table,
    task_table: task::Table,
) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<XMessage>::new());

    // message from worker client
    // this contains heartbeat (only access table when worker dead, the mission then
    // re-dispateched to other live worker. It should handle timeout for bad network condition,
    // but that can be complex, not under consideration in the POC implementation)
    // TODO:
    // - should reported from worker when the mission is finished
    // - should also get information from worker complain about the long running
    // block process if it runs on non-block worker.
    if let Some(Ok(msg)) = framed_reader.next().await {
        match msg {
            XMessage::ActionerOp(Inspect) => {
                let resp_msg = XMessage::Message {
                    id: 0,
                    content: format!("Good, hear you, \n {worker_table:#?}, \n {task_table:#?} \n"),
                };
                framed_writer.send(resp_msg).await?;
            }
            // placeholder, add a random test task to table
            XMessage::ActionerOp(Submit) => {
                let task = Task::new(0);
                task_table.create(task).await;
                let resp_msg = XMessage::Message {
                    id: 0,
                    content: format!("Good, hear you, \n {worker_table:#?}, \n {task_table:#?} \n"),
                };
                framed_writer.send(resp_msg).await?;
            }
            _ => {
                let resp_msg = XMessage::Message {
                    id: 0,
                    content: format!("Shutup, I try to ignore you, since you say '{msg:#?}'"),
                };
                framed_writer.send(resp_msg).await?;
            }
        }
    }

    Ok(())
}
