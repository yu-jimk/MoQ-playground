//! QUIC datagram クライアント。
//!
//! datagram_server(127.0.0.1:5001)へ接続し、"ping" をデータグラムで
//! 送って、返ってくる "pong" のデータグラムを表示する。

use anyhow::Result;
use bytes::Bytes;
use transport::make_client_config;
use quinn::Endpoint;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(make_client_config()?);

    // 接続の確立まではストリーム版と全く同じ。違うのは送受信のやり方だけ。
    let addr: SocketAddr = "127.0.0.1:5001".parse()?;
    let conn = endpoint.connect(addr, "localhost")?.await?;

    println!("connected");

    // "ping" をデータグラムで送る。ストリームのような open_bi / finish は不要。
    conn.send_datagram(Bytes::from_static(b"ping"))?;

    println!("send");

    // 返事のデータグラムを1つ待つ。
    let data = conn.read_datagram().await?;

    println!("recv={}", String::from_utf8_lossy(&data));

    Ok(())
}
