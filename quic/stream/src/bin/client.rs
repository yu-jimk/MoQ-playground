//! QUIC クライアント。
//!
//! 127.0.0.1:5000 で待つサーバへ接続し、"ping" を送って
//! 返ってくる "pong" を表示するだけの最小構成。

use anyhow::Result;
use transport::make_client_config;
use quinn::Endpoint;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    // クライアント側の Endpoint。"0.0.0.0:0" はポート番号を OS に
    // 自動で割り当ててもらう指定(こちらは待ち受けないので何番でもよい)。
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;

    // 発信時に使う TLS 設定を登録する。ここでサーバの自己署名証明書を
    // 受け入れる設定(make_client_config)を渡している。
    endpoint.set_default_client_config(make_client_config()?);

    // 接続先(サーバ)のアドレス。
    let addr: SocketAddr = "127.0.0.1:5000".parse()?;

    // connect() で接続を開始し、.await でハンドシェイク完了まで待つ。
    // "localhost" は証明書の検証で使うサーバ名(SNI / ホスト名)。
    let conn = endpoint.connect(addr, "localhost")?.await?;

    println!("connected");

    // 双方向ストリームを開く。
    //   send: サーバへ書き込む側
    //   recv: サーバからの返事を読む側
    let (mut send, mut recv) = conn.open_bi().await?;

    // "ping" を送り、finish() で「送信はここまで」とストリームを終端する。
    send.write_all(b"ping").await?;
    send.finish()?;

    println!("send");

    // サーバがストリームを閉じるまで、最大 1024 バイト読む。
    let data = recv.read_to_end(1024).await?;

    println!("recv={}", String::from_utf8_lossy(&data));

    Ok(())
}
