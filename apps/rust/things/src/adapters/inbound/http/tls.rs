//! Per-device TLS termination for the portal.
//!
//! The portal serves HTTPS with a self-signed certificate so browsers treat
//! the LAN origin as a secure context: `navigator.mediaDevices.getUserMedia`
//! (and therefore the camera talk feature) only exists on secure contexts.
//! The certificate is provisioned on first boot and persisted under
//! `/userdata` so it survives restarts and OTA upgrades; the private key is
//! root-only and never leaves the device.

use std::{
    fs,
    io::{self, ErrorKind},
    net::SocketAddr,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::Arc,
    time::Duration,
};

use tokio::task::JoinSet;
use tokio_rustls::{
    rustls::{
        pki_types::{
            pem::PemObject,
            {CertificateDer, PrivateKeyDer},
        },
        ServerConfig,
    },
    TlsAcceptor,
};

use super::LAN_ADDRESS;
use crate::logging::{self, Level};

const CERT_DIRECTORY: &str = "/userdata/hyz-things/tls";
const CERTIFICATE_FILE: &str = "server.crt";
const PRIVATE_KEY_FILE: &str = "server.key";
const CERTIFICATE_VALIDITY_YEARS: u16 = 10;

/// Loads or provisions the device TLS identity and terminates connections.
pub struct PortalTls {
    acceptor: TlsAcceptor,
}

impl PortalTls {
    /// Loads the persisted identity, or provisions a fresh self-signed pair
    /// on first boot (or after the persisted pair became unreadable).
    pub fn load_or_generate() -> Result<Self, String> {
        let directory = Path::new(CERT_DIRECTORY);
        let certificate_path = directory.join(CERTIFICATE_FILE);
        let private_key_path = directory.join(PRIVATE_KEY_FILE);
        if certificate_path.exists() && private_key_path.exists() {
            match load_certificate(&certificate_path, &private_key_path) {
                Ok(acceptor) => return Ok(Self { acceptor }),
                Err(_error) => {
                    logging::event(
                        Level::Warn,
                        "tls",
                        "load_identity",
                        "dependency_unavailable",
                        Some("tls_identity_unreadable"),
                        None,
                        None,
                        None,
                    );
                }
            }
        }
        let acceptor = generate_certificate(&certificate_path, &private_key_path)?;
        Ok(Self { acceptor })
    }

    pub fn acceptor(&self) -> TlsAcceptor {
        self.acceptor.clone()
    }
}

fn load_certificate(certificate_path: &Path, private_key_path: &Path) -> io::Result<TlsAcceptor> {
    let certificate_pem = fs::read(certificate_path)?;
    let private_key_pem = fs::read(private_key_path)?;
    let certificates = vec![
        CertificateDer::from_pem_slice(&certificate_pem).map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("device certificate is not valid PEM: {error}"),
            )
        })?,
    ];
    let private_key = PrivateKeyDer::from_pem_slice(&private_key_pem).map_err(|error| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("device private key is not valid PEM: {error}"),
        )
    })?;
    server_acceptor(certificates, private_key)
}

fn generate_certificate(
    certificate_path: &Path,
    private_key_path: &Path,
) -> Result<TlsAcceptor, String> {
    use rcgen::{date_time_ymd, CertificateParams, DnType, KeyPair};

    let key_pair =
        KeyPair::generate().map_err(|error| format!("TLS key generation failed: {error}"))?;
    // rcgen parses each string: IP literals become IP SANs, everything else a
    // DNS name. The LAN address must be present so browsers accept the
    // self-signed certificate for the exact origin users type.
    let mut params =
        CertificateParams::new(vec![LAN_ADDRESS.to_string(), "hyz-things.local".to_owned()])
            .map_err(|error| format!("TLS certificate parameters are invalid: {error}"))?;
    params
        .distinguished_name
        .push(DnType::CommonName, "hyz-things");
    params.not_before = date_time_ymd(2025, 1, 1);
    params.not_after = date_time_ymd(2025 + i32::from(CERTIFICATE_VALIDITY_YEARS), 1, 1);
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| format!("TLS self-signing failed: {error}"))?;

    fs::create_dir_all(
        certificate_path
            .parent()
            .expect("certificate path must have a parent"),
    )
    .map_err(|error| format!("cannot create {CERT_DIRECTORY}: {error}"))?;
    write_private(certificate_path, certificate.pem().as_bytes())?;
    write_private(private_key_path, key_pair.serialize_pem().as_bytes())?;

    let certificates = vec![certificate.der().clone()];
    let private_key = PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|error| format!("generated private key is unusable: {error}"))?;
    server_acceptor(certificates, private_key)
        .map_err(|error| format!("generated TLS identity is unusable: {error}"))
}

fn write_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush {}: {error}", path.display()))
}

fn server_acceptor(
    certificates: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
) -> io::Result<TlsAcceptor> {
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("TLS identity is invalid: {error}"),
            )
        })?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PENDING_TLS_HANDSHAKES: usize = 32;

enum HandshakeOutcome {
    Accepted(
        Box<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>,
        SocketAddr,
    ),
    Rejected {
        peer: SocketAddr,
        timed_out: bool,
    },
}

async fn complete_handshake(
    acceptor: TlsAcceptor,
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
) -> HandshakeOutcome {
    match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
        Ok(Ok(tls_stream)) => HandshakeOutcome::Accepted(Box::new(tls_stream), peer),
        Ok(Err(_error)) => HandshakeOutcome::Rejected {
            peer,
            timed_out: false,
        },
        Err(_elapsed) => HandshakeOutcome::Rejected {
            peer,
            timed_out: true,
        },
    }
}

/// Terminates TLS on top of any plain TCP `axum::serve::Listener`, so the LAN
/// and Tailscale management listeners share one TLS identity. Handshakes run
/// concurrently with a fixed limit and timeout so an incomplete client cannot
/// block later connections.
pub struct TlsListener<L> {
    inner: L,
    acceptor: TlsAcceptor,
    handshakes: JoinSet<HandshakeOutcome>,
    pending_handshakes: usize,
}

impl<L> TlsListener<L>
where
    L: axum::serve::Listener<Io = tokio::net::TcpStream, Addr = SocketAddr>,
{
    pub fn new(inner: L, acceptor: TlsAcceptor) -> Self {
        Self {
            inner,
            acceptor,
            handshakes: JoinSet::new(),
            pending_handshakes: 0,
        }
    }
}

impl<L> axum::serve::Listener for TlsListener<L>
where
    L: axum::serve::Listener<Io = tokio::net::TcpStream, Addr = SocketAddr>,
{
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if self.pending_handshakes >= MAX_PENDING_TLS_HANDSHAKES {
                match self.handshakes.join_next().await {
                    Some(result) => {
                        self.pending_handshakes -= 1;
                        if let Some(accepted) = self.handle_handshake_result(result) {
                            return accepted;
                        }
                    }
                    None => self.pending_handshakes = 0,
                }
                continue;
            }

            tokio::select! {
                biased;
                result = self.handshakes.join_next(), if self.pending_handshakes > 0 => {
                    match result {
                        Some(result) => {
                            self.pending_handshakes -= 1;
                            if let Some(accepted) = self.handle_handshake_result(result) {
                                return accepted;
                            }
                        }
                        None => self.pending_handshakes = 0,
                    }
                }
                (stream, peer) = self.inner.accept() => {
                    let acceptor = self.acceptor.clone();
                    self.handshakes.spawn(async move {
                        complete_handshake(acceptor, stream, peer).await
                    });
                    self.pending_handshakes += 1;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

impl<L> TlsListener<L>
where
    L: axum::serve::Listener<Io = tokio::net::TcpStream, Addr = SocketAddr>,
{
    fn handle_handshake_result(
        &self,
        result: Result<HandshakeOutcome, tokio::task::JoinError>,
    ) -> Option<(
        tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
        SocketAddr,
    )> {
        match result {
            Ok(HandshakeOutcome::Accepted(stream, peer)) => Some((*stream, peer)),
            Ok(HandshakeOutcome::Rejected { peer, timed_out }) => {
                let listener = self.inner.local_addr().unwrap_or(peer);
                logging::tls_handshake_result(listener, peer, timed_out);
                None
            }
            Err(_error) => {
                logging::event(
                    Level::Error,
                    "tls",
                    "handshake",
                    "worker_failed",
                    Some("tls_handshake_worker_failed"),
                    None,
                    None,
                    None,
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::serve::Listener;
    use std::{os::unix::fs::PermissionsExt, time::Duration};

    #[test]
    fn generated_self_signed_identity_round_trips() {
        let directory =
            std::env::temp_dir().join(format!("hyz-things-tls-test-{}", std::process::id()));
        let certificate_path = directory.join(CERTIFICATE_FILE);
        let private_key_path = directory.join(PRIVATE_KEY_FILE);

        let generated = generate_certificate(&certificate_path, &private_key_path)
            .expect("certificate generation must succeed");
        assert!(certificate_path.exists());
        assert!(private_key_path.exists());
        let key_mode = std::fs::metadata(&private_key_path)
            .expect("private key metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(key_mode, 0o600, "private key must be root-only");

        let loaded = load_certificate(&certificate_path, &private_key_path)
            .expect("persisted identity must load back");
        drop(generated);
        drop(loaded);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn generated_certificate_covers_the_lan_address() {
        let directory =
            std::env::temp_dir().join(format!("hyz-things-tls-san-test-{}", std::process::id()));
        let certificate_path = directory.join(CERTIFICATE_FILE);
        let private_key_path = directory.join(PRIVATE_KEY_FILE);
        generate_certificate(&certificate_path, &private_key_path)
            .expect("certificate generation must succeed");

        let pem = std::fs::read_to_string(&certificate_path).expect("certificate PEM");
        let certificate = CertificateDer::from_pem_slice(pem.as_bytes())
            .expect("generated certificate must parse");
        let der = certificate.as_ref();
        let lan_octets = LAN_ADDRESS.octets();
        assert!(
            der.windows(4).any(|window| window == lan_octets),
            "certificate SANs must cover the LAN address"
        );
        assert!(
            der.windows(b"hyz-things.local".len())
                .any(|window| window == b"hyz-things.local"),
            "certificate SANs must cover the device hostname"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[derive(Debug)]
    struct SkipServerVerification(Arc<tokio_rustls::rustls::crypto::CryptoProvider>);

    impl SkipServerVerification {
        fn new() -> Arc<Self> {
            Arc::new(Self(Arc::new(
                tokio_rustls::rustls::crypto::ring::default_provider(),
            )))
        }
    }

    impl tokio_rustls::rustls::client::danger::ServerCertVerifier for SkipServerVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: tokio_rustls::rustls::pki_types::UnixTime,
        ) -> Result<
            tokio_rustls::rustls::client::danger::ServerCertVerified,
            tokio_rustls::rustls::Error,
        > {
            Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            tokio_rustls::rustls::crypto::verify_tls12_signature(
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
            dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            tokio_rustls::rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    fn test_acceptor() -> TlsAcceptor {
        let key_pair = rcgen::KeyPair::generate().expect("generate test key");
        let params = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
            .expect("create test certificate parameters");
        let certificate = params
            .self_signed(&key_pair)
            .expect("create test certificate");
        let private_key =
            PrivateKeyDer::try_from(key_pair.serialize_der()).expect("serialize test private key");
        server_acceptor(vec![certificate.der().clone()], private_key)
            .expect("create test TLS acceptor")
    }

    #[tokio::test]
    async fn incomplete_tls_handshake_does_not_block_later_client() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let address = listener.local_addr().expect("read test listener address");
        let accept_task = tokio::spawn(async move {
            let mut listener = TlsListener::new(listener, test_acceptor());
            listener.accept().await
        });

        let stalled = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect stalled client");
        tokio::time::sleep(Duration::from_millis(25)).await;

        let client_config = tokio_rustls::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
        let client_stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect valid client");
        let client_peer = client_stream
            .local_addr()
            .expect("read valid client address");
        let server_name =
            tokio_rustls::rustls::pki_types::ServerName::try_from("localhost".to_owned())
                .expect("create server name");
        let _client_tls = tokio::time::timeout(
            Duration::from_secs(1),
            connector.connect(server_name, client_stream),
        )
        .await
        .expect("valid client handshake must not wait for stalled client")
        .expect("valid client handshake");

        let (_server_tls, peer) = tokio::time::timeout(Duration::from_secs(1), accept_task)
            .await
            .expect("server must accept valid client")
            .expect("server accept task");
        assert_eq!(peer, client_peer);
        drop(stalled);
    }
}
