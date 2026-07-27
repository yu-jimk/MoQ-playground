//! WebTransport(HTTP/3)上で動く MoQ リレー。
//!
//! # 役割
//! ライブ配信の中継点。構成は以下:
//!
//! ```text
//!   Browser(配信) ──publish live/<room>/video,audio──┐
//!                                                     ▼
//!                                              +-------------+
//!                                              |    relay    |  ← このバイナリ
//!                                              +-------------+
//!                                     ┌────────────┼────────────┐
//!                                     ▼            ▼            ▼
//!                                 Browser      Browser      Browser (視聴 + chat publish)
//! ```
//!
//! # 仕組み
//! moq-net の [`Server::with_origin`] は、1 つの `origin::Producer` を
//!   - publisher 面(`with_publisher`): 接続相手へ配信する読み出し口
//!   - subscriber 面(`with_subscriber`): 接続相手から届いた broadcast の書き込み先
//! の両方に繋ぐ。**全接続で同じ origin を clone して渡せば**、ある接続が
//! publish した broadcast が、別接続の視聴者にそのまま流れる = リレーになる。
//! Track一覧・購読者管理・キャッシュ・Fan-out は origin と driver が内部で行う。
//! リレー自身がメディアをデコードすることはない(そのまま転送するだけ)。
//!
//! # 証明書
//! ブラウザの WebTransport は自己署名証明書を `serverCertificateHashes` で
//! 受け入れられるが、条件が厳しい: **ECDSA P-256 かつ有効期間 14 日以内**。
//! ここではその条件を満たす証明書をその場で生成し、SHA-256 ハッシュ(hex)を
//! 標準出力と `RELAY_HASH_FILE`(既定 `webtransport/web/public/cert-hash.hex`)に
//! 書き出す。
//! Web アプリはそのハッシュを WebTransport のオプションに渡して接続する。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use web_transport_quinn::quinn::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use web_transport_quinn::quinn::{self, rustls};

/// リッスンアドレス(環境変数 `RELAY_ADDR` で上書き可)。
const DEFAULT_ADDR: &str = "[::]:4443";
/// keepalive 間隔。アイドルタイムアウトより十分短くする。
/// これが無いと、無通信のクライアント(接続だけして待っている視聴者など)が
/// QUIC のアイドルタイムアウトで勝手に切断されてしまう。
const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// アイドルタイムアウト。生きている接続は keepalive で保たれる。
/// クラッシュ(graceful close 無し)の相手はこの時間で切断検知される。
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// 証明書ハッシュ(hex)の書き出し先(環境変数 `RELAY_HASH_FILE` で上書き可)。
/// Web アプリ(Vite)は public/ 配下を `/` で配信するので、そこへ書く。
/// リポジトリのルートから `cargo run -p relay` する前提の相対パス。
const DEFAULT_HASH_FILE: &str = "webtransport/web/public/cert-hash.hex";

#[tokio::main]
async fn main() -> Result<()> {
    let addr: SocketAddr = std::env::var("RELAY_ADDR")
        .unwrap_or_else(|_| DEFAULT_ADDR.to_string())
        .parse()
        .context("invalid RELAY_ADDR")?;
    let hash_file = std::env::var("RELAY_HASH_FILE").unwrap_or_else(|_| DEFAULT_HASH_FILE.to_string());

    // ブラウザ向け証明書を生成し、WebTransport サーバを立てる。
    let (server, cert_hash_hex) = build_server(addr)?;

    // 証明書ハッシュを出力する。Web アプリはこれを serverCertificateHashes に渡す。
    println!("[relay] certificate sha-256 (hex): {cert_hash_hex}");
    if let Err(e) = std::fs::write(&hash_file, &cert_hash_hex) {
        eprintln!("[relay] warning: could not write {hash_file}: {e}");
    } else {
        println!("[relay] wrote hash to {hash_file}");
    }
    println!("[relay] listening on https://{addr}  (ALPN h3 / WebTransport)");

    // 全接続で共有するルート origin。これがリレーの「配信の交差点」になる。
    let origin = moq_net::Origin::random().produce();

    let mut server = server;
    // WebTransport の CONNECT を待ち受け、受理するたびにセッションを回す。
    while let Some(request) = server.accept().await {
        // ★ セッションごとに scope() で派生ハンドルを作る。
        //   clone() だと同一 Producer なので、切断しても相手が publish した
        //   broadcast が残り続ける(退出が伝わらない)。scope() の派生ハンドルは
        //   drop 時にそのセッションの broadcast を origin から撤回する。
        let session_origin = match origin.scope(&[moq_net::Path::empty()]) {
            Some(o) => o,
            None => {
                eprintln!("[relay] failed to scope origin for session");
                continue;
            }
        };
        tokio::spawn(async move {
            if let Err(e) = serve(request, session_origin).await {
                eprintln!("[relay] session error: {e}");
            }
        });
    }

    Ok(())
}

/// 1 本の WebTransport 接続を受理し、共有 origin に繋いだ MoQ セッションを回す。
async fn serve(
    request: web_transport_quinn::Request,
    origin: moq_net::origin::Producer,
) -> Result<()> {
    let remote = request.conn().remote_address();

    // WebTransport ハンドシェイクを 200 OK で完了し、セッションを得る。
    let session = request.ok().await.context("failed to accept WebTransport session")?;

    // 同じ origin を publisher 面 / subscriber 面の両方に繋ぐ = リレー動作。
    let (session, driver) = moq_net::Server::new()
        .with_origin(origin)
        .accept(session)
        .await
        .context("moq handshake failed")?;

    println!("[relay] connected: {remote}");

    // driver がプロトコル処理(Fan-out 含む)を回す。終わるまで待つ。
    let result = driver.await;
    drop(session);
    println!("[relay] disconnected: {remote}");
    result.context("session driver error")?;
    Ok(())
}

/// ブラウザ対応の自己署名証明書(ECDSA P-256 / 14 日以内)を生成し、
/// その証明書で待ち受ける WebTransport サーバと、証明書の SHA-256(hex)を返す。
fn build_server(addr: SocketAddr) -> Result<(web_transport_quinn::Server, String)> {
    // rcgen の KeyPair::generate() は既定で P-256。localhost 向けの SAN を付ける。
    let key_pair = rcgen::KeyPair::generate().context("failed to generate key pair")?;
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .context("failed to build certificate params")?;

    // 有効期間はブラウザの上限(14 日)未満に収める。余裕をみて 13 日。
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(13);

    let cert = params
        .self_signed(&key_pair)
        .context("failed to self-sign certificate")?;

    let cert_der: CertificateDer<'static> = cert.der().clone();
    let key_der = PrivatePkcs8KeyDer::from(key_pair.serialize_der());

    // 証明書の SHA-256。ブラウザの serverCertificateHashes に渡す値。
    let provider = web_transport_quinn::crypto::default_provider();
    let hash = web_transport_quinn::crypto::sha256(&provider, &cert_der);
    let cert_hash_hex = to_hex(hash.as_ref());

    // ServerBuilder は keepalive を設定できないので、endpoint を自前で組み立てる。
    // (ServerBuilder 相当の rustls/ALPN 設定 + keepalive 付き transport)
    let mut crypto = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der.into())?;
    crypto.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];

    let quic = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)
        .context("failed to build QUIC server config")?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic));

    // ★ keepalive を有効化して、無通信のクライアントも切断されないようにする。
    let mut tp = quinn::TransportConfig::default();
    tp.keep_alive_interval(Some(KEEP_ALIVE));
    tp.max_idle_timeout(Some(quinn::IdleTimeout::try_from(IDLE_TIMEOUT)?));
    server_config.transport_config(Arc::new(tp));

    let endpoint = quinn::Endpoint::server(server_config, addr)
        .context("failed to bind WebTransport server")?;
    let server = web_transport_quinn::Server::new(endpoint);

    Ok((server, cert_hash_hex))
}

/// バイト列を小文字 hex 文字列にする。
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
