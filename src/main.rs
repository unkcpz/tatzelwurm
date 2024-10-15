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
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

use tatzelwurm::mission::dispatch;
use tatzelwurm::{
    codec::{Codec, TMessage},
    worker::{ClientMap, Worker},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;
    let client_map: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    // spawn a load balance lookup task and send process to run on worker
    let client_map_clone = Arc::clone(&client_map);
    tokio::spawn(async move {
        let _ = dispatch(client_map_clone).await;
    });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");

                let client_map_clone = Arc::clone(&client_map);
                tokio::spawn(async move {
                    // TODO: process_stream shouldn't return, using tracing to recording logs and
                    // handling errors
                    let _ = handle_client(stream, client_map_clone).await;
                });
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn handle_client(stream: TcpStream, client_map: ClientMap) -> anyhow::Result<()> {
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
    client_map.lock().await.insert(client_id, worker);

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
                dbg!(message);
            }

            // message from mission dispatch table lookup
            // forward to the worker
            Some(message) = rx.recv() => {
                framed_writer.send(message).await?;

                let mut client_map = client_map.lock().await;
                let worker = client_map.get_mut(&client_id).unwrap();

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
