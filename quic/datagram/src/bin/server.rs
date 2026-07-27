//! QUIC datagram サーバ。
//!
//! ストリーム版(server.rs)と違い、QUIC の「データグラム」で送受信する。
//! データグラムは順序保証も再送もない一発配信のメッセージ(暗号化された
//! UDP のようなもの)。ストリームのような開始/終端の手続きが不要で、
//! 1メッセージ = 1データグラムとして送り合う。
//!
//! ストリーム版と同時に動かせるよう、ポートは 5001 を使う。

use anyhow::Result;
use bytes::Bytes;
use transport::make_server_config;
use quinn::Endpoint;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    let addr: SocketAddr = "127.0.0.1:5001".parse()?;

    // Endpoint の作り方はストリーム版と同じ(TLS 設定を共有している)。
    let endpoint = Endpoint::server(make_server_config()?, addr)?;

    println!("waiting...");

    while let Some(connecting) = endpoint.accept().await {
        println!("connecting...");

        let conn = connecting.await?;

        println!("connected");

        tokio::spawn(async move {
            // read_datagram() は届いたデータグラムを1つ返す。
            // 接続が閉じると Err になり、ループを抜ける。
            // ストリームと違い accept_bi のような「ストリームを開く」段階はない。
            while let Ok(data) = conn.read_datagram().await {
                println!("recv={}", String::from_utf8_lossy(&data));

                // 返事もデータグラムで送る。send_datagram はキューに積むだけで
                // 即座に返る(finish のような終端処理は不要)。
                conn.send_datagram(Bytes::from_static(b"pong")).unwrap();

                println!("reply");
            }
        });
    }

    Ok(())
}
