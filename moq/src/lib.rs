//! moq-native を使わずに MoQ の接続を確立するための最小限のヘルパ。
//!
//! moq-net の `Client` / `Server` は「`web_transport_trait::Session` を実装した
//! 何か」を受け取ってハンドシェイクする。moq-native はその「何か」を各種
//! トランスポート向けに用意してくれるヘルパだった。ここではそのうち
//! **生 QUIC(moqt)** の1経路だけを、quinn + web-transport-quinn で自前で組む。
//!
//! 生 QUIC では「MoQ のバージョン = ALPN」。ALPN でバージョンをネゴシエートし、
//! 確立した quinn 接続を `web_transport_quinn::Session::raw` で
//! web-transport セッションに“見せかけて”moq-net に渡す。

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use quinn::rustls;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

/// 使用する MoQ バージョン。生 QUIC ではこれが ALPN に対応する
/// (draft-19 の実際の ALPN 文字列は "moqt-19")。
pub const MOQ_VERSION: &str = "moq-transport-19";

/// [`MOQ_VERSION`] だけを含むバージョン集合。
fn versions() -> moq_net::Versions {
    let version: moq_net::Version = MOQ_VERSION.parse().expect("valid moq version");
    version.into()
}

/// rustls の `alpn_protocols` に渡す ALPN 一覧(バイト列)。
fn alpn_protocols() -> Vec<Vec<u8>> {
    versions()
        .alpns()
        .iter()
        .map(|alpn| alpn.as_bytes().to_vec())
        .collect()
}

/// ring ベースの暗号プロバイダ。client/server の rustls 設定で共通に使う。
fn ring_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// クライアント用エンドポイントを作る(証明書検証なし + MoQ の ALPN)。
///
/// 返した `Endpoint` は接続が生きている間 drop しないこと(drop すると
/// 接続の駆動が止まる)。呼び出し側が main 等で保持する想定。
pub fn client_endpoint() -> Result<quinn::Endpoint> {
    // 証明書検証をスキップし、ALPN に MoQ バージョンを載せた rustls 設定。
    let mut crypto = rustls::ClientConfig::builder_with_provider(ring_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(transport::skip_server_verification())
        .with_no_client_auth();
    crypto.alpn_protocols = alpn_protocols();

    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    // localhost は ::1(IPv6)に解決されることが多いので v6 で bind する。
    let mut endpoint = quinn::Endpoint::client("[::]:0".parse()?)?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic)));
    Ok(endpoint)
}

/// 生 QUIC で接続し、moq-net が扱える web-transport セッションに包んで返す。
pub async fn connect(endpoint: &quinn::Endpoint, url: url::Url) -> Result<web_transport_quinn::Session> {
    let host = url.host_str().context("url has no host")?.to_string();
    let port = url.port().unwrap_or(443);

    // 名前解決し、クライアントソケットと同じアドレスファミリを優先して1つ選ぶ
    // (v6 ソケットから v4 アドレスへ繋ぎに行くと失敗するため)。
    let local = endpoint.local_addr()?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.clone(), port)).await?.collect();
    let addr = addrs
        .iter()
        .copied()
        .find(|a| a.is_ipv6() == local.is_ipv6())
        .or_else(|| addrs.first().copied())
        .context("no address resolved")?;

    // SNI / 証明書検証名には URL のホスト名を使う。
    let connection = endpoint.connect(addr, &host)?.await?;
    let alpn = negotiated_alpn(&connection)?;

    // 生 QUIC を web-transport セッションに“見せかける”(moq-native と同じ手口)。
    let request = web_transport_quinn::proto::ConnectRequest::new(url).with_protocol(alpn.clone());
    let response = web_transport_quinn::proto::ConnectResponse::OK.with_protocol(alpn);
    Ok(web_transport_quinn::Session::raw(connection, request, response))
}

/// 確立済み接続がネゴシエートした ALPN 文字列を取り出す。
fn negotiated_alpn(connection: &quinn::Connection) -> Result<String> {
    let handshake = connection
        .handshake_data()
        .context("missing handshake data")?
        .downcast::<quinn::crypto::rustls::HandshakeData>()
        .ok()
        .context("unexpected handshake data type")?;
    let alpn = handshake.protocol.context("no ALPN negotiated")?;
    Ok(String::from_utf8(alpn)?)
}

/// サーバ用エンドポイントを作る(自己署名証明書を生成 + MoQ の ALPN)。
pub fn server_endpoint(bind: SocketAddr) -> Result<quinn::Endpoint> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_der = CertificateDer::from(cert.cert);
    let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

    let mut crypto = rustls::ServerConfig::builder_with_provider(ring_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der.into())?;
    crypto.alpn_protocols = alpn_protocols();

    let quic = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
    let endpoint =
        quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(quic)), bind)?;
    Ok(endpoint)
}

/// 受け入れた QUIC 接続を moq-net が扱える web-transport セッションに包む。
pub async fn accept(incoming: quinn::Incoming) -> Result<web_transport_quinn::Session> {
    let mut connecting = incoming.accept()?;

    // ALPN と SNI はハンドシェイク完了前(この段階)で取得できる。
    let handshake = connecting
        .handshake_data()
        .await?
        .downcast::<quinn::crypto::rustls::HandshakeData>()
        .ok()
        .context("unexpected handshake data type")?;
    let alpn = handshake.protocol.context("no ALPN negotiated")?;
    let alpn = String::from_utf8(alpn)?;
    let host = handshake.server_name.unwrap_or_default();

    let connection = connecting.await?;

    // 生 QUIC には WebTransport の CONNECT URL が無いので SNI から URL を作る。
    let host_str = if host.contains(':') { format!("[{host}]") } else { host };
    let url: url::Url = format!("moqt://{host_str}")
        .parse()
        .unwrap_or_else(|_| "moqt://localhost".parse().expect("valid fallback url"));

    let request = web_transport_quinn::proto::ConnectRequest::new(url);
    let response = web_transport_quinn::proto::ConnectResponse::OK.with_protocol(alpn);
    Ok(web_transport_quinn::Session::raw(connection, request, response))
}
