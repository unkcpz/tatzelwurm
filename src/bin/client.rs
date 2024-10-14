use std::time::Duration;

use bytes::Bytes;
use futures::SinkExt;
use tokio_stream::StreamExt;
use tokio::{io::AsyncWriteExt, net::TcpStream, time};
use tokio_util::codec::{Framed, FramedRead, FramedWrite, LengthDelimitedCodec};

use tatzelwurm::{TMessage, Codec};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(500)); // 500 ms

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                dbg!(message);
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
