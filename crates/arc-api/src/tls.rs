use std::path::Path;
use std::sync::Arc;

use rustls::server::WebPkiClientVerifier;
use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use crate::server_config::TlsConfig;

/// Build a rustls `ServerConfig` from the `[api.tls]` configuration.
///
/// - `mtls_enabled`: whether mTLS is listed as an authentication strategy.
/// - `mtls_optional`: whether other strategies (e.g. JWT) are also present,
///   meaning client certs should be requested but not required.
pub fn build_rustls_config(
    tls_config: &TlsConfig,
    mtls_enabled: bool,
    mtls_optional: bool,
) -> Arc<ServerConfig> {
    let certs = load_certs(&tls_config.cert);
    let key = load_private_key(&tls_config.key);

    let config = if mtls_enabled {
        let ca_certs = load_certs(&tls_config.ca);
        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert).expect("failed to add CA certificate to root store");
        }

        let verifier = if mtls_optional {
            WebPkiClientVerifier::builder(Arc::new(root_store))
                .allow_unauthenticated()
                .build()
                .expect("failed to build optional client verifier")
        } else {
            WebPkiClientVerifier::builder(Arc::new(root_store))
                .build()
                .expect("failed to build required client verifier")
        };

        ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .expect("invalid server certificate or key")
    } else {
        // TLS for encryption only, no client cert verification
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("invalid server certificate or key")
    };

    Arc::new(config)
}

fn load_certs(path: &Path) -> Vec<CertificateDer<'static>> {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open certificate file {}: {e}", path.display()));
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| panic!("failed to parse certificates from {}: {e}", path.display()))
}

fn load_private_key(path: &Path) -> PrivateKeyDer<'static> {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open private key file {}: {e}", path.display()));
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .unwrap_or_else(|e| panic!("failed to parse private key from {}: {e}", path.display()))
        .unwrap_or_else(|| panic!("no private key found in {}", path.display()))
}
