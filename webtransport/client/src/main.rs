//! webtransport/web/ と互換な Rust の MoQ クライアント(チャット + プレゼンス)。
//!
//! relay に WebTransport で接続し、web アプリと同じチャット部屋に参加する。
//!   - 自分の broadcast `chat/<name>` の track "messages" に:
//!       * 標準入力の各行を `{"user","text"}` JSON(web と同じ)で publish
//!       * 3 秒ごとに存在確認の ping `{"user","presence":true}` を publish
//!   - announce で見つけた他人の `chat/*` を subscribe して受信する
//!   - それ以外(メディア)の broadcast は存在だけ一覧表示する(デコードはしない)
//!
//! # プレゼンス(参加者の入退室)について
//! moq-net の announce は「追加」は伝わるが、購読を保持している相手の「撤回」は
//! すぐには伝わらない(relay 内で参照が残る)。そこで **ハートビート方式** を採る:
//! 各自が定期的に ping を送り、一定時間 ping が途絶えた相手を退室とみなす。
//! これは announce の撤回に依存しないので、退室・再入室が確実に反映される。
//! web 側も同じ ping を送受信するので相互運用できる。
//!
//! # 使い方
//! ```text
//! cargo run -p relay              # 先に relay を起動
//! cargo run -p client -- <name>   # 別ターミナル。各行をチャット送信、Ctrl-D で退出
//! ```
//! 環境変数: `RELAY_URL`(既定 https://localhost:4443/)、`RELAY_NAME`。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use moq_net::{Origin, Timestamp};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use web_transport_quinn::quinn::{self, rustls};

const DEFAULT_URL: &str = "https://localhost:4443/";
/// QUIC keepalive 間隔(無通信でも接続を保つ)。
const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// QUIC アイドルタイムアウト。keepalive があるので生存接続は到達しない。
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// プレゼンス ping の送信間隔。
const PING_INTERVAL: Duration = Duration::from_secs(3);
/// この時間 ping が途絶えたら退室とみなす。
const PRESENCE_TIMEOUT: Duration = Duration::from_secs(9);

/// user -> 最終受信時刻。参加者の生存管理に使う。
type Seen = Arc<Mutex<HashMap<String, Instant>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let name = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RELAY_NAME").ok())
        .unwrap_or_else(|| "rust-user".to_string());
    let url_str = std::env::var("RELAY_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    let url: url::Url = url_str.parse().context("invalid RELAY_URL")?;

    let client = build_client().context("failed to build WebTransport client")?;
    let request = web_transport_quinn::proto::ConnectRequest::new(url.clone());
    let session = client.connect(request).await.context("WebTransport connect failed")?;

    // publisher / subscriber 兼用の origin。
    let origin = Origin::random().produce();
    // チャットのパスはセッションごとにユニークにする。同じパスを使い回すと、
    // 撤回されずに残る古い broadcast と衝突して再入室が検知できないため。
    // 参加者の識別はパスではなくメッセージ内の "user" 名で行う。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let chat_path = format!("chat/{name}/{}-{}", std::process::id(), nanos);
    let mut chat_broadcast = origin
        .create_broadcast(&chat_path, moq_net::broadcast::Route::new().with_announce(true))
        .context("failed to create chat broadcast")?;
    let mut chat_track = chat_broadcast
        .create_track("messages", None)
        .context("failed to create chat track")?;

    // announce の受け口。Consumer 本体を保持し続ける(即 drop すると取りこぼす)。
    let consumer = origin.consume();
    let announced = consumer.announced();

    let (session, driver) = moq_net::Client::new()
        .with_origin(origin.clone())
        .connect(session)
        .await
        .context("moq handshake failed")?;

    let driver_task = tokio::spawn(async move {
        if let Err(e) = driver.await {
            eprintln!("[client] セッション切断: {e:?}");
        }
    });

    println!("[client] connected to {url} as \"{name}\"");
    println!("[client] チャットを入力して Enter で送信します(Ctrl-D で退出)。");

    let seen: Seen = Arc::new(Mutex::new(HashMap::new()));

    // 発見と受信。自分の broadcast パスは購読しない。
    tokio::spawn(discover(announced, name.clone(), chat_path.clone(), seen.clone()));
    // 参加者のタイムアウト除去。
    tokio::spawn(prune(seen.clone(), name.clone()));

    // 単一の writer(元の chat_track)で、標準入力のチャットとプレゼンス ping の
    // 両方を送る。track の clone から書くと購読者に届かないので clone は使わない。
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut ping = tokio::time::interval(PING_INTERVAL);
    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line? {
                    Some(l) => {
                        let text = l.trim();
                        if !text.is_empty() {
                            let payload = serde_json::json!({ "user": name, "text": text }).to_string();
                            send_frame(&mut chat_track, payload);
                        }
                    }
                    None => break, // 標準入力 EOF(Ctrl-D)= 退出
                }
            }
            _ = ping.tick() => {
                let payload = serde_json::json!({ "user": name, "presence": true }).to_string();
                send_frame(&mut chat_track, payload);
            }
        }
    }

    // 退出: broadcast を明示的に finish して即時 unannounce にし、driver が
    // CONNECTION_CLOSE を送り切るのを待ってから終了する。
    println!("[client] 退出します…");
    let _ = chat_track.finish();
    chat_broadcast.finish();
    drop(session);
    let _ = driver_task.await;
    Ok(())
}

/// keepalive とアイドルタイムアウトを設定した WebTransport クライアント。
/// ローカル relay 相手なので証明書検証はスキップする。
fn build_client() -> Result<web_transport_quinn::Client> {
    let provider = web_transport_quinn::crypto::default_provider();
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(transport::skip_server_verification())
        .with_no_client_auth();
    crypto.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];

    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    let mut config = quinn::ClientConfig::new(Arc::new(quic));

    let mut tp = quinn::TransportConfig::default();
    tp.keep_alive_interval(Some(KEEP_ALIVE));
    tp.max_idle_timeout(Some(quinn::IdleTimeout::try_from(IDLE_TIMEOUT)?));
    config.transport_config(Arc::new(tp));

    let endpoint = quinn::Endpoint::client("[::]:0".parse()?)?;
    Ok(web_transport_quinn::Client::new(endpoint, config))
}

/// チャット track に 1 フレーム(1 グループ)書き込む。
fn send_frame(track: &mut moq_net::track::Producer, payload: String) {
    match track.append_group() {
        Ok(mut group) => {
            if group.write_frame(Timestamp::now(), payload).is_ok() {
                let _ = group.finish();
            }
        }
        Err(e) => eprintln!("[client] 送信エラー: {e}"),
    }
}

/// 一定時間 ping が来ない参加者を退室として除去する。
async fn prune(seen: Seen, me: String) {
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        let now = Instant::now();
        let mut map = seen.lock().await;
        let gone: Vec<String> = map
            .iter()
            .filter(|(u, t)| **u != me && now.duration_since(**t) > PRESENCE_TIMEOUT)
            .map(|(u, _)| u.clone())
            .collect();
        for u in gone {
            map.remove(&u);
            println!("[client] {u} が退出しました");
        }
    }
}

/// announce を監視し、`chat/*` は購読、メディアは一覧表示する。
/// 参加者名はパスではなく受信メッセージの "user" で判別するので、
/// ここではパスが `chat/` 配下かどうかだけを見る。
async fn discover(mut announced: moq_net::announce::Consumer, me: String, my_path: String, seen: Seen) {
    while let Some(update) = announced.next().await {
        let moq_net::announce::Update { path, broadcast } = update;
        let path = path.as_str().to_string();

        if path.starts_with("chat/") {
            // 自分の broadcast は購読しない。
            if path == my_path {
                continue;
            }
            if let Some(broadcast) = broadcast {
                let (me, seen) = (me.clone(), seen.clone());
                tokio::spawn(async move {
                    if let Err(e) = read_chat(broadcast, me, seen).await {
                        eprintln!("[client] read_chat error: {e:?}");
                    }
                });
            }
        } else {
            match broadcast {
                Some(_) => println!("[client] メディア配信あり: {path}"),
                None => println!("[client] メディア配信終了: {path}"),
            }
        }
    }
}

/// チャット track を購読し、chat とプレゼンス ping を処理する。
/// 参加者は各メッセージの "user" フィールドで識別する(パスに依存しない)。
async fn read_chat(broadcast: moq_net::broadcast::Consumer, me: String, seen: Seen) -> Result<()> {
    let mut track = broadcast
        .track("messages")
        .context("chat track not found")?
        .subscribe(None)
        .await
        .context("subscribe failed")?;

    // 一定時間フレームが来なければ購読を畳む(退出者の購読を溜めない)。
    loop {
        let next = tokio::time::timeout(PRESENCE_TIMEOUT + Duration::from_secs(2), track.recv_group()).await;
        let mut group = match next {
            Ok(Ok(Some(group))) => group,
            _ => break, // タイムアウト / エラー / 終了
        };
        while let Some(frame) = group.read_frame().await? {
            let raw = String::from_utf8_lossy(&frame.payload);
            let v: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(user) = v.get("user").and_then(|u| u.as_str()) else {
                continue;
            };
            // 自分の発言(echo)は無視する。
            if user == me {
                continue;
            }

            // 受信したら生存時刻を更新。初回なら参加を通知。
            {
                let mut map = seen.lock().await;
                if !map.contains_key(user) {
                    println!("[client] {user} が参加しました");
                }
                map.insert(user.to_string(), Instant::now());
            }

            // presence ping は表示しない。text があればチャットとして表示。
            if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                println!("[chat] {user}: {text}");
            }
        }
    }
    Ok(())
}
