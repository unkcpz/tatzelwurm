use futures::SinkExt;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tabled::builder::Builder as TableBuilder;
use tabled::settings::Style;
use uuid::Uuid;

use tokio::sync::{mpsc::Sender, Mutex};
use tokio::{
    net::{tcp::WriteHalf, TcpStream},
    sync::mpsc::{self},
    time,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::codec::Codec;
use crate::codec::{IMessage, XMessage};
use crate::task;
use crate::task::Table as TaskTable;

#[derive(Debug, Clone)]
pub struct Worker {
    // The rx used for communicate
    // TODO: make fields private
    tx: Sender<IMessage>,

    // number of processes running on this worker
    load: u64,
}

impl Worker {
    #[must_use]
    pub fn new(tx: Sender<IMessage>) -> Self {
        Self { tx, load: 0 }
    }

    pub async fn launch_task(&self, id: &Uuid) -> anyhow::Result<()> {
        let msg = IMessage::TaskLaunch(*id);
        self.tx
            .send(msg)
            .await?;
        Ok(())
    }

    pub fn incr_load(&mut self) {
        self.load += 1;
    }

    pub fn decr_load(&mut self) {
        self.load -= 1;
    }
}

// TODO: use actor pattern
#[derive(Debug, Clone)]
pub struct Table {
    // ??: which Mutex to use, can I use std sync mutex??
    inner: Arc<Mutex<HashMap<Uuid, Worker>>>,
}

#[allow(dead_code)]
type WorkerTable = Table;

impl Table {
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Table {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, w: Worker) -> Uuid {
        let id = Uuid::new_v4();
        self.inner.lock().await.insert(id, w);

        id
    }

    pub async fn update(&self, id: &Uuid, w: Worker) -> anyhow::Result<()> {
        let mut table = self.inner.lock().await;
        if table.contains_key(id) {
            table.insert(*id, w);
            Ok(())
        } else {
            anyhow::bail!("Item {id} not found")
        }
    }

    pub async fn read(&self, id: &Uuid) -> Option<Worker> {
        self.inner.lock().await.get(id).cloned()
    }

    pub async fn delete(&self, id: &Uuid) -> anyhow::Result<()> {
        let mut table = self.inner.lock().await;
        if table.remove(id).is_some() {
            Ok(())
        } else {
            anyhow::bail!("Item {id} not found")
        }
    }

    /// Render a pretty printed table
    pub async fn render(&self) -> String {
        let mut builder = TableBuilder::default();
        for (id, worker) in self.inner.lock().await.iter() {
            let line = vec![id.to_string(), format!("{}", worker.load)];
            builder.push_record(line);
        }
        let header = vec!["Uuid".to_string(), "load".to_string()];
        builder.insert_record(0, header);

        let mut table = builder.build();
        table.with(Style::modern());
        table.to_string()
    }

    pub async fn find_least_loaded_worker(&self) -> Option<Uuid> {
        self.inner
            .lock()
            .await
            .iter()
            .min_by_key(|&(_, w)| w.load)
            .map(|(&id, _)| id)
    }
}

pub async fn handle(
    mut stream: TcpStream,
    worker_table: Table,
    task_table: task::Table,
) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<XMessage>::new());

    let mut interval = time::interval(Duration::from_millis(2000));

    // TODO: The handshake is the guardian for security, the authentication should
    // happend here.

    // XXX: different client type: (1) worker (2) actioner from handshaking
    let (tx, mut rx) = mpsc::channel(100);
    let worker = Worker::new(tx);

    let worker_id = worker_table.create(worker).await;

    loop {
        tokio::select! {
            // message from worker client
            // this contains heartbeat (only access table when worker dead, the mission then
            // re-dispateched to other live worker. It should handle timeout for bad network condition,
            // but that can be complex, not under consideration in the POC implementation)
            // TODO:
            // - should reported from worker when the mission is finished
            // - should also get information from worker complain about the long running
            // block process if it runs on non-block worker.
            Some(Ok(msg)) = framed_reader.next() => {
                handle_worker_xmessage(&msg, &mut framed_writer, &worker_table, &task_table, &worker_id).await?;
            }

            // message from task assign table lookup
            // fast-forward to the real worker client
            // then update the worker booking
            Some(imsg) = rx.recv() => {
                if let IMessage::TaskLaunch(id) = imsg {
                    let xmsg = XMessage::TaskLaunch(id);
                    framed_writer.send(xmsg).await?;
                } else {
                    anyhow::bail!("unknown msg {imsg:?} from assigner.");
                }
            }

            _ = interval.tick() => {
                println!("heartbeat Coordinator -> Worker");
                framed_writer.send(XMessage::HeartBeat(0)).await?;
            }
        }
    }
}

async fn handle_worker_xmessage<'a>(
    msg: &XMessage,
    _framed_writer: &mut FramedWrite<WriteHalf<'a>, Codec<XMessage>>,
    worker_table: &WorkerTable,
    task_table: &TaskTable,
    worker_id: &Uuid,
) -> anyhow::Result<()> {
    match msg {
        XMessage::HeartBeat(port) => {
            println!("worker {port} alive!");
        }

        // Signal direction - src: worker, dst: coordinator 
        // Handle signal submit -> run
        XMessage::TaskStateChange {
            id,
            from: task::State::Submit,
            to: task::State::Run,
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Run;
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.incr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }

        // Signal direction - src: worker, dst: coordinator 
        // Handle signal run -> complete
        XMessage::TaskStateChange {
            id,
            from: task::State::Run,
            to: task::State::Terminated(0),
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Terminated(0);
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.decr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }

        // Signal direction - src: worker, dst: coordinator 
        // Handle signal run -> except
        XMessage::TaskStateChange {
            id,
            from: task::State::Run,
            to: task::State::Terminated(x),
        } => {
            if let Some(mut task) = task_table.read(id).await {
                task.state = task::State::Terminated(*x);
                task_table.update(id, task).await?;
            }

            if let Some(mut worker) = worker_table.read(worker_id).await {
                worker.decr_load();

                worker_table.update(worker_id, worker).await?;
            }
        }
        _ => {
            println!("main.rs narrate {msg:?}");
        }
    }

    Ok(())
}
