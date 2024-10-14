use std::time::Duration;

use bytes::Bytes;
use futures::SinkExt;
use tokio_stream::StreamExt;
use tokio::{io::AsyncWriteExt, net::TcpStream, time};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (reader_half, writer_half) = stream.into_split();

    let mut framed_reader = LengthDelimitedCodec::builder()
        .length_field_type::<u16>()
        .new_read(reader_half);

    let mut framed_writer = LengthDelimitedCodec::builder()
        .length_field_type::<u16>()
        .new_write(writer_half);

    // Start a heartbeat interval
    let mut interval = time::interval(Duration::from_millis(500)); // 500 ms

    loop {
        tokio::select! {
            Some(Ok(message)) = framed_reader.next() => {
                dbg!(message);
            }

            _ = interval.tick() => {
                println!("Sending heartbeat to server");
                let frame = Bytes::from("alive");
                framed_writer.send(frame).await?;
            }
        }
    }
}
