use clap::Parser;
use futures::SinkExt;
use tatzelwurm::codec::Operation;
use tatzelwurm::codec::{Codec, XMessage};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Operation to take
    #[arg(short, long)]
    operation: String,
}

// TODO: this can be generic as a client method and used together with worker and even as interface
// for other language client.
async fn handshake<'a>(rhalf: &mut ReadHalf<'a>, whalf: &mut WriteHalf<'a>) -> anyhow::Result<()> {
    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    loop {
        if let Some(Ok(message)) = framed_reader.next().await {
            match message {
                XMessage::HandShake(info) => match info.as_str() {
                    "Go" => {
                        println!("handshake successful!");
                        break;
                    }
                    "Who you are?" => {
                        framed_writer
                            .send(XMessage::HandShake("actioner".to_string()))
                            .await?;
                    }
                    _ => eprintln!("unknown handshake info: {info}"),
                },
                _ => eprintln!("unknown message: {message:#?}"),
            }
        }
    }

    Ok(())
}

// NOTE: This bin should be as simple as possible, since it will be replaced with python client
// with using the comm API provided by the wurm.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let (mut rhalf, mut whalf) = stream.split();

    if let Err(err) = handshake(&mut rhalf, &mut whalf).await {
        anyhow::bail!(err);
    }

    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    // define the mission and then adding a mission to the table (and set the state to ready)
    // This will define the mission and put the mission to some where accessable by the runner
    // (in aiida it then will be a DB.)
    // let msg = Message::add_mission(Mission::new());

    match args.operation.as_str() {
        "inspect" => {
            framed_writer.send(XMessage::PrintTable()).await?;
        }
        "add_task" => {
            framed_writer.send(XMessage::ActionerOp(Operation::AddTask)).await?;
        },
        "play_all_task" => {
            framed_writer.send(XMessage::ActionerOp(Operation::PlayAllTask)).await?;
        },
        _ => {
            framed_writer
                .send(XMessage::BulkMessage("useless op".to_string()))
                .await?;
        }
    }

    if let Some(Ok(msg)) = framed_reader.next().await {
        match msg {
            XMessage::BulkMessage(s) => {
                println!("{s}");
            }
            _ => {
                dbg!(msg);
            }
        }
    }

    Ok(())
}
