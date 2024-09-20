use std::io::Cursor;

use atoi::atoi;
use bytes::{Buf, Bytes, BytesMut};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("incomplete stream to parse as a frame")]
    InComplete,
    #[error("unable to parse {0:?}")]
    ParseError(Bytes),
    #[error("unknown data type leading with byte {0:?}")]
    UnknownType(u8),
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
                    Ok(()) => {
                        // since check frame will move cursor position forward,
                        // I store the position for advancing buffer to discard.
                        let len = src.position() as usize;

                        // reset seek pos to 0 to parse
                        src.set_position(0);
                        let frame = parse_frame(&mut src);

                        dbg!(frame);

                        // discard bytes that already read
                        buf.advance(len);
                    }
                    Err(InComplete) => continue,
                    Err(err) => anyhow::bail!("check frame failed, {err}"),
                }
            }
            Err(err) => anyhow::bail!("unable to handle stream, {err:?}"),
        }
    }
    Ok(())
}

fn check_frame(src: &mut Cursor<&[u8]>) -> Result<(), StreamError> {
    // TODO: deal with src already empty
    match src.get_u8() {
        b'$' => {
            // bulk string https://redis.io/docs/latest/develop/reference/protocol-spec/#bulk-strings
            let line = get_line(src)?;
            let len = atoi::<usize>(line)
                .ok_or_else(|| StreamError::ParseError(Bytes::from(line.to_owned())))?;

            if src.remaining() < len + 2 {
                return Err(StreamError::InComplete);
            }

            src.advance(len + 2);

            Ok(())
        }
        b => Err(StreamError::UnknownType(b)),
    }
}

fn get_line<'a>(src: &mut Cursor<&'a [u8]>) -> Result<&'a [u8], StreamError> {
    let start = src.position() as usize;
    let end = src.get_ref().len() - 1;

    for i in start..end {
        if src.get_ref()[i] == b'\r' && src.get_ref()[i + 1] == b'\n' {
            // step and consume in cursor
            src.set_position((i + 2) as u64);

            return Ok(&src.get_ref()[start..i]);
        }
    }

    Err(StreamError::InComplete)
}

// TODO: depend on use case maybe no need to use different frame type
// Now the idea is borrowed from redis.
#[derive(Debug)]
enum Frame {
    BulkString(Bytes),
}

fn parse_frame(src: &mut Cursor<&[u8]>) -> Result<Frame, StreamError> {
    match src.get_u8() {
        b'$' => {
            let line = get_line(src)?;
            let len = atoi::<usize>(line)
                .ok_or_else(|| StreamError::ParseError(Bytes::from(line.to_owned())))?;

            if src.remaining() < len + 2 {
                return Err(StreamError::InComplete);
            }

            let data = Bytes::copy_from_slice(&src.chunk()[..len]);

            Ok(Frame::BulkString(data))
        }
        _ => todo!(),
    }
}
