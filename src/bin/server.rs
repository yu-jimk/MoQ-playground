use anyhow::Result;
use quinn::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use quinn::{Endpoint, ServerConfig};
use std::net::SocketAddr;

fn make_server_config() -> Result<ServerConfig> {
    // ループバック用に自己署名証明書をその場で生成する
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;

    let cert_der = CertificateDer::from(cert.cert);
    let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

    let server_config =
        ServerConfig::with_single_cert(vec![cert_der], key_der.into())?;

    Ok(server_config)
}

#[tokio::main]
async fn main() -> Result<()> {

    let addr: SocketAddr =
        "127.0.0.1:5000".parse()?;

    let endpoint =
        Endpoint::server(make_server_config()?, addr)?;

    println!("waiting...");

    while let Some(connecting) = endpoint.accept().await {

        println!("connecting...");

        let conn = connecting.await?;

        println!("connected");

        tokio::spawn(async move {

            while let Ok((mut send, mut recv)) =
                conn.accept_bi().await
            {
                let data =
                    recv.read_to_end(1024).await.unwrap();

                println!(
                    "recv={}",
                    String::from_utf8_lossy(&data)
                );

                send.write_all(b"pong").await.unwrap();
                send.finish().unwrap();

                println!("reply");
            }

        });

    }

    Ok(())
}
