use tokio::{io::AsyncWriteExt, net::TcpStream};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:5677").await?;
    let n = stream.write_all(b"$11\r\nhello boss!\r\n").await?;

    Ok(())
}
