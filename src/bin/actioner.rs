use futures::SinkExt;
use tatzelwurm::codec::{Codec, TMessage};
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (read_half, write_half) = stream.into_split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<TMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<TMessage>::new());

    if let Some(Ok(message)) = framed_reader.next().await {
        if message.content == "Who you are?" {
            framed_writer.send(TMessage::new("actioner")).await?;
        } else {
            eprintln!("unknown message: {message:#?}");
        }
    }

    if let Some(Ok(message)) = framed_reader.next().await {
        if message.content == "Go" {
            println!("handshake successful!");
        } else {
            framed_writer.get_mut().shutdown().await?;
            anyhow::bail!("handshake fail");
        }
    }

    let msg = TMessage::new("this is a message");
    framed_writer.send(msg).await?;

    if let Some(Ok(resp_msg)) = framed_reader.next().await {
        dbg!(resp_msg);
    }

    Ok(())
}
