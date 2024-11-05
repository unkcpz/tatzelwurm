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
            XMessage::ActionerOp(Operation::Submit) => {
                let task_ = Task::new(0);
                let id = task_table.create(task_.clone()).await;
                
                ///////////
                // XXX: this should be by another signal
                let mut task_ = task_.clone();
                task_.state = task::State::Ready;
                task_table.update(&id, task_).await?;
                ////////////
                
                let resp_msg = XMessage::BulkMessage(format!(
                    "{}\n{}\n",
                    worker_table.render().await,
                    task_table.render().await,
                ));
                framed_writer.send(resp_msg).await?;
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
