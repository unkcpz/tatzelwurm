use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time};
use uuid::Uuid;

use crate::codec::XMessage;
use crate::worker;

#[derive(Serialize, Deserialize, Debug)]
pub enum State {
    Ready,
    Submiting,
    Running,
    Terminated,
}

#[derive(Debug)]
pub struct Task {
    pub id: Uuid,
    pub state: State,
    pub priority: u32,
    pub worker: Option<Uuid>,
}

impl Task {
    #[must_use]
    pub fn new(priority: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            state: State::Ready,
            priority,
            worker: None,
        }
    }
}

pub type Table = Arc<Mutex<HashMap<Uuid, Task>>>;

// dispatch do one thing, which is look at the task table to find all the task in state "Ready"
// and dispatch those tasks to workers.
// TODO: distinguish task type and send to the correspend worker.
// TODO: TBD if using least loaded. If the type of process is tagged.
// There should be two ways to trigger the mission dispatch,
// - one by clocking,
// - one by triggering from notifier.
// The notifier is function that called when it is sure the mission table state changed.
pub async fn dispatch(worker_table: worker::Table, task_table: Table) -> anyhow::Result<()> {
    let mut interval = time::interval(Duration::from_millis(2000));

    loop {
        interval.tick().await;

        // NOTE: a design consideration here
        // the worker_table is a snapshot and used unchanged
        // in the task loop.
        // Since when assign the task to worker, the worker table is changed onwards.
        // But here I use the static table in a single lookup.
        let worker_table = worker_table.lock().await;
        if worker_table.is_empty() {
            continue;
        }

        async {
            // TODO: should be able to pick the least load worker based on process type
            let mut task_table = task_table.lock().await;
            for (tuuid_, task) in task_table.iter_mut() {
                if let Some(act_by) = worker_table
                    .iter()
                    .min_by_key(|&(_, client)| client.load)
                    .map(|(&uuid, worker)| (uuid, worker))
                {
                    let wuuid_ = act_by.0;
                    let worker = act_by.1;

                    let xmessage = XMessage::TaskDispatch(*tuuid_);
                    if let Err(e) = worker.tx.send(xmessage).await {
                        eprintln!("Failed to send message: {e}");
                    } else {
                        // TODO: require a ack from worker and then make the table change
                        task.state = State::Submiting;
                        task.worker = Some(wuuid_);
                    }
                } else {
                    println!("no worker yet.");
                }
            }
        }
        .await;
    }
}

// lookup look at two table and construct a message send to worker.

// Mock tasks
// task 1: async sleep.
// task 2: sync sleep.
