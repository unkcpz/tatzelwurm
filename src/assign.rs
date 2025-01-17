use std::time::Duration;

use tokio::task::yield_now;
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
            let ready_tasks = task_table.filter_by_states(vec![task::State::Ready]).await;

            for (task_id, _) in ready_tasks {
                let Some(mut task) = task_table.read(&task_id).await else {
                    continue;
                };

                // TODO: should be able to pick the least load worker based on process type
                let Some(worker_id) = worker_table.find_least_loaded_worker().await else {
                    println!("No worker yet available");
                    continue;
                };

                let Some(worker) = worker_table.read(&worker_id).await else {
                    continue;
                };

                if let Err(e) = worker.launch_task(&task_id, &task).await {
                    eprintln!("Failed to send message: {e}");
                    continue;
                }

                // TODO: require a ack from worker and then make the table change
                task.state = task::State::Submit;
                task.worker = Some(worker_id);
                let _ = task_table.update(&task_id, task).await;

                // relinquish the control for this tight loop
                yield_now().await;
            }
        }
        .await;
    }
}
