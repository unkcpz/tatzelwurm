use clap::{Parser, Subcommand, ValueEnum};
use futures::SinkExt;
use tatzelwurm::codec::Operation;
use tatzelwurm::codec::{Codec, XMessage};
use tatzelwurm::task;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

#[derive(Clone, ValueEnum)]
enum TaskState {
    Created,
    Ready,
    Submit,
    Pause,
    Run,
    Complete,
    Kill,
    Except,
}

#[derive(Subcommand)]
enum TaskCommand {
    /// Add a new task
    Add {
        // Number of tasks
        #[arg(short)]
        number: u32,
    },

    /// Play a specific task or all tasks
    Play {
        /// Task ID to play
        id: Option<Uuid>,
        /// Flag to play all tasks
        #[arg(short, long)]
        all: bool,
    },

    /// Kill a specific task
    Kill {
        /// Task ID to play
        id: Option<Uuid>,
    },

    /// List tasks
    List {
        // Filter task states
        #[arg(short, long, value_enum)]
        filter: Vec<TaskState>,
    },
}

#[derive(Subcommand)]
enum WorkerCommand {
    /// list workers
    List,
}

#[derive(Subcommand)]
enum Commands {
    // Operations on task(s)
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },

    // Operations on table(s)
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
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
    let cli = Cli::parse();

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

    match cli.commands {
        Commands::Worker { command } => match command {
            WorkerCommand::List => {
                framed_writer.send(XMessage::WorkerTablePrint).await?;
            }
        },
        Commands::Task { command } => match command {
            TaskCommand::Add { number } => {
                framed_writer
                    .send(XMessage::ActionerOp(Operation::AddTask(number)))
                    .await?;
            }
            TaskCommand::Play { all: true, .. } => {
                framed_writer
                    .send(XMessage::ActionerOp(Operation::PlayAllTask))
                    .await?;
            }
            TaskCommand::Play { id, all: false } => {
                if let Some(id) = id {
                    framed_writer
                        .send(XMessage::ActionerOp(Operation::PlayTask(id)))
                        .await?;
                } else {
                    println!("provide the task id.");
                }
            }
            TaskCommand::Kill { id } => {
                if let Some(id) = id {
                    framed_writer
                        .send(XMessage::ActionerOp(Operation::KillTask(id)))
                        .await?;
                } else {
                    println!("provide the task id.");
                }
            }
            TaskCommand::List { filter: states } => {
                let states = states.iter()
                        .map(|s| {
                        match s {
                            TaskState::Created => task::State::Created,
                            TaskState::Ready=> task::State::Ready,
                            TaskState::Submit => task::State::Submit,
                            TaskState::Pause => task::State::Pause,
                            TaskState::Run => task::State::Run,
                            TaskState::Complete => task::State::Terminated(0), 
                            TaskState::Kill => task::State::Terminated(-1), 
                            // TODO: not elegant, try better
                            TaskState::Except => task::State::Terminated(-2), 
                        }
                    }).collect();
                framed_writer
                    .send(XMessage::TaskTablePrint { states })
                    .await?;
            }
            _ => todo!(),
        },
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
