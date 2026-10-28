//! `netz tls`: prüft, wer das HTTPS-Zertifikat einer Seite unterschrieben hat.
//! Geprüft wird gegen die Mozilla-Wurzeln (webpki-roots), nicht gegen den macOS-Schlüsselbund,
//! weil dort per MDM verteilte Firmenzertifikate eine Inspection verstecken würden.

use crate::explain;
use ring::digest;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, Error, RootCertStore,
    SignatureScheme,
};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x509_cert::Certificate;
use x509_cert::der::Decode;

const DEFAULT_HOSTS: [&str; 3] = ["github.com", "apple.com", "cloudflare.com"];

/// Substring of the lowercase issuer name -> product name.
const PRODUCTS: [(&str, &str); 12] = [
    ("fortinet", "Fortinet FortiGate"),
    ("fortigate", "Fortinet FortiGate"),
    ("zscaler", "Zscaler"),
    ("sophos", "Sophos"),
    ("palo alto", "Palo Alto Networks"),
    ("umbrella", "Cisco Umbrella"),
    ("cisco", "Cisco"),
    ("bluecoat", "Symantec/Broadcom Blue Coat"),
    ("netskope", "Netskope"),
    ("forcepoint", "Forcepoint"),
    ("checkpoint", "Check Point"),
    ("watchguard", "WatchGuard"),
];

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Normal,
    /// Issuer is not publicly trusted: someone may be reading along.
    Aufgebrochen,
    Fehler(String),
}

pub struct ChainInfo {
    pub leaf_issuer: String,
    pub top_subject: String,
    pub top_issuer: String,
    pub not_before: String,
    pub not_after: String,
    /// SHA-256 of the certificate that signed the leaf (hex, lowercase), for the comparison in #6.
    pub issuer_sha256: Option<String>,
    pub product: Option<&'static str>,
}

pub struct TlsReport {
    pub verdict: Verdict,
    /// `None` when the server never presented a certificate.
    pub chain: Option<ChainInfo>,
}

type Seen = (Vec<Vec<u8>>, Option<Result<(), Error>>);

/// Records the presented chain and the WebPKI result, then lets the handshake go on or fail.
#[derive(Debug)]
struct Recorder {
    inner: Arc<WebPkiServerVerifier>,
    seen: Mutex<Seen>,
}

impl Recorder {
    fn new() -> Result<Self, String> {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let inner = WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .map_err(|e| format!("Die Mozilla-Wurzeln ließen sich nicht laden ({e})."))?;
        Ok(Self {
            inner,
            seen: Mutex::new((Vec::new(), None)),
        })
    }
}

impl ServerCertVerifier for Recorder {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let result =
            self.inner
                .verify_server_cert(end_entity, intermediates, server_name, ocsp, now);
        if let Ok(mut seen) = self.seen.lock() {
            seen.0 = std::iter::once(end_entity)
                .chain(intermediates)
                .map(|c| c.to_vec())
                .collect();
            seen.1 = Some(result.as_ref().map(|_| ()).map_err(Clone::clone));
        }
        result
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn judge(result: &Result<(), Error>) -> Verdict {
    match result {
        Ok(()) => Verdict::Normal,
        Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)) => Verdict::Aufgebrochen,
        Err(e) => Verdict::Fehler(format!("Das Zertifikat wurde abgelehnt ({e}).")),
    }
}

/// Names a known inspection product from an issuer string.
fn detect_product(issuer: &str) -> Option<&'static str> {
    let lower = issuer.to_lowercase();
    PRODUCTS
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, name)| *name)
}

fn parse(der: &[u8]) -> Result<Certificate, String> {
    Certificate::from_der(der)
        .map_err(|_| "Ein Zertifikat des Servers ließ sich nicht lesen.".to_string())
}

fn date_of(t: &x509_cert::time::Time) -> String {
    let d = t.to_date_time();
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn chain_info(chain: &[Vec<u8>]) -> Result<ChainInfo, String> {
    let (Some(leaf_der), Some(top_der)) = (chain.first(), chain.last()) else {
        return Err("Der Server hat kein Zertifikat geschickt.".into());
    };
    let leaf = parse(leaf_der)?;
    let top = parse(top_der)?;
    let leaf_tbs = leaf.tbs_certificate();
    let top_tbs = top.tbs_certificate();
    let leaf_issuer = leaf_tbs.issuer().to_string();
    let top_issuer = top_tbs.issuer().to_string();
    // The signer is the second certificate; a self-signed leaf is its own signer.
    // ponytail: assumes the server sends the chain in order; upgrade: match issuer to subject.
    let signer = chain
        .get(1)
        .or_else(|| (leaf_issuer == leaf_tbs.subject().to_string()).then_some(leaf_der));
    Ok(ChainInfo {
        product: detect_product(&leaf_issuer).or_else(|| detect_product(&top_issuer)),
        top_subject: top_tbs.subject().to_string(),
        not_before: date_of(&leaf_tbs.validity().not_before),
        not_after: date_of(&leaf_tbs.validity().not_after),
        issuer_sha256: signer.map(|d| hex(digest::digest(&digest::SHA256, d).as_ref())),
        leaf_issuer,
        top_issuer,
    })
}

fn handshake(config: ClientConfig, host: &str) -> Result<(), String> {
    let name = ServerName::try_from(host.to_string()).map_err(|_| {
        format!("„{host}“ ist kein gültiger Hostname. Bitte prüfe die Schreibweise.")
    })?;
    let timeout = Duration::from_secs(5);
    let addr = (host, 443)
        .to_socket_addrs()
        .map_err(|_| format!("„{host}“ ließ sich nicht auflösen. Prüfe Name und Netz."))?
        .next()
        .ok_or_else(|| format!("„{host}“ hat keine Adresse."))?;
    let mut sock = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("Verbindung zu {host}:443 nicht möglich ({e})."))?;
    let _ = sock.set_read_timeout(Some(timeout));
    let _ = sock.set_write_timeout(Some(timeout));
    let mut conn = ClientConnection::new(Arc::new(config), name)
        .map_err(|e| format!("TLS ließ sich nicht starten ({e})."))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut sock)
            .map_err(|e| format!("TLS-Handshake mit {host} gescheitert ({e})."))?;
    }
    Ok(())
}

/// Connects to `host:443`, collects the chain and judges it. A failed connection is a `Fehler` verdict, not an `Err`.
pub fn check_host(host: &str) -> Result<TlsReport, String> {
    let recorder = Arc::new(Recorder::new()?);
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS ließ sich nicht einrichten ({e})."))?
            .dangerous()
            .with_custom_certificate_verifier(recorder.clone())
            .with_no_client_auth();
    let handshake_result = handshake(config, host);
    let (chain_der, result) = match recorder.seen.lock() {
        Ok(seen) => seen.clone(),
        Err(_) => (Vec::new(), None),
    };
    let report = |verdict, chain| TlsReport { verdict, chain };
    // Without a recorded result the failure happened before any certificate arrived.
    let Some(result) = result else {
        let msg = handshake_result
            .err()
            .unwrap_or_else(|| "Kein Zertifikat erhalten.".into());
        return Ok(report(Verdict::Fehler(msg), None));
    };
    Ok(report(judge(&result), Some(chain_info(&chain_der)?)))
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let hosts: Vec<String> = if args.is_empty() {
        DEFAULT_HOSTS.iter().map(|h| h.to_string()).collect()
    } else {
        args.to_vec()
    };
    explain(
        explain_on,
        "Ein Zertifikat ist der Ausweis einer Webseite: „Dieser Schlüssel gehört zu github.com“. Damit niemand sich den Ausweis selbst ausstellen kann, unterschreibt ihn eine Zertifizierungsstelle (CA).",
    );
    explain(
        explain_on,
        "Dein Mac vertraut einer festen Liste solcher Stellen (Wurzelzertifikate). netz nimmt hier die Liste von Mozilla und nicht den Schlüsselbund deines Macs, denn in den kann eine Firma per MDM eigene Wurzeln einschleusen.",
    );
    for host in &hosts {
        println!("{host}");
        explain(
            explain_on,
            &format!(
                "netz verbindet sich mit {host}:443 und lässt sich die Zertifikatskette zeigen."
            ),
        );
        let r = check_host(host)?;
        if let Some(c) = &r.chain {
            println!("  Aussteller des Serverzertifikats: {}", c.leaf_issuer);
            println!(
                "  Oberstes Kettenglied: {} (ausgestellt von {})",
                c.top_subject, c.top_issuer
            );
            println!("  Gültig: {} bis {}", c.not_before, c.not_after);
            if let Some(fp) = &c.issuer_sha256 {
                println!("  SHA-256 des Ausstellerzertifikats: {fp}");
            }
            if let Some(p) = c.product {
                println!("  Erkanntes Produkt: {p}");
            }
        }
        match &r.verdict {
            Verdict::Normal => {
                println!("  Urteil: normal");
                explain(
                    explain_on,
                    "Die Kette endet bei einer öffentlich bekannten CA. So soll es sein.",
                );
            }
            Verdict::Aufgebrochen => {
                println!("  Urteil: aufgebrochen (Aussteller nicht öffentlich vertrauenswürdig)");
                explain(
                    explain_on,
                    "Die Kette endet bei einer CA, die Mozilla nicht kennt. Wer in deinem Netz ein eigenes Zertifikat für diese Seite ausstellt und dem dein Gerät vertraut, kann die Verbindung entschlüsseln, mitlesen und neu verschlüsseln. Firmennetze machen das als SSL-Inspection, ein Angreifer im WLAN könnte dasselbe tun.",
                );
            }
            Verdict::Fehler(msg) => {
                println!("  Urteil: Verbindung schlägt fehl");
                println!("  {msg}");
                explain(
                    explain_on,
                    "Die Verbindung kam nicht zustande oder das Zertifikat wurde aus einem anderen Grund abgelehnt, etwa weil es abgelaufen ist oder nicht zum Namen passt.",
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAF: &[u8] = include_bytes!("../testdata/github_leaf.der");
    const INTERMEDIATE: &[u8] = include_bytes!("../testdata/sectigo_intermediate.der");
    const SELF_SIGNED: &[u8] = include_bytes!("../testdata/selfsigned.der");

    /// about 2026-10-30, inside the validity of the embedded github.com certificate.
    fn now() -> UnixTime {
        UnixTime::since_unix_epoch(Duration::from_secs(1_793_400_000))
    }

    fn verify(chain: &[&[u8]], host: &str) -> Verdict {
        let rec = Recorder::new().unwrap();
        let ee = CertificateDer::from(chain[0]);
        let inter: Vec<_> = chain[1..]
            .iter()
            .map(|c| CertificateDer::from(*c))
            .collect();
        let name = ServerName::try_from(host.to_string()).unwrap();
        let r = rec
            .verify_server_cert(&ee, &inter, &name, &[], now())
            .map(|_| ());
        judge(&r)
    }

    #[test]
    fn public_chain_is_normal() {
        assert_eq!(verify(&[LEAF, INTERMEDIATE], "github.com"), Verdict::Normal);
    }

    #[test]
    fn self_signed_is_aufgebrochen() {
        assert_eq!(verify(&[SELF_SIGNED], "github.com"), Verdict::Aufgebrochen);
    }

    #[test]
    fn wrong_name_is_fehler_not_aufgebrochen() {
        assert!(matches!(
            verify(&[LEAF, INTERMEDIATE], "example.org"),
            Verdict::Fehler(_)
        ));
    }

    #[test]
    fn chain_info_reads_names_and_fingerprint() {
        let c = chain_info(&[LEAF.to_vec(), INTERMEDIATE.to_vec()]).unwrap();
        assert!(c.leaf_issuer.contains("Sectigo"));
        assert_eq!(c.not_after, "2026-11-29");
        assert_eq!(
            c.issuer_sha256,
            Some(hex(digest::digest(&digest::SHA256, INTERMEDIATE).as_ref()))
        );
        assert_eq!(c.product, None);
    }

    #[test]
    fn self_signed_leaf_is_its_own_issuer() {
        let c = chain_info(&[SELF_SIGNED.to_vec()]).unwrap();
        assert_eq!(c.product, Some("Fortinet FortiGate"));
        assert!(c.issuer_sha256.is_some());
    }

    #[test]
    fn products_are_detected_by_issuer() {
        assert_eq!(
            detect_product("CN=Zscaler Root CA,O=Zscaler Inc."),
            Some("Zscaler")
        );
        assert_eq!(detect_product("CN=SOPHOS_SSL_CA_1"), Some("Sophos"));
        assert_eq!(
            detect_product("O=Palo Alto Networks,CN=Forward-Trust"),
            Some("Palo Alto Networks")
        );
        assert_eq!(
            detect_product("CN=Cisco Umbrella Secondary SubCA"),
            Some("Cisco Umbrella")
        );
        assert_eq!(detect_product("O=Let's Encrypt,CN=R11"), None);
    }

    #[test]
    fn missing_chain_is_an_error() {
        assert!(chain_info(&[]).is_err());
    }
}
