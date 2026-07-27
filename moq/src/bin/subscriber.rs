//! MoQ サブスクライバ(moq-native なし)。
//!
//! publisher へ接続し、broadcast の announce を待って "counter" track を購読し、
//! 届いた frame を表示し続ける。publisher.rs と対で動かす。
//!
//! QUIC/TLS のセットアップは moq クレートの lib(client_endpoint / connect)が
//! 肩代わりする。

use anyhow::{Context, Result};
use moq_net::Origin;

#[tokio::main]
async fn main() -> Result<()> {
    // 購読側の origin。届いた announce はここに溜まる。
    let origin = Origin::random().produce();
    let mut announced = origin.consume().announced();

    // 生 QUIC クライアント。証明書検証なし + ALPN は lib 側で設定。
    // endpoint はセッションが生きている間 drop しないよう main で保持する。
    let endpoint = moq::client_endpoint()?;

    // moqt:// は「生 QUIC 上の MoQ」を表すスキーム。
    let url: url::Url = "moqt://localhost:4443".parse()?;
    println!("[sub] connecting to {url}");

    // 接続して moq-net が扱えるセッションに包む。
    let session = moq::connect(&endpoint, url).await.context("connect failed")?;

    // MoQ ハンドシェイク。購読用 origin を渡し、driver を spawn する。
    let (session, driver) = moq_net::Client::new()
        .with_subscriber(origin.clone())
        .connect(session)
        .await?;
    tokio::spawn(driver);
    println!("[sub] connected");

    // broadcast の announce を待つ(Some=登場, None=消滅)。
    let moq_net::announce::Update { path, broadcast } =
        announced.next().await.context("origin closed")?;
    println!("[sub] announced: {}", path.as_str());
    let broadcast = broadcast.context("broadcast went offline")?;

    // "counter" track を購読する。
    let mut track = broadcast
        .track("counter")
        .context("track not found")?
        .subscribe(None)
        .await
        .context("subscribe failed")?;

    // group → frame の順に読んで表示し続ける。
    while let Some(mut group) = track.recv_group().await? {
        while let Some(frame) = group.read_frame().await? {
            println!("[sub] recv: {}", String::from_utf8_lossy(&frame.payload));
        }
    }

    drop(session);
    Ok(())
}
