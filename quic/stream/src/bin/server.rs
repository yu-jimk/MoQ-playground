//! QUIC サーバ。
//!
//! ローカルの 127.0.0.1:5000 で待ち受け、クライアントから届いた
//! メッセージ("ping")を受け取って "pong" を返すだけの最小構成。

use anyhow::Result;
use transport::make_server_config;
use quinn::Endpoint;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    // 待ち受けるアドレス(ローカルホストの 5000 番ポート)。
    let addr: SocketAddr = "127.0.0.1:5000".parse()?;

    // Endpoint は QUIC 通信の入り口(内部的には1つの UDP ソケット)。
    // make_server_config() で TLS 証明書などのサーバ設定を渡して起動する。
    let endpoint = Endpoint::server(make_server_config()?, addr)?;

    println!("waiting...");

    // accept() は新しい接続要求を1つずつ返す。相手が来るまで待つ。
    while let Some(connecting) = endpoint.accept().await {
        println!("connecting...");

        // ハンドシェイク(TLS の鍵交換など)の完了を待って接続を確立する。
        let conn = connecting.await?;

        println!("connected");

        // 1接続ごとに別タスクで処理する。こうすると複数クライアントを
        // 同時にさばける(このタスクが動いている間、下の while はすぐ次の
        // 接続待ちに戻れる)。
        tokio::spawn(async move {
            // accept_bi() は相手(クライアント)が開いた双方向ストリームを返す。
            //   send: こちらから相手へ書き込む側
            //   recv: 相手から届いたデータを読む側
            while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                // 相手がストリームを閉じる(finish)まで、最大 1024 バイト読む。
                let data = recv.read_to_end(1024).await.unwrap();

                println!("recv={}", String::from_utf8_lossy(&data));

                // 返事を書き込み、finish() でストリームを終端する。
                // finish() を呼ばないと、相手側の read_to_end が完了しない。
                send.write_all(b"pong").await.unwrap();
                send.finish().unwrap();

                println!("reply");
            }
        });
    }

    Ok(())
}
