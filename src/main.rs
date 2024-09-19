use std::io::Cursor;

use bytes::{Buf, Bytes, BytesMut};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("incomplete stream to parse as a frame")]
    InComplete
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5677").await?;

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                // TODO: spawn task for handling every client
                println!("Client listen on: {addr}");
                process_stream(socket).await?;
            }
            Err(err) => println!("client cannot has connection established {err:?}"),
        }
    }
}

async fn process_stream(mut stream: TcpStream) -> anyhow::Result<()> {
    use StreamError::InComplete;

    let mut buf = BytesMut::with_capacity(1024);

    loop {
        // TODO: deal with read timeout
        match stream.read_buf(&mut buf).await {
            Ok(0) => break,
            Ok(_) => {
                // wrap in cursor to seek the len to advance and discard
                let mut src = Cursor::new(&buf[..]);

                match check_frame(&mut src) {
                    Ok(_) => {
                        // since check frame will move cursor position forward, 
                        // I store the position for advancing buffer to discard.
                        let len = src.position() as usize;

                        // reset seek pos to 0 to parse
                        src.set_position(0);
                        let frame = parse_frame(src);

                        // discard bytes that already read
                        buf.advance(len);
                    }
                    Err(InComplete) => continue,
                    Err(err) => anyhow::bail!("check frame failed, {err:?}"),
                }
            }
            Err(err) => anyhow::bail!("unable to handle stream, {err:?}"),
        }
    }
    Ok(())
}

fn check_frame(src: &mut Cursor<&[u8]>) -> Result<(), StreamError> {
    todo!()
}

// TODO: depend on use case maybe no need to use different frame type
// Now the idea is borrowed from redis.
enum Frame {
    BulkString(Bytes) 
}

fn parse_frame(src: Cursor<&[u8]>) -> Result<Frame, StreamError> {
    todo!()
}
