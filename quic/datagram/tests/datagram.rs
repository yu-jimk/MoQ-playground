use anyhow::Result;
use bytes::Bytes;
use transport::{make_client_config, make_server_config};
use quinn::Endpoint;
use std::net::SocketAddr;

/// サーバとクライアントを同一プロセス内で起動し、データグラムでの
/// ping/pong の往復を検証する。
#[tokio::test]
async fn ping_pong_datagram() -> Result<()> {
    // OS に空きポートを割り当てさせる
    let addr: SocketAddr = "127.0.0.1:0".parse()?;
    let server = Endpoint::server(make_server_config()?, addr)?;
    let server_addr = server.local_addr()?;

    // サーバ: 1つ受け取って pong を返す
    let server_task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap().await.unwrap();
        let data = conn.read_datagram().await.unwrap();
        assert_eq!(&data[..], b"ping");
        conn.send_datagram(Bytes::from_static(b"pong")).unwrap();
        // クライアントが受け取るまで接続を維持
        conn.closed().await;
    });

    // クライアント: ping を送って pong を受け取る
    let mut client = Endpoint::client("127.0.0.1:0".parse()?)?;
    client.set_default_client_config(make_client_config()?);

    let conn = client.connect(server_addr, "localhost")?.await?;
    conn.send_datagram(Bytes::from_static(b"ping"))?;

    let data = conn.read_datagram().await?;
    assert_eq!(&data[..], b"pong");

    conn.close(0u32.into(), b"done");
    server_task.await?;

    Ok(())
}
