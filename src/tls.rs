use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

pub const ALPN_PROTOCOL: &[u8] = b"gpg-bridge/1";

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("failed to read {kind} at {path}: {source}")]
    Read {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("no certificates found in {path}")]
    EmptyCertificateChain { path: PathBuf },
    #[error("failed to parse certificates in {path}: {source}")]
    CertificateParse {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("no private key found in {path}")]
    MissingPrivateKey { path: PathBuf },
    #[error("multiple private keys found in {path}")]
    MultiplePrivateKeys { path: PathBuf },
    #[error("failed to parse private key in {path}: {source}")]
    PrivateKeyParse {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[cfg(unix)]
    #[error("private key file {path} must not be readable by group or others")]
    InsecurePrivateKeyPermissions { path: PathBuf },
    #[error("no trust anchors found in {path}")]
    EmptyRootStore { path: PathBuf },
    #[error("TLS configuration error while {operation}: {reason}")]
    Configuration {
        operation: &'static str,
        reason: String,
    },
    #[error("TLS peer did not negotiate the required ALPN protocol")]
    AlpnMismatch,
}

/// Ensures that a TLS peer negotiated the bridge's sole application protocol.
///
/// # Errors
///
/// Returns `AlpnMismatch` when the negotiated protocol is absent or different.
pub fn validate_alpn(negotiated: Option<&[u8]>) -> Result<(), TlsError> {
    if negotiated == Some(ALPN_PROTOCOL) {
        Ok(())
    } else {
        Err(TlsError::AlpnMismatch)
    }
}

fn open(path: &Path, kind: &'static str) -> Result<BufReader<File>, TlsError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| TlsError::Read {
            kind,
            path: path.to_owned(),
            source,
        })
}

/// Loads a non-empty PEM certificate chain.
///
/// # Errors
///
/// Returns a path-specific error when the file cannot be read or parsed.
pub fn load_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let certificates = rustls_pemfile::certs(&mut open(path, "certificate file")?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsError::CertificateParse {
            path: path.to_owned(),
            source,
        })?;
    if certificates.is_empty() {
        return Err(TlsError::EmptyCertificateChain {
            path: path.to_owned(),
        });
    }
    Ok(certificates)
}

#[cfg(unix)]
fn check_private_key_permissions(path: &Path) -> Result<(), TlsError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path).map_err(|source| TlsError::Read {
        kind: "private key file metadata",
        path: path.to_owned(),
        source,
    })?;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(TlsError::InsecurePrivateKeyPermissions {
            path: path.to_owned(),
        });
    }
    Ok(())
}

/// Loads exactly one PEM private key after checking Unix file permissions.
///
/// # Errors
///
/// Returns a path-specific error for unreadable, malformed, ambiguous, or insecure keys.
pub fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    #[cfg(unix)]
    check_private_key_permissions(path)?;

    let mut reader = open(path, "private key file")?;
    let key =
        rustls_pemfile::private_key(&mut reader).map_err(|source| TlsError::PrivateKeyParse {
            path: path.to_owned(),
            source,
        })?;
    let Some(key) = key else {
        return Err(TlsError::MissingPrivateKey {
            path: path.to_owned(),
        });
    };
    if rustls_pemfile::private_key(&mut reader)
        .map_err(|source| TlsError::PrivateKeyParse {
            path: path.to_owned(),
            source,
        })?
        .is_some()
    {
        return Err(TlsError::MultiplePrivateKeys {
            path: path.to_owned(),
        });
    }
    Ok(key)
}

fn root_store(path: &Path) -> Result<rustls::RootCertStore, TlsError> {
    let mut roots = rustls::RootCertStore::empty();
    let certificates = load_certificates(path)?;
    let (added, _) = roots.add_parsable_certificates(certificates);
    if added == 0 {
        return Err(TlsError::EmptyRootStore {
            path: path.to_owned(),
        });
    }
    Ok(roots)
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Builds a TLS-1.3-only server configuration requiring a trusted client certificate.
///
/// # Errors
///
/// Returns a path-specific or TLS configuration error for invalid input materials.
pub fn server_config(
    client_ca_cert: &Path,
    server_cert: &Path,
    server_key: &Path,
) -> Result<rustls::ServerConfig, TlsError> {
    let provider = provider();
    let client_verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(root_store(client_ca_cert)?),
        provider.clone(),
    )
    .build()
    .map_err(|error| TlsError::Configuration {
        operation: "building client certificate verifier",
        reason: error.to_string(),
    })?;
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| TlsError::Configuration {
            operation: "selecting TLS 1.3",
            reason: error.to_string(),
        })?
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(
            load_certificates(server_cert)?,
            load_private_key(server_key)?,
        )
        .map_err(|error| TlsError::Configuration {
            operation: "loading server certificate and key",
            reason: error.to_string(),
        })?;
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    Ok(config)
}

/// Builds a TLS-1.3-only client configuration with server verification and client auth.
///
/// # Errors
///
/// Returns a path-specific or TLS configuration error for invalid input materials.
pub fn client_config(
    server_ca_cert: &Path,
    client_cert: &Path,
    client_key: &Path,
) -> Result<rustls::ClientConfig, TlsError> {
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| TlsError::Configuration {
            operation: "selecting TLS 1.3",
            reason: error.to_string(),
        })?
        .with_root_certificates(root_store(server_ca_cert)?)
        .with_client_auth_cert(
            load_certificates(client_cert)?,
            load_private_key(client_key)?,
        )
        .map_err(|error| TlsError::Configuration {
            operation: "loading client certificate and key",
            reason: error.to_string(),
        })?;
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair,
    };
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    struct Materials {
        directory: TempDir,
        ca: PathBuf,
        server_cert: PathBuf,
        server_key: PathBuf,
        client_cert: PathBuf,
        client_key: PathBuf,
    }

    fn write_private_key(path: &Path, key: &KeyPair) {
        fs::write(path, key.serialize_pem()).expect("write private key");
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("secure key mode");
    }

    fn materials() -> Materials {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().expect("CA key"))
            .expect("CA certificate");

        let mut server_params =
            CertificateParams::new(vec!["server.test".to_owned()]).expect("server parameters");
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().expect("server key");
        let server = server_params
            .signed_by(&server_key, &ca)
            .expect("server certificate");

        let mut client_params = CertificateParams::default();
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_key = KeyPair::generate().expect("client key");
        let client = client_params
            .signed_by(&client_key, &ca)
            .expect("client certificate");

        let ca_path = directory.path().join("ca.pem");
        let server_cert = directory.path().join("server.pem");
        let server_key_path = directory.path().join("server-key.pem");
        let client_cert = directory.path().join("client.pem");
        let client_key_path = directory.path().join("client-key.pem");
        fs::write(&ca_path, ca.pem()).expect("write CA");
        fs::write(&server_cert, server.pem()).expect("write server certificate");
        write_private_key(&server_key_path, &server_key);
        fs::write(&client_cert, client.pem()).expect("write client certificate");
        write_private_key(&client_key_path, &client_key);
        Materials {
            directory,
            ca: ca_path,
            server_cert,
            server_key: server_key_path,
            client_cert,
            client_key: client_key_path,
        }
    }

    #[tokio::test]
    async fn mutual_tls_uses_tls13_and_the_required_alpn() {
        let materials = materials();
        let server = server_config(&materials.ca, &materials.server_cert, &materials.server_key)
            .expect("server config");
        let client = client_config(&materials.ca, &materials.client_cert, &materials.client_key)
            .expect("client config");
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            TlsAcceptor::from(Arc::new(server))
                .accept(server_stream)
                .await
                .expect("server handshake")
        });
        let client_stream = TlsConnector::from(Arc::new(client))
            .connect(
                rustls::pki_types::ServerName::try_from("server.test".to_owned())
                    .expect("server name"),
                client_stream,
            )
            .await
            .expect("client handshake");
        let server_stream = server_task.await.expect("server task");
        assert_eq!(
            client_stream.get_ref().1.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        assert_eq!(
            server_stream.get_ref().1.alpn_protocol(),
            Some(ALPN_PROTOCOL)
        );
    }

    #[tokio::test]
    async fn wrong_server_name_fails_closed() {
        let materials = materials();
        let server = server_config(&materials.ca, &materials.server_cert, &materials.server_key)
            .expect("server config");
        let client = client_config(&materials.ca, &materials.client_cert, &materials.client_key)
            .expect("client config");
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            TlsAcceptor::from(Arc::new(server))
                .accept(server_stream)
                .await
        });
        let result = TlsConnector::from(Arc::new(client))
            .connect(
                rustls::pki_types::ServerName::try_from("wrong.test".to_owned())
                    .expect("server name"),
                client_stream,
            )
            .await;
        assert!(result.is_err());
        assert!(server_task.await.expect("server task").is_err());
    }

    #[tokio::test]
    async fn rogue_client_certificate_is_rejected_by_the_server() {
        let trusted_materials = materials();
        let server = server_config(
            &trusted_materials.ca,
            &trusted_materials.server_cert,
            &trusted_materials.server_key,
        )
        .expect("server config");
        let rogue = materials();
        let rogue_client =
            client_config(&trusted_materials.ca, &rogue.client_cert, &rogue.client_key)
                .expect("rogue client config");
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            TlsAcceptor::from(Arc::new(server))
                .accept(server_stream)
                .await
        });
        let _client = TlsConnector::from(Arc::new(rogue_client))
            .connect(
                rustls::pki_types::ServerName::try_from("server.test".to_owned())
                    .expect("server name"),
                client_stream,
            )
            .await;
        assert!(server_task.await.expect("server task").is_err());
    }

    #[test]
    fn malformed_or_insecure_pem_fails_without_disclosing_contents() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let certificate = directory.path().join("certificate.pem");
        let key = directory.path().join("key.pem");
        fs::write(&certificate, "not a certificate").expect("write certificate");
        fs::write(&key, "not a key").expect("write key");
        #[cfg(unix)]
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).expect("insecure mode");
        let error = load_private_key(&key).expect_err("key is rejected");
        assert!(!error.to_string().contains("not a key"));
        #[cfg(unix)]
        assert!(matches!(
            error,
            TlsError::InsecurePrivateKeyPermissions { .. }
        ));
        assert!(load_certificates(&certificate).is_err());
    }

    #[test]
    fn empty_missing_and_mismatched_pem_inputs_name_their_paths() {
        let materials = materials();
        let missing = materials.directory.path().join("missing.pem");
        let error = load_certificates(&missing).expect_err("missing certificate fails");
        assert!(
            error
                .to_string()
                .contains(missing.to_string_lossy().as_ref())
        );

        let empty = materials.directory.path().join("empty.pem");
        fs::write(&empty, "").expect("write empty certificate");
        assert!(matches!(
            load_certificates(&empty),
            Err(TlsError::EmptyCertificateChain { .. })
        ));

        let mismatch = server_config(&materials.ca, &materials.server_cert, &materials.client_key)
            .expect_err("mismatched certificate and key fail");
        assert!(matches!(mismatch, TlsError::Configuration { .. }));
    }

    #[test]
    fn configuration_only_advertises_tls13_and_required_alpn() {
        let materials = materials();
        let config = server_config(&materials.ca, &materials.server_cert, &materials.server_key)
            .expect("server config");
        assert_eq!(config.alpn_protocols, [ALPN_PROTOCOL]);
        assert!(validate_alpn(Some(ALPN_PROTOCOL)).is_ok());
        assert!(matches!(validate_alpn(None), Err(TlsError::AlpnMismatch)));
    }
}
