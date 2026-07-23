use anyhow::Result;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::rustls;
use quinn::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use quinn::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use quinn::rustls::{DigitallySignedStruct, SignatureScheme};
use quinn::{ClientConfig, Endpoint};
use std::net::SocketAddr;
use std::sync::Arc;

/// ループバック用: サーバの自己署名証明書を検証せず受け入れる。
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl ServerCertVerifier for SkipServerVerification {
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

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn make_client_config() -> Result<ClientConfig> {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    let client_config =
        ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));

    Ok(client_config)
}

#[tokio::main]
async fn main() -> Result<()> {

    let mut endpoint =
        Endpoint::client("0.0.0.0:0".parse()?)?;

    endpoint.set_default_client_config(make_client_config()?);

    let addr: SocketAddr = "127.0.0.1:5000".parse()?;

    let conn = endpoint
        .connect(addr, "localhost")?
        .await?;

    println!("connected");

    let (mut send, mut recv) =
        conn.open_bi().await?;

    send.write_all(b"ping").await?;
    send.finish()?;

    println!("send");

    let data =
        recv.read_to_end(1024).await?;

    println!(
        "recv={}",
        String::from_utf8_lossy(&data)
    );

    Ok(())
}
