use futures::SinkExt;
use tokio::net::TcpStream;

use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

use crate::codec::Codec;
use crate::codec::XMessage;

pub enum Hook {
    Worker { stream: TcpStream },
    Actioner { stream: TcpStream },
}

// Perform handshake, decide client type (worker or actioner) and protocol (always messagepack)
// XXX: hyperequeue seems doesn't have handshake stage, how??
pub async fn handshake(stream: TcpStream) -> anyhow::Result<Hook> {
    let mut frame = Framed::new(stream, Codec::<XMessage>::new());

    frame
        .send(XMessage::HandShake("Who you are?".to_string()))
        .await?;

    let client = if let Some(Ok(message)) = frame.next().await {
        match message {
            XMessage::HandShake(info) => match info.as_str() {
                "worker" => {
                    frame.send(XMessage::HandShake("Go".to_string())).await?;
                    let stream = frame.into_inner();
                    Hook::Worker { stream }
                }
                "actioner" => {
                    frame.send(XMessage::HandShake("Go".to_string())).await?;
                    let stream = frame.into_inner();
                    Hook::Actioner { stream }
                }
                _ => anyhow::bail!("unknown client: {info:#?}"),
            },
            _ => anyhow::bail!("unknown message: {message:#?}"),
        }
    } else {
        anyhow::bail!("fail handshake");
    };

    Ok(client)
}
