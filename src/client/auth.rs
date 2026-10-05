//! TRUEOS auth keeps HTTPS and account authentication without loading OS roots.
//! Certificate chain and hostname checks are deliberately bypassed here; TLS
//! handshake signatures still have to verify against the server certificate.

use std::sync::Arc;

use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};

#[derive(Debug)]
struct AuthServerVerifier {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AuthServerVerifier {
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
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub(super) fn client(
    scheme: authc::Scheme,
    authority: authc::Authority,
) -> Result<authc::AuthClient, authc::AuthClientError> {
    // AuthClient otherwise permits plain HTTP in debug builds and on localhost.
    if scheme != authc::Scheme::HTTPS {
        return Err(authc::AuthClientError::InsecureSchema);
    }

    let builder = rustls::ClientConfig::builder();
    let provider = Arc::clone(builder.crypto_provider());
    let config = builder
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AuthServerVerifier { provider }))
        .with_no_client_auth();
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_only()
        .enable_http1()
        .build();
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(https);

    authc::AuthClient::with_client(scheme, authority, client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_client_does_not_require_native_roots() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _enter = runtime.enter();
        assert!(client(authc::Scheme::HTTPS, "auth.veloren.net".parse().unwrap()).is_ok());
        assert!(matches!(
            client(authc::Scheme::HTTP, "localhost".parse().unwrap()),
            Err(authc::AuthClientError::InsecureSchema)
        ));
    }

    #[test]
    fn chain_validation_does_not_require_a_trusted_certificate() {
        let verifier = AuthServerVerifier {
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        };
        let cert = CertificateDer::from(vec![0u8]);
        assert!(verifier.verify_server_cert(
            &cert,
            &[],
            &ServerName::try_from("auth.veloren.net").unwrap(),
            &[],
            UnixTime::since_unix_epoch(std::time::Duration::ZERO),
        ).is_ok());
        assert!(!verifier.supported_verify_schemes().is_empty());
    }
}
