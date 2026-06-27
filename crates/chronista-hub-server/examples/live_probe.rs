//! live_probe — 公開 hub に exported cert を `TrustAnchors::Custom` で pin して
//! register/discover を通す疎通確認 (worlds_demo は SkipVerification=loopback 限定なので
//! 非 loopback の公開 hub にはこれを使う)。throwaway。
//!
//!   HUB_CERT=<cert DER path> HUB_ADDR=hub.chronista.club:12879 \
//!     cargo run -p chronista-hub-server --example live_probe

use anyhow::Result;
use rustls::pki_types::CertificateDer;
use serde_json::{Value, json};
use unison::ProtocolClient;
use unison::network::channel::UnisonChannel;
use unison::network::{QuicClient, TrustAnchors};

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let addr = std::env::var("HUB_ADDR").unwrap_or_else(|_| "hub.chronista.club:12879".into());
    let cert_path = std::env::var("HUB_CERT").expect("HUB_CERT=<exported cert DER path>");
    let der = std::fs::read(&cert_path)?;
    println!(
        "hub={addr}  cert={cert_path} ({}B) を TrustAnchors::Custom で pin\n",
        der.len()
    );

    let quic = QuicClient::builder()
        .trust_anchors(TrustAnchors::Custom(vec![CertificateDer::from(der)]))
        .build()?;
    let client = ProtocolClient::new(quic);

    client.connect(&addr).await?;
    println!("✓ QUIC connect OK (cert pin 通過 = TLS handshake 成立)");

    let ch: UnisonChannel = client.open_channel("worlds").await?;
    let reg: Value = ch
        .request(
            "Register",
            &json!({
                "wld_id": "wld_liveprobe",
                "handle": "live-probe",
                "name": "Live Probe",
                "endpoints": ["[2400:4150:0:1::1]:32000"]
            }),
        )
        .await?;
    println!("✓ Register → {reg}");

    let disc: Value = ch.request("Discover", &json!({})).await?;
    println!("✓ Discover → {disc}");

    ch.close().await?;
    client.disconnect().await?;
    println!("\n✅ 公開 hub federation 疎通 OK (cert pin + register + discover round-trip)");
    Ok(())
}
