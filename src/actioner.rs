use futures::SinkExt;
use tokio::net::TcpStream;

use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::codec::Operation;
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
    if let Some(Ok(msg)) = framed_reader.next().await {
        match msg {
            XMessage::PrintTable() => {
                let resp_msg = XMessage::BulkMessage(format!(
                    "{}\n\n{}\n",
                    worker_table.render().await,
                    task_table.render().await,
                ));
                framed_writer.send(resp_msg).await?;
            }

            // Signal direction - src: actioner, dst: coordinator
            // Handle signal n/a -> Created
            XMessage::ActionerOp(Operation::AddTask) => {
                let task_ = Task::new(0);
                task_table.create(task_.clone()).await;

                let resp_msg = XMessage::BulkMessage(format!(
                    "{}\n{}\n",
                    worker_table.render().await,
                    task_table.render().await,
                ));
                framed_writer.send(resp_msg).await?;
            }
            // Signal direction - src: actioner, dst: coordinator
            // Handle signal x -> Ready
            XMessage::ActionerOp(Operation::PlayTask(id)) => {
                let task_ = task_table.read(&id).await;
                if let Some(mut task_) = task_ {
                    task_.state = task::State::Ready;
                    task_table.update(&id, task_).await?;

                    let resp_msg = XMessage::BulkMessage(format!(
                        "{}\n{}\n",
                        worker_table.render().await,
                        task_table.render().await,
                    ));
                    framed_writer.send(resp_msg).await?;
                }
            }
            // Signal direction - src: actioner, dst: coordinator
            // Handle signal all pause/created x -> Ready
            XMessage::ActionerOp(Operation::PlayAllTask) => {
                // TODO: also include pause state to resume
                let resumable_tasks = task_table.filter_by_state(task::State::Created).await;

                for (task_id, _) in resumable_tasks {
                    let Some(mut task_) = task_table.read(&task_id).await else {
                        continue;
                    };
                    // XXX: check, is cloned?? so the old_state is different from after changed
                    let old_state = task_.state;

                    task_.state = task::State::Ready;
                    task_table.update(&task_id, task_).await?;
                    println!("Play task {task_id}: {} -> {}", old_state, task::State::Ready);
                }
            }
            _ => {
                let resp_msg = XMessage::BulkMessage(format!(
                    "Shutup, I try to ignore you, since you say '{msg:#?}'"
                ));
                framed_writer.send(resp_msg).await?;
            }
        }
    }

    Ok(())
}
