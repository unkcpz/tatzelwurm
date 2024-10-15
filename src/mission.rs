use std::time::Duration;

use tokio::time;

use crate::{codec::TMessage, worker::ClientMap};

// TODO: TBD if using least loaded. If the type of process is tagged.
// There should be two ways to trigger the mission dispatch,
// - one by clocking,
// - one by triggering from notifier.
// The notifier is function that called when it is sure the mission table state changed.
pub async fn dispatch(client_map: ClientMap) -> anyhow::Result<()> {
    let mut interval = time::interval(Duration::from_millis(2000));

    loop {
        interval.tick().await;
        // dbg!(&client_map);

        async {
            if let Some(act_on) = client_map
                .lock()
                .await
                .iter()
                .min_by_key(|&(_, client)| client.load)
                .map(|(&uuid, worker)| (uuid, worker))
            {
                let uuid_ = act_on.0;
                let worker = act_on.1;

                let message = TMessage {
                    id: 3,
                    content: format!("processed by worker {uuid_}"),
                };
                if let Err(e) = worker.tx.send(message).await {
                    eprintln!("Failed to send message: {e}");
                }
            } else {
                println!("no worker yet.");
            }
        }
        .await;
    }
}
