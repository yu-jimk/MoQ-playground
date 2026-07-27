//! MoQ パブリッシャ(moq-native なし)。
//!
//! サーバを立て、"demo" broadcast の中の "counter" track に 1秒ごとにカウンタ
//! 値を配信する。接続してきた subscriber にこの broadcast を publish する。
//!
//! MoQ の階層(上から):
//!   origin(配信元の識別子)
//!     └ broadcast("demo")    … 配信の単位。announce で存在を知らせる
//!         └ track("counter") … メディアの1系統
//!             └ group        … まとまり(ここでは1カウント=1グループ)
//!                 └ frame     … 実データ("count=N")
//!
//! QUIC/TLS のセットアップは moq クレートの lib(server_endpoint / accept)が
//! 肩代わりする。ここは MoQ のモデル操作とセッション確立だけに集中する。

use std::time::Duration;

use anyhow::{Context, Result};
use moq_net::{Origin, Timestamp};

#[tokio::main]
async fn main() -> Result<()> {
    // origin 配下に broadcast と track を用意する。
    let origin = Origin::random().produce();
    let mut broadcast = origin
        .create_broadcast("demo", moq_net::broadcast::Route::new().with_announce(true))
        .context("failed to create broadcast")?;
    let mut track = broadcast
        .create_track("counter", None)
        .context("failed to create track")?;

    // 1秒ごとにカウンタを1グループ書き込む(track をこのタスクで生かし続ける)。
    tokio::spawn(async move {
        let mut n: u64 = 0;
        loop {
            let mut group = match track.append_group() {
                Ok(group) => group,
                Err(e) => {
                    eprintln!("[pub] append_group error: {e}");
                    break;
                }
            };
            if let Err(e) = group.write_frame(Timestamp::now(), format!("count={n}")) {
                eprintln!("[pub] write_frame error: {e}");
                break;
            }
            let _ = group.finish();

            println!("[pub] wrote count={n}");
            n += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // 生 QUIC サーバを起動(自己署名証明書と ALPN は lib 側で設定)。
    // localhost は ::1 に解決されがちなのでデュアルスタック [::] で待ち受ける。
    let endpoint = moq::server_endpoint("[::]:4443".parse()?)?;
    println!("[pub] listening on {}", endpoint.local_addr()?);

    // 接続を受けるたびに publish 用 origin を渡してセッションを確立する。
    while let Some(incoming) = endpoint.accept().await {
        let origin = origin.clone();
        tokio::spawn(async move {
            if let Err(e) = serve(incoming, origin).await {
                eprintln!("[pub] connection error: {e}");
            }
        });
    }

    Ok(())
}

/// 1接続を受理し、MoQ サーバセッションとして publish する。
async fn serve(incoming: quinn::Incoming, origin: moq_net::origin::Producer) -> Result<()> {
    // 生 QUIC 接続を moq-net が扱えるセッションに包む。
    let session = moq::accept(incoming).await?;

    // MoQ ハンドシェイク。driver がプロトコル処理を回すので spawn する。
    let (session, driver) = moq_net::Server::new()
        .with_publisher(&origin)
        .accept(session)
        .await?;
    tokio::spawn(driver);

    println!("[pub] subscriber connected");
    let _ = session.closed().await;
    println!("[pub] subscriber disconnected");
    Ok(())
}
