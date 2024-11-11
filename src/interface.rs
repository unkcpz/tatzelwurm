use crate::codec::{Codec, XMessage};

use futures::SinkExt;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

#[derive(Debug)]
pub enum ClientType {
    Worker,
    Actioner, 
}

pub async fn handshake(stream: TcpStream, client_t: ClientType) -> anyhow::Result<TcpStream> {
    let mut framed = Framed::new(stream, Codec::<XMessage>::new());

    loop {
        if let Some(Ok(message)) = framed.next().await {
            match message {
                XMessage::HandShake(info) => match info.as_str() {
                    "Go" => {
                        println!("handshake successful!");
                        break;
                    }
                    "Who you are?" => match client_t {
                        ClientType::Worker => framed
                            .send(XMessage::HandShake("worker".to_string()))
                            .await?,
                        ClientType::Actioner => framed
                            .send(XMessage::HandShake("actioner".to_string()))
                            .await?,
                    }
                    _ => eprintln!("unknown handshake info: {info}"),
                },
                _ => eprintln!("unknown message: {message:#?}"),
            }
        }
    }

    Ok(framed.into_inner())
}
