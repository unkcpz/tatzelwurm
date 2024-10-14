use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    time,
};
use futures::SinkExt;

use thiserror::Error;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

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

    // wrap stream into a framed codec
    let mut framed_reader = LengthDelimitedCodec::builder()
        // .length_field_offset(0)
        .length_field_type::<u16>()
        // .length_adjustment(2)
        // .num_skip(0)
        .new_read(read_half);

    // server heartbeat interval

    let mut framed_writer = LengthDelimitedCodec::builder()
        .length_field_type::<u16>()
        .new_write(write_half);

    let mut interval = time::interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                dbg!(message);
            }

            _ = interval.tick() => {
                println!("heartbeat Coordinator -> Worker");
                let frame = Bytes::from("coordinator alive");
                framed_writer.send(frame).await?;
            }
        }
    }
}
