use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use futures::SinkExt;
use rand::{self, Rng};
use surrealdb::sql::Datetime;
use uuid::Uuid;

use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

use tatzelwurm::codec::{Codec, Operation, XMessage};
use tatzelwurm::interface::{handshake, ClientType};
use tatzelwurm::task;

use serde::{Deserialize, Serialize, Serializer};
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::opt::Resource;
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::Value;

/// For dev and test purpose
/// Could move to example or as independent crates depend on how to support it in future.
/// The surrealdb crate dependency should only used by actioner/worker bins.
/// The surrealdb crate should not be used by the main crate.

#[derive(Debug, Serialize)]
struct MockTask {
    expr: String,

    // in milliseconds
    snooze: u64,

    is_block: bool,
    create_at: Datetime,
    start_at: Option<Datetime>,
    end_at: Option<Datetime>,
    res: Option<String>,
}

impl MockTask {
    fn new(snooze: u64, is_block: bool, create_at: Datetime) -> Self {
        let x = {
            let mut rng = rand::thread_rng();
            rng.gen_range(1..10)
        };
        let y = {
            let mut rng = rand::thread_rng();
            rng.gen_range(1..10)
        };

        let syms = ["+", "-", "*", "/"];
        let sym = {
            let mut rng = rand::thread_rng();
            let i = rng.gen_range(0..=3);
            syms[i]
        };

        let expr = format!("{x} {sym} {y}");
        Self {
            expr,
            snooze,
            is_block,
            create_at,
            start_at: None,
            end_at: None,
            res: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Record {
    id: RecordId,
}

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

#[derive(Clone, ValueEnum)]
enum TaskScale {
    Small,
    Medium,
    Large,
}

#[derive(Clone, ValueEnum)]
enum BlockType {
    Async,
    Sync,
}

#[derive(Subcommand)]
enum TaskCommand {
    /// Add a new task
    Add {
        // Scale of task, small, medium, large
        #[arg(short, long, value_enum)]
        scale: TaskScale,

        // Block type, sync or async
        #[arg(short, long, value_enum)]
        block_type: BlockType,
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

// NOTE: This bin should be as simple as possible, since it will be replaced with python client
// with using the comm API provided by the wurm.
#[tokio::main]
#[allow(clippy::too_many_lines)] // FIXME:
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let stream = TcpStream::connect("127.0.0.1:5677").await?;
    println!("Connected to coordinator");

    let Ok(mut stream) = handshake(stream, ClientType::Actioner).await else {
        anyhow::bail!("handshak failed");
    };

    let (rhalf, whalf) = stream.split();

    let mut framed_reader = FramedRead::new(rhalf, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(whalf, Codec::<XMessage>::new());

    // Task Pool using surrealdb
    let db = Surreal::new::<Ws>("127.0.0.1:8000").await?;

    db.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    db.use_ns("test").use_db("test").await?;

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
            TaskCommand::Add {
                scale,
                block_type,
            } => {
                let isblock = match block_type {
                    BlockType::Sync => true,
                    BlockType::Async => false,
                };

                // small scale for 100 ~ 1000 millis
                // medium for 1000 ~ 10_000 millis
                // large for 10_000 ~ 100_000
                let x = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(1..10)
                };

                let st = match scale {
                    TaskScale::Small => x * 100,
                    TaskScale::Medium => x * 1000,
                    TaskScale::Large => x * 10_000,
                };

                let task = MockTask::new(st, isblock, Utc::now().into());

                let created: Option<Record> = db.create("task").content(task).await?;

                if let Some(created) = created {
                    let record_id = created.id.to_string();

                    framed_writer
                        .send(XMessage::ActionerOp(Operation::AddTask(record_id)))
                        .await?;
                } else {
                    eprintln!("not able to create task to pool.");
                }
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
                let states = states
                    .iter()
                    .map(|s| {
                        match s {
                            TaskState::Created => task::State::Created,
                            TaskState::Ready => task::State::Ready,
                            TaskState::Submit => task::State::Submit,
                            TaskState::Pause => task::State::Pause,
                            TaskState::Run => task::State::Run,
                            TaskState::Complete => task::State::Terminated(0),
                            TaskState::Kill => task::State::Terminated(-1),
                            // TODO: not elegant, try better
                            TaskState::Except => task::State::Terminated(-2),
                        }
                    })
                    .collect();
                framed_writer
                    .send(XMessage::TaskTablePrint { states })
                    .await?;
            }
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
