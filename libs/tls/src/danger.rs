use rustls::client::{ServerCertVerified, WebPkiVerifier};
use rustls::{CertificateError, Error, RootCertStore};

pub struct StifledCertificateVerification {
    pub roots: RootCertStore,
}

impl rustls::client::ServerCertVerifier for StifledCertificateVerification {
    /// Will verify the certificate with the default rustls WebPkiVerifier,
    /// BUT specifically overrides a `CertificateError::UnknownIssuer` and
    /// return ServerCertVerified::assertion()
    fn verify_server_cert(
        &self,
        end_entity: &rustls::Certificate,
        intermediates: &[rustls::Certificate],
        server_name: &rustls::ServerName,
        scts: &mut dyn Iterator<Item = &[u8]>,
        ocsp: &[u8],
        now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        let rustls_default_verifier = WebPkiVerifier::new(self.roots.clone(), None);
        match rustls_default_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            scts,
            ocsp,
            now,
        ) {
            Ok(ok) => Ok(ok),
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {
                Ok(ServerCertVerified::assertion())
            }
            Err(e) => Err(e),
        }
    }
}
