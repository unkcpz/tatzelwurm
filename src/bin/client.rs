use std::time::Duration;

use bytes::Bytes;
use futures::SinkExt;
use tokio::{io::AsyncWriteExt, net::TcpStream, time};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (read_half, write_half) = stream.into_split();

    let mut framed_write = LengthDelimitedCodec::builder()
        .length_field_type::<u16>()
        .new_write(write_half);

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(500)); // 500 ms

    loop {
        tokio::select! {
            // TODO: read from coordinator part

            _ = interval.tick() => {
                println!("Sending heartbeat to server");
                let frame = Bytes::from("alive");
                framed_write.send(frame).await?;
            }
        }
    }
}
