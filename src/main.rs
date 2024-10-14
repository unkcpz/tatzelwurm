use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use futures::SinkExt;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    time,
};

use thiserror::Error;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, FramedRead, FramedWrite, LengthDelimitedCodec};

use tatzelwurm::{Codec, TMessage};

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("incomplete stream to parse as a frame")]
    InComplete,
    #[error("unable to parse {0:?}")]
    ParseError(Bytes),
    #[error("unknown data type leading with byte {0:?}")]
    UnknownType(u8),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");
                tokio::spawn(async move {
                    // TODO: process_stream shouldn't return, using tracing to recording logs and
                    // handling errors
                    let _ = handle_client(socket).await;
                });
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    let mut interval = time::interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
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
