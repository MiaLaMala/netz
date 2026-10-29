//! `netzinfo sni`: stört das Netz Verbindungen anhand des Servernamens im TLS-Hallo (SNI)?
//! Vorgehen nach OONI ts-024: derselbe Testserver, einmal mit einem neutralen Kontrollnamen,
//! dann mit ein paar Beispielnamen. Geprüft wird nur, ob das Hallo samt Zertifikat ankommt.
//! Issue #52.

use crate::explain;
use crate::netze;
use crate::pruefen::Klasse;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x509_cert::Certificate;
use x509_cert::der::Decode;

/// Testserver, der viele Namen ohne Fehler annimmt (Cloudflare).
const ZIEL: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443);
const KONTROLLE: &str = "example.org";
/// Namen ohne eigenes Zertifikat bekommen vom Testserver sein Standardzertifikat; dessen
/// Aussteller gilt neben dem der Kontrolle als „echt“.
const REFERENZ: &str = "sni-pruefung.invalid";
/// Kleine, harmlose Auswahl (Nachrichtenportal, Videodienst, Messenger), Stand 2026-10-29.
/// Kein Abbild aller Sperren.
const NAMEN: [&str; 3] = ["bbc.com", "youtube.com", "telegram.org"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Verbindung nach dem ClientHello abgebrochen (RST, FIN, Alarm).
    Zurueckgesetzt,
    Zeitueberschreitung,
    /// Es kam ein Zertifikat, aber von einem anderen Aussteller als bei der Kontrolle.
    FremdesZertifikat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SniErgebnis {
    pub kontrolle: Status,
    pub namen: Vec<(String, Status)>,
    /// `sni_gestoert`: Namen, die nicht `Ok` sind, obwohl die Kontrolle durchkam.
    pub gestoert: Vec<String>,
    /// Die Kontrolle fiel aus: Der Testserver selbst ist nicht erreichbar, ein Urteil wäre geraten.
    pub unklar: bool,
}

/// Merkt sich den Aussteller des Serverzertifikats und bricht dann ab: Mehr als „das Hallo kam an“
/// braucht die Messung nicht, und ein Vertrauensurteil ist hier nicht gefragt.
#[derive(Debug)]
struct Merker {
    aussteller: Arc<Mutex<Option<String>>>,
    schemes: Vec<SignatureScheme>,
}

impl ServerCertVerifier for Merker {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let name = Certificate::from_der(end_entity)
            .map(|c| c.tbs_certificate().issuer().to_string())
            .unwrap_or_default();
        if let Ok(mut g) = self.aussteller.lock() {
            *g = Some(name);
        }
        Err(Error::General("Messung beendet".into()))
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("Messung beendet".into()))
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("Messung beendet".into()))
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.clone()
    }
}

fn fehlerstatus(e: &std::io::Error) -> Status {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Status::Zeitueberschreitung,
        _ => Status::Zurueckgesetzt,
    }
}

/// Ein Handshake mit SNI `name` bis zum Zertifikat. Gibt den Status und den Aussteller zurück.
fn sondieren(addr: SocketAddr, name: &str) -> Result<(Status, Option<String>), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let aussteller = Arc::new(Mutex::new(None));
    let merker = Merker {
        aussteller: aussteller.clone(),
        schemes: provider
            .signature_verification_algorithms
            .supported_schemes(),
    };
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS ließ sich nicht einrichten ({e})."))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(merker))
        .with_no_client_auth();
    let server = ServerName::try_from(name.to_string())
        .map_err(|_| format!("„{name}“ ist kein gültiger Hostname."))?;
    let timeout = Duration::from_secs(4);
    let mut sock = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => s,
        Err(e) => return Ok((fehlerstatus(&e), None)),
    };
    let _ = sock.set_read_timeout(Some(timeout));
    let _ = sock.set_write_timeout(Some(timeout));
    let mut conn = ClientConnection::new(Arc::new(config), server)
        .map_err(|e| format!("TLS ließ sich nicht starten ({e})."))?;
    let mut fehler = None;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut sock) {
            fehler = Some(e);
            break;
        }
    }
    let seen = aussteller.lock().ok().and_then(|g| g.clone());
    Ok(match (seen, fehler) {
        (Some(a), _) => (Status::Ok, Some(a)),
        (None, Some(e)) => (fehlerstatus(&e), None),
        (None, None) => (Status::Ok, None),
    })
}

fn messen_gegen(addr: SocketAddr, kontrolle: &str, namen: &[&str]) -> Result<SniErgebnis, String> {
    let (k, k_aussteller) = sondieren(addr, kontrolle)?;
    if k != Status::Ok {
        return Ok(SniErgebnis {
            kontrolle: k,
            namen: Vec::new(),
            gestoert: Vec::new(),
            unklar: true,
        });
    }
    let echt = [k_aussteller, sondieren(addr, REFERENZ)?.1];
    let mut out = Vec::new();
    for n in namen {
        let (mut s, aussteller) = sondieren(addr, n)?;
        if s == Status::Ok && !echt.contains(&aussteller) {
            s = Status::FremdesZertifikat;
        }
        out.push((n.to_string(), s));
    }
    let gestoert = out
        .iter()
        .filter(|(_, s)| *s != Status::Ok)
        .map(|(n, _)| n.clone())
        .collect();
    Ok(SniErgebnis {
        kontrolle: k,
        namen: out,
        gestoert,
        unklar: false,
    })
}

/// Fünf Verbindungen nacheinander zu `1.1.1.1:443`: Kontrolle, Referenz, dann die Beispielnamen.
pub fn messen() -> Result<SniErgebnis, String> {
    messen_gegen(ZIEL, KONTROLLE, &NAMEN)
}

#[derive(Debug, Serialize, Deserialize)]
struct Gespeichert {
    zeit: u64,
    gestoert: Vec<String>,
}

type Store = BTreeMap<String, Gespeichert>;

/// Was die Messung für `pruefen` bedeutet. Gestörte Namen allein sind in Schulen und Firmen
/// üblich (auffällig, nie Alarm); Alarm nur bei einem getauschten Zertifikat ohne Portal oder
/// bei einem Filter, der in einem „meins“-Netz seit der letzten Messung neu ist.
fn befunde(
    alt: Option<&Gespeichert>,
    erg: &SniErgebnis,
    meins: bool,
    portal: bool,
) -> Vec<(Klasse, String)> {
    let mit = |gesucht: fn(Status) -> bool| -> String {
        let n: Vec<&str> = erg
            .namen
            .iter()
            .filter(|(_, s)| gesucht(*s))
            .map(|(n, _)| n.as_str())
            .collect();
        n.join(", ")
    };
    let mut out = Vec::new();
    let fremd = mit(|s| s == Status::FremdesZertifikat);
    if !fremd.is_empty() {
        let klasse = if portal {
            Klasse::Auffaellig
        } else {
            Klasse::Alarm
        };
        out.push((
            klasse,
            format!(
                "Für {fremd} kommt ein anderes Zertifikat als bei der Kontrollseite. Vielleicht tauscht eine Zwischenstation im Netz es aus."
            ),
        ));
    }
    let gesperrt = mit(|s| matches!(s, Status::Zurueckgesetzt | Status::Zeitueberschreitung));
    if !gesperrt.is_empty() {
        let neu = alt.is_some_and(|a| {
            gesperrt
                .split(", ")
                .any(|n| !a.gestoert.iter().any(|g| g == n))
        });
        let (klasse, zusatz) = if meins && neu {
            (
                Klasse::Alarm,
                " In einem Netz, das du als „meins“ markiert hast, war das bei der letzten Messung noch nicht so.",
            )
        } else {
            (Klasse::Auffaellig, "")
        };
        out.push((
            klasse,
            format!(
                "Filter im Netz: Verbindungen zu {gesperrt} brechen schon beim Verbindungsaufbau ab, die Kontrollseite geht durch.{zusatz}"
            ),
        ));
    }
    out
}

/// Für `pruefen`: misst, speichert die gestörten Namen und vergleicht mit dem letzten Besuch.
/// Fällt die Kontrolle aus oder lässt sich nicht speichern, bleibt die Liste leer.
pub fn fuer_pruefen(path: &Path, key: &str, meins: bool, portal: bool) -> Vec<(Klasse, String)> {
    let Ok(erg) = messen() else {
        return Vec::new();
    };
    if erg.unklar {
        return Vec::new();
    }
    let datei = path.with_file_name("sni.json");
    let Ok(mut store) = netze::load_json::<Store>(&datei) else {
        return Vec::new();
    };
    let neu = Gespeichert {
        zeit: netze::now(),
        gestoert: erg.gestoert.clone(),
    };
    let alt = store.insert(key.to_string(), neu);
    if netze::save(&datei, &store).is_err() {
        return Vec::new();
    }
    befunde(alt.as_ref(), &erg, meins, portal)
}

fn wort(s: Status) -> &'static str {
    match s {
        Status::Ok => "ok",
        Status::Zurueckgesetzt => "Verbindung abgebrochen",
        Status::Zeitueberschreitung => "Zeitüberschreitung",
        Status::FremdesZertifikat => "fremdes Zertifikat",
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    if let Some(a) = args.first() {
        return Err(format!(
            "`netzinfo sni` kennt „{a}“ nicht. Es braucht keine weiteren Angaben, `--erklaer` erklärt die Schritte."
        ));
    }
    explain(
        explain_on,
        "Beim Verbindungsaufbau verrät dein Gerät im Klartext, zu welcher Seite es will (SNI, der Servername im TLS-Hallo). Ein Webfilter im Netz kann daran Seiten sperren, auch wenn DNS und Zertifikate unauffällig sind.",
    );
    explain(
        explain_on,
        &format!(
            "Ich verbinde mich fünfmal mit 1.1.1.1:443, das viele Namen annimmt: mit dem neutralen Namen {KONTROLLE} als Kontrolle, mit einem erfundenen Namen als Referenz für das Standardzertifikat des Servers, dann mit {}. Ich breche jede Verbindung ab, sobald das Zertifikat da ist, und schicke keine Daten.",
            NAMEN.join(", ")
        ),
    );
    let erg = messen()?;
    println!("Kontrolle ({KONTROLLE}): {}", wort(erg.kontrolle));
    for (n, s) in &erg.namen {
        println!("{n}: {}", wort(*s));
    }
    if erg.unklar {
        println!("Urteil: unklar");
        explain(
            explain_on,
            "Schon die Kontrolle kam nicht durch. Dann sagt ein Ausfall bei den anderen Namen nichts: Der Testserver ist nicht erreichbar oder das Netz ist gestört. Versuche es später noch mal.",
        );
    } else if erg.gestoert.is_empty() {
        println!("Urteil: kein Name gestört");
        explain(
            explain_on,
            "Alle Testnamen kamen durch. Das Netz filtert keine Webseiten nach Namen.",
        );
    } else {
        println!("Urteil: gestört für {}", erg.gestoert.join(", "));
        explain(
            explain_on,
            "Dieses Netz blockiert Verbindungen zu einer Seite schon beim Verbindungsaufbau, am Namen erkannt. Das ist ein Webfilter, kein Fehler deines Geräts. In Schulen und Firmen ist er gewollt. Ein „fremdes Zertifikat“ heißt, dass jemand im Netz die Verbindung übernimmt und ein eigenes Zertifikat zeigt.",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::ServerConfig;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::server::Acceptor;
    use std::net::TcpListener;

    const LEAF: &[u8] = include_bytes!("../testdata/dnstest_leaf.der");
    const KEY: &[u8] = include_bytes!("../testdata/dnstest_leaf_key.der");

    /// Lokaler TLS-Server: setzt die Verbindung zurück, wenn das SNI `gesperrt` ist, sonst
    /// zeigt er das Testzertifikat. Die Aussteller sind überall gleich, der Test prüft nur das Zurücksetzen.
    fn server(gesperrt: &'static str) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for sock in l.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut sock = sock;
                    let mut acceptor = Acceptor::default();
                    let angenommen = loop {
                        if acceptor.read_tls(&mut sock).is_err() {
                            return;
                        }
                        match acceptor.accept() {
                            Ok(Some(a)) => break a,
                            Ok(None) => {}
                            Err(_) => return,
                        }
                    };
                    if angenommen.client_hello().server_name() == Some(gesperrt) {
                        let _ = socket2::SockRef::from(&sock).set_linger(Some(Duration::ZERO));
                        return;
                    }
                    let cfg = ServerConfig::builder_with_provider(Arc::new(
                        rustls::crypto::ring::default_provider(),
                    ))
                    .with_safe_default_protocol_versions()
                    .unwrap()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![CertificateDer::from(LEAF.to_vec())],
                        PrivateKeyDer::try_from(KEY.to_vec()).unwrap(),
                    )
                    .unwrap();
                    if let Ok(mut conn) = angenommen.into_connection(Arc::new(cfg)) {
                        while conn.complete_io(&mut sock).is_ok() {}
                    }
                });
            }
        });
        addr
    }

    #[test]
    fn nur_der_gesperrte_name_ist_gestoert() {
        let addr = server("gesperrt.test");
        let e = messen_gegen(addr, "kontrolle.test", &["gesperrt.test", "frei.test"]).unwrap();
        assert_eq!(e.kontrolle, Status::Ok);
        assert!(!e.unklar);
        assert_eq!(e.namen[0], ("gesperrt.test".into(), Status::Zurueckgesetzt));
        assert_eq!(e.namen[1], ("frei.test".into(), Status::Ok));
        assert_eq!(e.gestoert, ["gesperrt.test"]);
    }

    #[test]
    fn ausgefallene_kontrolle_ist_unklar() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        let e = messen_gegen(addr, "kontrolle.test", &["frei.test"]).unwrap();
        assert!(e.unklar && e.namen.is_empty() && e.gestoert.is_empty());
        assert_ne!(e.kontrolle, Status::Ok);
    }

    fn erg(namen: &[(&str, Status)]) -> SniErgebnis {
        let namen: Vec<_> = namen.iter().map(|(n, s)| (n.to_string(), *s)).collect();
        let gestoert = namen
            .iter()
            .filter(|(_, s)| *s != Status::Ok)
            .map(|(n, _)| n.clone())
            .collect();
        SniErgebnis {
            kontrolle: Status::Ok,
            namen,
            gestoert,
            unklar: false,
        }
    }

    fn alt(gestoert: &[&str]) -> Gespeichert {
        Gespeichert {
            zeit: 1,
            gestoert: gestoert.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn befunde_stufen() {
        let frei = erg(&[("a.test", Status::Ok)]);
        assert!(befunde(None, &frei, true, false).is_empty());

        let filter = erg(&[("a.test", Status::Zurueckgesetzt), ("b.test", Status::Ok)]);
        // Filter allein ist nur auffällig, auch im eigenen Netz ohne frühere Messung.
        assert_eq!(befunde(None, &filter, true, false)[0].0, Klasse::Auffaellig);
        let a = alt(&[]);
        assert_eq!(
            befunde(Some(&a), &filter, false, false)[0].0,
            Klasse::Auffaellig
        );
        // Neu in einem „meins“-Netz: Alarm. War schon da: auffällig.
        assert_eq!(befunde(Some(&a), &filter, true, false)[0].0, Klasse::Alarm);
        let b = alt(&["a.test"]);
        assert_eq!(
            befunde(Some(&b), &filter, true, false)[0].0,
            Klasse::Auffaellig
        );

        let fremd = erg(&[("a.test", Status::FremdesZertifikat)]);
        assert_eq!(befunde(None, &fremd, false, false)[0].0, Klasse::Alarm);
        assert_eq!(befunde(None, &fremd, false, true)[0].0, Klasse::Auffaellig);
    }
}
