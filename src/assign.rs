use std::time::Duration;

use tokio::time;

use crate::task::{self, Table as TaskTable};
use crate::worker::Table as WorkerTable;

// dispatch do one thing, which is look at the task table to find all the task in state "Ready"
// and dispatch those tasks to workers.
// TODO: distinguish task type and send to the correspend worker.
// TODO: TBD if using least loaded. If the type of process is tagged.
// There should be two ways to trigger the mission dispatch,
// - one by clocking,
// - one by triggering from notifier.
// The notifier is function that called when it is sure the mission table state changed.
pub async fn assign(worker_table: WorkerTable, task_table: TaskTable) -> anyhow::Result<()> {
    let mut interval = time::interval(Duration::from_millis(2000));

    loop {
        interval.tick().await;

        // NOTE: a design consideration here
        // the worker_table is a snapshot and used unchanged
        // in the task loop.
        // Since when assign the task to worker, the worker table is changed onwards.
        // But here I use the static table in a single lookup.

        async {
            // TODO: inner should be private and should impl Iter 
            for (task_id, _) in task_table.inner.lock().await.iter() {
                let mut task = task_table.read(task_id).await.unwrap();

                if task.state != task::State::Ready {
                    continue;
                }

                // TODO: should be able to pick the least load worker based on process type
                if let Some(worker_id) = worker_table.find_least_loaded_worker().await {
                    if let Some(worker) = worker_table.read(&worker_id).await {
                        if let Err(e) = worker.launch_task(task_id).await {
                            eprintln!("Failed to send message: {e}");
                        } else {
                            // TODO: require a ack from worker and then make the table change
                            task.state = task::State::Submit;
                            task.worker = Some(worker_id);
                            let _ = task_table.update(task_id, task).await;
                        }
                    }
                } else {
                    println!("no worker yet.");
                }
            }
        }
        .await;
    }
}
