use std::{collections::HashMap, sync::Arc, time::Duration};

use bytes::{Buf, Bytes, BytesMut};
use futures::SinkExt;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::{
        mpsc::{self, Receiver},
        Mutex,
    },
    time,
};

use thiserror::Error;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, FramedRead, FramedWrite, LengthDelimitedCodec};

use tatzelwurm::{Codec, TMessage};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("incomplete stream to parse as a frame")]
    InComplete,
    #[error("unable to parse {0:?}")]
    ParseError(Bytes),
    #[error("unknown data type leading with byte {0:?}")]
    UnknownType(u8),
}

// XXX: use tokio Mutex or sync Mutex?
type ClientMap = Arc<Mutex<HashMap<Uuid, mpsc::Sender<TMessage>>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;
    let client_map: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    // spawn a load balance lookup task and send process to run on worker
    let client_map_clone = Arc::clone(&client_map);
    tokio::spawn(async move {
        load_balancing(client_map_clone).await;
    });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");

                // TODO: The handshake is the guardian for security, the authentication should
                // happend here.
                let client_id = Uuid::new_v4();

                // XXX: for demo, always attach mpsc channel at the moment for all types of clients.

                let (tx, rx) = mpsc::channel(100);
                client_map.lock().await.insert(client_id, tx);

                tokio::spawn(async move {
                    // TODO: process_stream shouldn't return, using tracing to recording logs and
                    // handling errors
                    let _ = handle_client(stream, rx).await;
                });
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn load_balancing(client_map: ClientMap) {
    todo!()
}

async fn handle_client(mut stream: TcpStream, mut rx: Receiver<TMessage>) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    let mut interval = time::interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            // message from worker client
            Some(Ok(message)) = framed_reader.next() => {
                dbg!(message);
            }

            // message from load balancing table lookup
            Some(message) = rx.recv() => {
                dbg!(message);
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
