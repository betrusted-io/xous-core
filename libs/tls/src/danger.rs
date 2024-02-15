use locales::t;
use modals::Modals;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::ServerCertVerified;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, Error, RootCertStore};
use xous_names::XousNames;

pub struct StifledCertificateVerification {
    pub roots: RootCertStore,
}

impl rustls::client::ServerCertVerifier for StifledCertificateVerification {
    /// Will verify the certificate with the default rustls WebPkiVerifier,
    /// BUT specifically overrides a `CertificateError::UnknownIssuer` and
    /// return ServerCertVerified::assertion()
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        server_name: &ServerName,
        scts: &mut dyn Iterator<Item = &[u8]>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let rustls_default_verifier = WebPkiServerVerifier::new(self.roots.clone(), None);
        match rustls_default_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            scts,
            ocsp,
            now,
        ) {
            Ok(ok) => Ok(ok),
            Err(Error::InvalidCertificate(e)) => {
                let xns = XousNames::new().unwrap();
                let modals = Modals::new(&xns).unwrap();
                match e {
                    CertificateError::UnknownIssuer => Ok(ServerCertVerified::assertion()),
                    CertificateError::NotValidYet => {
                        modals
                            .show_notification(t!("tls.probe_help_not_valid_yet", locales::LANG), None)
                            .expect("modal failed");
                        Err(Error::InvalidCertificate(e))
                    }
                    _ => {
                        modals
                            .show_notification(
                                format!("{}\n{:?}", t!("tls.probe_invalid_certificate", locales::LANG), e)
                                    .as_str(),
                                None,
                            )
                            .expect("modal failed");

                        Err(Error::InvalidCertificate(e))
                    }
                }
            }
            Err(e) => Err(e),
        }
    }
}
