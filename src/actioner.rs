use futures::SinkExt;
use tokio::net::TcpStream;

use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::codec::Operation;
use crate::task::State;
use crate::{
    codec::Codec,
    task,
    worker::{self},
};
use crate::{codec::XMessage, task::Task};

pub async fn handle(
    mut stream: TcpStream,
    worker_table: worker::Table,
    task_table: task::Table,
) -> anyhow::Result<()> {
    // TODO: check can I use borrowed halves if no moves of half to spawn
    let (read_half, write_half) = stream.split();

    let mut framed_reader = FramedRead::new(read_half, Codec::<XMessage>::new());
    let mut framed_writer = FramedWrite::new(write_half, Codec::<XMessage>::new());

    // message from worker client
    // this contains heartbeat (only access table when worker dead, the mission then
    // re-dispateched to other live worker. It should handle timeout for bad network condition,
    // but that can be complex, not under consideration in the POC implementation)
    // TODO:
    // - should reported from worker when the mission is finished
    // - should also get information from worker complain about the long running
    // block process if it runs on non-block worker.
    loop {
        if let Some(Ok(msg)) = framed_reader.next().await {
            match msg {
                // You say over, I say over
                XMessage::Over => {
                    framed_writer.send(XMessage::Over).await?;
                    break;
                }

                XMessage::WorkerTablePrint => {
                    let rtable = worker_table.render().await.to_string();
                    let resp_msg = XMessage::BulkMessage(rtable);
                    framed_writer.send(resp_msg).await?;
                }

                XMessage::TaskTablePrint { states } => {
                    let count_info = task_table.count().await;
                    let count_info = format!("created: {}, ready: {}, submit: {}, pause: {}, run: {}, complete: {}, killed: {}.",
                    count_info.get(&State::Created).unwrap_or(&0),
                    count_info.get(&State::Ready).unwrap_or(&0),
                    count_info.get(&State::Submit).unwrap_or(&0),
                    count_info.get(&State::Pause).unwrap_or(&0),
                    count_info.get(&State::Run).unwrap_or(&0),
                    count_info.get(&State::Terminated(0)).unwrap_or(&0),
                    count_info.get(&State::Terminated(-1)).unwrap_or(&0),
                );
                    let tasks = task_table.filter_by_states(states).await;
                    let task_table = task::Table::from_mapping(tasks);
                    let resp_msg = XMessage::BulkMessage(format!(
                        "{}\n\n{}",
                        task_table.render().await,
                        count_info,
                    ));
                    framed_writer.send(resp_msg).await?;
                }

                // Signal direction - src: actioner, dst: coordinator
                // Handle signal n/a -> Created
                XMessage::ActionerOp(Operation::AddTask(record_id)) => {
                    // TODO: need to check if the task exist
                    // TODO: priority passed from operation
                    let task_ = Task::new(0, &record_id);
                    let id = task_table.create(task_.clone()).await;

                    // send resp to actioner
                    let resp_msg = XMessage::BulkMessage(format!(
                        "Add task id={id}, map to task record_id={record_id} to run."
                    ));
                    framed_writer.send(resp_msg).await?;
                }
                // Signal direction - src: actioner, dst: coordinator
                // Handle signal x -> Ready
                XMessage::ActionerOp(Operation::PlayTask(id)) => {
                    // TODO: need to check init state is able to be played
                    let task_ = task_table.read(&id).await;
                    if let Some(mut task_) = task_ {
                        task_.state = task::State::Ready;
                        task_table.update(&id, task_).await?;
                    }

                    let resp_msg = XMessage::BulkMessage(format!("Launching task uuid={id}.",));
                    framed_writer.send(resp_msg).await?;
                }
                // Signal direction - src: actioner, dst: coordinator
                // Handle signal all pause/created x -> Ready
                XMessage::ActionerOp(Operation::PlayAllTask) => {
                    // TODO: also include pause state to resume
                    let resumable_tasks = task_table
                        .filter_by_states(vec![task::State::Created])
                        .await;

                    for (task_id, _) in resumable_tasks {
                        let Some(mut task_) = task_table.read(&task_id).await else {
                            continue;
                        };
                        // XXX: check, is cloned?? so the old_state is different from after changed
                        let old_state = task_.state;

                        task_.state = task::State::Ready;
                        task_table.update(&task_id, task_).await?;
                        println!(
                            "Play task {task_id}: {} -> {}",
                            old_state,
                            task::State::Ready
                        );
                    }
                }
                // Signal direction - src: actioner, dst: coordinator
                // Handle signal x -> Terminated(-1)
                XMessage::ActionerOp(Operation::KillTask(id)) => {
                    let task_ = task_table.read(&id).await;
                    if let Some(mut task_) = task_ {
                        task_.state = task::State::Terminated(-1);
                        task_table.update(&id, task_).await?;

                        // TODO: also sending a cancelling signal to the runnning task on worker

                        let resp_msg = XMessage::BulkMessage(format!("Kill task uuid={id}.\n",));
                        framed_writer.send(resp_msg).await?;
                    }
                }

                // boss is asking nonsense
                _ => {
                    let resp_msg = XMessage::BulkMessage(format!(
                        "Shutup, I try to ignore you, since you say '{msg:#?}'"
                    ));
                    framed_writer.send(resp_msg).await?;
                }
            }
        }
    }

    Ok(())
}
