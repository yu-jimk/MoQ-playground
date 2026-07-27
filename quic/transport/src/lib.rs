//! client.rs / server.rs で共有する QUIC(TLS)設定をまとめたモジュール。
//!
//! QUIC は必ず TLS で暗号化される。そのため通信するには
//!   - サーバ側: 自分の証明書と秘密鍵
//!   - クライアント側: 「相手の証明書を信頼してよいか」を判断する仕組み
//! が要る。ここではローカルのループバック検証用に、証明書をその場で
//! 自己署名で作り、クライアントは検証をスキップして受け入れる。

use anyhow::Result;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::rustls;
use quinn::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use quinn::rustls::pki_types::{
    CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime,
};
use quinn::rustls::{DigitallySignedStruct, SignatureScheme};
use quinn::{ClientConfig, ServerConfig};
use std::sync::Arc;

/// サーバ設定を作る。
///
/// 通常サーバ証明書は認証局(CA)から取得するが、ローカル検証では用意が
/// 面倒なので、"localhost" 向けの証明書と秘密鍵をその場で自己署名で生成する。
pub fn make_server_config() -> Result<ServerConfig> {
    // 証明書と鍵ペアを生成する。
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;

    // rustls/quinn が扱う DER 形式に変換する。
    //   cert_der: 証明書本体
    //   key_der : 秘密鍵(PKCS#8)
    let cert_der = CertificateDer::from(cert.cert);
    let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

    // 「この証明書と鍵で待ち受ける」というサーバ設定を組み立てる。
    let server_config =
        ServerConfig::with_single_cert(vec![cert_der], key_der.into())?;

    Ok(server_config)
}

/// クライアント設定を作る。
///
/// 注意: サーバの証明書を検証せず無条件で受け入れる設定。自己署名証明書は
/// どの CA でも裏付けられないため通常は拒否されるが、ここでは検証を差し替えて
/// 受け入れている。**ローカルのループバック / 検証用途に限定**すること。
/// 実運用では正規の証明書を使い、この差し替えは外すこと。
pub fn make_client_config() -> Result<ClientConfig> {
    // rustls のクライアント設定を作る。
    //   .dangerous().with_custom_certificate_verifier(...) で
    //   証明書の検証ロジックを自前の SkipServerVerification に差し替える。
    //   .with_no_client_auth() はクライアント証明書を出さない(サーバのみ認証)。
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    // rustls の設定を QUIC 用にラップして quinn の ClientConfig にする。
    let client_config =
        ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));

    Ok(client_config)
}

/// 証明書検証をスキップする検証器を返す。
///
/// ALPN を自分で設定するなど、rustls の `ClientConfig` を直接組み立てたい
/// コード(moq クレートなど)向け。用途と注意は [`make_client_config`] と同じ。
pub fn skip_server_verification() -> Arc<dyn ServerCertVerifier> {
    SkipServerVerification::new()
}

/// サーバ証明書の検証をスキップするカスタム検証器。
///
/// 保持している `CryptoProvider` は、下の署名検証(verify_tls1x_signature)を
/// 標準ロジックに委譲するために使う。
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        // ring(暗号ライブラリ)の標準プロバイダを使う。
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl ServerCertVerifier for SkipServerVerification {
    /// 証明書チェーンそのものの検証。ここが肝で、中身を一切見ずに
    /// 「検証OK」を返すことで自己署名証明書を受け入れている。
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    /// TLS 1.2 ハンドシェイクの署名検証。証明書自体は信頼すると決めたが、
    /// 「その鍵で正しく署名されているか」は標準ロジックで確認する。
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    /// TLS 1.3 版の署名検証(中身は 1.2 と同様)。
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    /// 対応している署名方式の一覧。プロバイダが対応するものをそのまま返す。
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
