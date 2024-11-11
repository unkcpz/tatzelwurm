use tokio::net::TcpListener;

use tatzelwurm::{actioner, task, worker};
use tatzelwurm::{
    assign::assign,
    client::{self, handshake},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;
    let worker_table = worker::Table::new();
    let task_table = task::Table::new();

    // spawn a load balance lookup task and send process to run on worker
    let worker_table_clone = worker_table.clone();
    let task_table_clone = task_table.clone();
    tokio::spawn(async move {
        let _ = assign(worker_table_clone, task_table_clone).await;
    });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");
                let worker_table_clone = worker_table.clone();
                let task_table_clone = task_table.clone();

                match handshake(stream).await {
                    Ok(client::Hook::Worker { stream }) => {
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ =
                                worker::handle(stream, worker_table_clone, task_table_clone).await;
                        });
                    }
                    Ok(client::Hook::Actioner { stream }) => {
                        tokio::spawn(async move {
                            // TODO: process_stream shouldn't return, using tracing to recording logs and
                            // handling errors
                            let _ = actioner::handle(stream, worker_table_clone, task_table_clone)
                                .await;
                        });
                    }
                    _ => {
                        // Hand shake failed, refuse connection and close stream
                        eprintln!("Hand shake refuse for {addr}");
                        // XXX: can not use mut since it is moved, how??
                        // stream.shutdown().await?;
                    }
                }
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}
