//! `netzinfo dns --verschluesselt`: lässt das Netz DNS über TLS (853) und DNS über HTTPS (443) zu,
//! und bietet es selbst einen verschlüsselten Resolver an (DDR, RFC 9462)? Issue #33.
//! Nur lesen: die DNS-Einstellung des Systems wird nie geändert, ein angebotener Resolver nie benutzt.

use crate::explain;
use crate::netze;
use crate::pruefen::Klasse;
use crate::{dns, hier};
use rustls::pki_types::ServerName;
use rustls::{CertificateError, ClientConfig, ClientConnection, Error, RootCertStore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Öffentliche Resolver Dritter (Cloudflare, Quad9): Adresse und Name im Zertifikat.
const DOT_ZIELE: [([u8; 4], &str); 2] = [
    ([1, 1, 1, 1], "cloudflare-dns.com"),
    ([9, 9, 9, 9], "dns.quad9.net"),
];
const DOH_ZIEL: ([u8; 4], &str, &str) = ([1, 1, 1, 1], "cloudflare-dns.com", "/dns-query");
const TESTNAME: &str = "example.org";
const FRIST: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Erreichbar,
    /// Verbindung abgelehnt, Zeitüberschreitung oder Reset.
    Blockiert,
    /// Das Zertifikat passt nicht zum Namen oder stammt von einer unbekannten Stelle.
    Abgefangen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsVerschluesselung {
    pub dot: Status,
    pub doh: Status,
    /// SVCB-Einträge von `_dns.resolver.arpa.`; `None`: das Netz bietet nichts an.
    pub ddr: Option<Vec<String>>,
    /// Unix-Sekunden.
    pub zeit: u64,
}

#[derive(Debug, Serialize)]
pub struct DnsErgebnis {
    /// Name des gemerkten Netzes; `None`: nicht gemerkt, nichts gespeichert.
    pub netz: Option<String>,
    pub messung: DnsVerschluesselung,
    /// Messung des letzten Besuchs.
    pub vorher: Option<DnsVerschluesselung>,
}

type Store = BTreeMap<String, DnsVerschluesselung>;
type Lesen = fn(&mut dyn Read) -> io::Result<Vec<u8>>;

fn abgefangen(e: &io::Error) -> bool {
    matches!(
        e.get_ref().and_then(|i| i.downcast_ref::<Error>()),
        Some(Error::InvalidCertificate(
            CertificateError::UnknownIssuer
                | CertificateError::NotValidForName
                | CertificateError::NotValidForNameContext { .. }
        ))
    )
}

/// TLS-Verbindung zu `addr`, `anfrage` senden, mit `lesen` die Antwort holen.
/// `Err` ist schon das Urteil: Zertifikatsfehler = Abgefangen, alles andere = Blockiert.
fn gespraech(
    addr: SocketAddr,
    sni: &str,
    roots: Arc<RootCertStore>,
    anfrage: &[u8],
    lesen: Lesen,
) -> Result<Vec<u8>, Status> {
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| Status::Blockiert)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    let name = ServerName::try_from(sni.to_string()).map_err(|_| Status::Blockiert)?;
    let mut sock = TcpStream::connect_timeout(&addr, FRIST).map_err(|_| Status::Blockiert)?;
    let _ = sock.set_read_timeout(Some(FRIST));
    let _ = sock.set_write_timeout(Some(FRIST));
    let mut conn = ClientConnection::new(Arc::new(config), name).map_err(|_| Status::Blockiert)?;
    let urteil = |e: io::Error| {
        if abgefangen(&e) {
            Status::Abgefangen
        } else {
            Status::Blockiert
        }
    };
    while conn.is_handshaking() {
        conn.complete_io(&mut sock).map_err(urteil)?;
    }
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    tls.write_all(anfrage).map_err(urteil)?;
    lesen(&mut tls).map_err(urteil)
}

/// DoT (RFC 7858): 2 Byte Länge vor jeder Nachricht.
fn dot_pruefen(addr: SocketAddr, sni: &str, roots: Arc<RootCertStore>) -> Status {
    let id = dns::random_id();
    let Ok(q) = dns::build_query(id, TESTNAME, dns::A, dns::IN) else {
        return Status::Blockiert;
    };
    let mut paket = (q.len() as u16).to_be_bytes().to_vec();
    paket.extend(q);
    let lesen: Lesen = |r| {
        let mut len = [0u8; 2];
        r.read_exact(&mut len)?;
        let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
        r.read_exact(&mut buf)?;
        Ok(buf)
    };
    match gespraech(addr, sni, roots, &paket, lesen) {
        Ok(a) => antwort_status(&a, id),
        Err(s) => s,
    }
}

fn antwort_status(buf: &[u8], id: u16) -> Status {
    match dns::parse_response(buf, id, TESTNAME, dns::A, dns::IN) {
        Ok(_) => Status::Erreichbar,
        Err(_) => Status::Blockiert,
    }
}

/// DoH (RFC 8484), POST mit `application/dns-message`, HTTP/1.1 ohne Keep-Alive.
fn doh_pruefen(addr: SocketAddr, sni: &str, pfad: &str, roots: Arc<RootCertStore>) -> Status {
    let id = dns::random_id();
    let Ok(q) = dns::build_query(id, TESTNAME, dns::A, dns::IN) else {
        return Status::Blockiert;
    };
    let mut paket = format!(
        "POST {pfad} HTTP/1.1\r\nHost: {sni}\r\nContent-Type: application/dns-message\r\nAccept: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        q.len()
    )
    .into_bytes();
    paket.extend(q);
    let lesen: Lesen = |r| {
        let mut v = Vec::new();
        // Ohne close_notify endet das Lesen mit einem Fehler; was bis dahin kam, zählt.
        let res = r.take(1 << 16).read_to_end(&mut v);
        if v.is_empty() {
            res?;
        }
        Ok(v)
    };
    match gespraech(addr, sni, roots, &paket, lesen) {
        Ok(raw) => http_body(&raw).map_or(Status::Blockiert, |b| antwort_status(&b, id)),
        Err(s) => s,
    }
}

/// Der Rumpf einer HTTP/1.1-Antwort mit Status 200, auch wenn er in Stücken (chunked) kommt.
fn http_body(raw: &[u8]) -> Option<Vec<u8>> {
    let kopf_ende = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let kopf = String::from_utf8_lossy(&raw[..kopf_ende]).to_ascii_lowercase();
    if !kopf.starts_with("http/1.1 200") && !kopf.starts_with("http/1.0 200") {
        return None;
    }
    let mut rest = &raw[kopf_ende + 4..];
    if !kopf.contains("transfer-encoding: chunked") {
        return Some(rest.to_vec());
    }
    let mut body = Vec::new();
    loop {
        let z = rest.windows(2).position(|w| w == b"\r\n")?;
        let n = usize::from_str_radix(std::str::from_utf8(&rest[..z]).ok()?.trim(), 16).ok()?;
        if n == 0 {
            return Some(body);
        }
        // n kommt aus der Antwort: z + 2 + n darf nicht überlaufen
        let ende = z.checked_add(2)?.checked_add(n)?;
        body.extend_from_slice(rest.get(z + 2..ende)?);
        rest = rest.get(ende.checked_add(2)?..)?;
    }
}

/// Mehrere Ziele, ein Urteil: Abgefangen schlägt Erreichbar schlägt Blockiert.
fn zusammen(s: &[Status]) -> Status {
    [Status::Abgefangen, Status::Erreichbar]
        .into_iter()
        .find(|x| s.contains(x))
        .unwrap_or(Status::Blockiert)
}

fn ddr_suchen(server: Option<IpAddr>) -> Option<Vec<String>> {
    let a = dns::query(server?, "_dns.resolver.arpa", dns::SVCB, dns::IN).ok()?;
    let v: Vec<String> = a
        .records
        .into_iter()
        .filter(|r| r.rtype == dns::SVCB)
        .map(|r| r.data)
        .collect();
    (!v.is_empty()).then_some(v)
}

/// Die Prüfungen laufen gleichzeitig. `ddr_server`: DNS-Server aus dem DHCP, sonst der des Systems.
pub fn messen(ddr_server: Option<Ipv4Addr>) -> DnsVerschluesselung {
    let roots = Arc::new(RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    });
    let server = ddr_server
        .map(IpAddr::V4)
        .or_else(|| dns::system_resolver().ok());
    let sock = |ip: [u8; 4], port| SocketAddr::from((ip, port));
    std::thread::scope(|s| {
        let dots: Vec<_> = DOT_ZIELE
            .iter()
            .map(|(ip, sni)| s.spawn(|| dot_pruefen(sock(*ip, 853), sni, roots.clone())))
            .collect();
        let doh = s.spawn(|| {
            let (ip, sni, pfad) = DOH_ZIEL;
            doh_pruefen(sock(ip, 443), sni, pfad, roots.clone())
        });
        let ddr = s.spawn(|| ddr_suchen(server));
        let aus =
            |h: std::thread::ScopedJoinHandle<'_, Status>| h.join().unwrap_or(Status::Blockiert);
        let dot: Vec<Status> = dots.into_iter().map(aus).collect();
        DnsVerschluesselung {
            dot: zusammen(&dot),
            doh: aus(doh),
            ddr: ddr.join().unwrap_or(None),
            zeit: netze::now(),
        }
    })
}

/// Legt `neu` unter `key` ab und gibt die Messung davor zurück.
fn speichern(
    path: &Path,
    key: &str,
    neu: &DnsVerschluesselung,
) -> Result<Option<DnsVerschluesselung>, String> {
    let _sperre = crate::sperre(path)?;
    let mut store: Store = netze::load_json(path)?;
    let alt = store.insert(key.to_string(), neu.clone());
    netze::save(path, &store)?;
    Ok(alt)
}

/// Speicherdatei neben `netze.json`.
pub fn store_path() -> Result<std::path::PathBuf, String> {
    Ok(netze::data_path()?.with_file_name("dnsverschl.json"))
}

/// Eine Messung gilt einen Tag als frisch; danach misst „Dieses Netz“ beim Öffnen neu.
const FRISCH_SEKUNDEN: u64 = 86400;

#[derive(Debug, PartialEq, Serialize)]
pub struct Gespeichert {
    pub messung: DnsVerschluesselung,
    /// Jünger als ein Tag: die Oberfläche misst dann nicht neu.
    pub frisch: bool,
}

fn gespeichert_aus(
    netze_pfad: &Path,
    store_pfad: &Path,
    name: &str,
    jetzt: u64,
) -> Result<Option<Gespeichert>, String> {
    let list = netze::load(netze_pfad)?;
    let Some(n) = list.iter().find(|n| n.name == name) else {
        return Err(format!(
            "Das Netz „{name}“ ist nicht gemerkt. Sieh unter „Gemerkte Netze“ nach."
        ));
    };
    let mut store: Store = netze::load_json(store_pfad)?;
    Ok(store
        .remove(&n.fingerprint.key())
        .map(|messung| Gespeichert {
            frisch: jetzt.saturating_sub(messung.zeit) < FRISCH_SEKUNDEN,
            messung,
        }))
}

/// Letzte gespeicherte Messung eines gemerkten Netzes, ohne etwas zu senden. `None`: noch nie gemessen.
pub fn gespeichert(name: &str) -> Result<Option<Gespeichert>, String> {
    gespeichert_aus(&netze::data_path()?, &store_path()?, name, netze::now())
}

/// Misst im aktuellen Netz; ist es gemerkt, kommt die Messung in den Speicher.
pub fn dns_daten() -> Result<DnsErgebnis, String> {
    let info = hier::collect()?;
    let messung = messen(info.dhcp_dns.first().copied());
    let path = netze::data_path()?;
    let bekannt = netze::fingerprint(&info).ok().and_then(|fp| {
        let list = netze::load(&path).ok()?;
        netze::find(&list, &fp).map(|i| (fp.key(), list[i].name.clone()))
    });
    let (netz, vorher) = match bekannt {
        Some((key, name)) => (Some(name), speichern(&store_path()?, &key, &messung)?),
        None => (None, None),
    };
    Ok(DnsErgebnis {
        netz,
        messung,
        vorher,
    })
}

/// Was sich zur letzten Messung geändert hat. Gesperrtes DoT/DoH allein ist harmlos (viele Firmen-
/// und Schulnetze sperren es), nur der Wechsel von frei auf gesperrt fällt auf. Ein falsches
/// Zertifikat bei einem bekannten öffentlichen Resolver ist Alarm, hinter einem Portal nur auffällig.
pub fn unterschiede(
    old: &DnsVerschluesselung,
    new: &DnsVerschluesselung,
    portal: bool,
) -> Vec<(Klasse, String)> {
    let mut out = Vec::new();
    for (name, o, n) in [
        ("DNS über TLS (Port 853)", old.dot, new.dot),
        ("DNS über HTTPS (Port 443)", old.doh, new.doh),
    ] {
        if n == Status::Abgefangen {
            out.push((
                if portal { Klasse::Auffaellig } else { Klasse::Alarm },
                format!("{name} zu einem öffentlichen Resolver zeigt ein falsches Zertifikat. Jemand im Netz fängt die verschlüsselte DNS-Verbindung ab."),
            ));
        } else if o == Status::Erreichbar && n == Status::Blockiert {
            out.push((
                Klasse::Auffaellig,
                format!("{name} ist in diesem Netz jetzt gesperrt, vorher ging es. Deine Namensanfragen wären hier nicht mehr verschlüsselt, falls du es eingeschaltet hast."),
            ));
        }
    }
    out
}

/// Für `pruefen`: misst, speichert und vergleicht mit dem letzten Besuch. Ein Speicherfehler
/// lässt den Vergleich weg, er bricht die Prüfung nicht ab.
pub fn fuer_pruefen(
    path: &Path,
    key: &str,
    ddr_server: Option<Ipv4Addr>,
    portal: bool,
) -> Vec<(Klasse, String)> {
    let neu = messen(ddr_server);
    match speichern(path, key, &neu) {
        Ok(Some(alt)) => unterschiede(&alt, &neu, portal),
        _ => Vec::new(),
    }
}

fn wort(s: Status) -> &'static str {
    match s {
        Status::Erreichbar => "erreichbar",
        Status::Blockiert => "blockiert",
        Status::Abgefangen => "abgefangen (falsches Zertifikat)",
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, &[], "dns --verschluesselt")?;
    if let Some(a) = args.first() {
        return Err(format!(
            "`netzinfo dns --verschluesselt` kennt „{a}“ nicht. Es braucht keine weiteren Angaben, `--erklaer` erklärt die Schritte."
        ));
    }
    explain(
        explain_on,
        "Normales DNS läuft im Klartext: Jeder im Netz kann mitlesen, welche Namen du abfragst. DNS über TLS (Port 853) und DNS über HTTPS (Port 443) verschlüsseln diese Anfragen. Ich prüfe nur, ob dein Netz sie durchlässt. Deine DNS-Einstellung ändere ich nie.",
    );
    explain(
        explain_on,
        "Ich verbinde mich mit 1.1.1.1 (Cloudflare) und 9.9.9.9 (Quad9), prüfe das Zertifikat gegen die Mozilla-Wurzeln und frage nach example.org. Passt das Zertifikat nicht, sitzt jemand dazwischen.",
    );
    explain(
        explain_on,
        "Zusätzlich frage ich den DNS-Server deines Netzes nach `_dns.resolver.arpa` (Typ SVCB, DDR). So sagt ein Resolver, dass er auch verschlüsselt erreichbar ist. Die Antwort ist nicht beglaubigt, ich zeige sie nur an und nutze den Resolver nicht.",
    );
    let r = dns_daten()?;
    let m = &r.messung;
    for (name, s) in [
        ("DNS über TLS (Port 853)", m.dot),
        ("DNS über HTTPS (Port 443)", m.doh),
    ] {
        println!("{name}: {}", wort(s));
    }
    match &m.ddr {
        Some(v) => {
            println!("Verschlüsselter Resolver im Netz (DDR):");
            v.iter().for_each(|e| println!("  {e}"));
        }
        None => crate::satz("Verschlüsselter Resolver im Netz (DDR): keiner angeboten"),
    }
    match m.dot {
        Status::Erreichbar => explain(
            explain_on,
            "DNS über TLS ist erreichbar. Deine Namensanfragen können in diesem Netz nicht mitgelesen werden, wenn du es einschaltest.",
        ),
        Status::Blockiert => explain(
            explain_on,
            "Port 853 ist gesperrt. Das ist in vielen Firmennetzen normal und kein Zeichen für einen Angriff.",
        ),
        Status::Abgefangen => explain(
            explain_on,
            "Der Resolver zeigt ein Zertifikat, das nicht zu ihm gehört. Entweder lenkt das Netz die Verbindung um (Firmenfilter, Captive Portal) oder jemand fängt sie ab. Melde dich an, falls das Netz eine Anmeldung will, und prüfe noch mal.",
        ),
    }
    match (&r.netz, &r.vorher) {
        (None, _) => crate::satz(
            "Dieses Netz ist nicht gemerkt, das Ergebnis wird nicht gespeichert. Merke es mit `netzinfo merken <Name>`.",
        ),
        (Some(n), Some(v)) => println!(
            "Netz „{n}“, letzte Messung am {}: DoT {}, DoH {}.",
            netze::format_utc(v.zeit),
            wort(v.dot),
            wort(v.doh)
        ),
        (Some(n), None) => crate::satz(&format!(
            "Netz „{n}“: erste Messung, sie ist ab jetzt die Vergleichsbasis."
        )),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::{ServerConfig, ServerConnection};
    use std::net::TcpListener;

    const CA: &[u8] = include_bytes!("../testdata/dnstest_ca.der");
    const LEAF: &[u8] = include_bytes!("../testdata/dnstest_leaf.der");
    const KEY: &[u8] = include_bytes!("../testdata/dnstest_leaf_key.der");

    fn roots(ders: &[&[u8]]) -> Arc<RootCertStore> {
        let mut r = RootCertStore::empty();
        for d in ders {
            r.add(CertificateDer::from(d.to_vec())).unwrap();
        }
        Arc::new(r)
    }

    /// Beantwortet die Frage in `q` mit einem A-Eintrag (192.0.2.1).
    fn antwort(q: &[u8]) -> Vec<u8> {
        let mut a = q.to_vec();
        a[2] |= 0x80;
        a[7] = 1;
        a.extend([0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 1]);
        a
    }

    /// Lokaler TLS-Server mit dem Testzertifikat für „dns.test“; beantwortet eine Anfrage.
    fn server(doh: bool) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
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
            let Ok((mut sock, _)) = l.accept() else {
                return;
            };
            let mut conn = ServerConnection::new(Arc::new(cfg)).unwrap();
            while conn.is_handshaking() {
                if conn.complete_io(&mut sock).is_err() {
                    return;
                }
            }
            let mut tls = rustls::Stream::new(&mut conn, &mut sock);
            let mut buf = [0u8; 4096];
            let q = if doh {
                let Ok(n) = tls.read(&mut buf) else { return };
                let Some(z) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") else {
                    return;
                };
                buf[z + 4..n].to_vec()
            } else {
                let mut len = [0u8; 2];
                if tls.read_exact(&mut len).is_err() {
                    return;
                }
                let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                if tls.read_exact(&mut q).is_err() {
                    return;
                }
                q
            };
            let a = antwort(&q);
            if doh {
                let kopf = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\n\r\n",
                    a.len()
                );
                let _ = tls.write_all(kopf.as_bytes());
            } else {
                let _ = tls.write_all(&(a.len() as u16).to_be_bytes());
            }
            let _ = tls.write_all(&a);
            let _ = tls.flush();
        });
        addr
    }

    #[test]
    fn dot_trusted_root_is_erreichbar() {
        let addr = server(false);
        assert_eq!(
            dot_pruefen(addr, "dns.test", roots(&[CA])),
            Status::Erreichbar
        );
    }

    #[test]
    fn dot_wrong_name_is_abgefangen() {
        let addr = server(false);
        assert_eq!(
            dot_pruefen(addr, "anders.test", roots(&[CA])),
            Status::Abgefangen
        );
    }

    #[test]
    fn dot_unknown_issuer_is_abgefangen() {
        let addr = server(false);
        let mozilla = Arc::new(RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        });
        assert_eq!(dot_pruefen(addr, "dns.test", mozilla), Status::Abgefangen);
    }

    #[test]
    fn dot_closed_port_is_blockiert() {
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        assert_eq!(
            dot_pruefen(addr, "dns.test", roots(&[CA])),
            Status::Blockiert
        );
    }

    #[test]
    fn doh_trusted_root_is_erreichbar() {
        let addr = server(true);
        assert_eq!(
            doh_pruefen(addr, "dns.test", "/dns-query", roots(&[CA])),
            Status::Erreichbar
        );
    }

    #[test]
    fn http_body_reads_plain_and_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc";
        assert_eq!(http_body(plain), Some(b"abc".to_vec()));
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n1\r\nc\r\n0\r\n\r\n";
        assert_eq!(http_body(chunked), Some(b"abc".to_vec()));
        assert_eq!(http_body(b"HTTP/1.1 403 Forbidden\r\n\r\nx"), None);
        assert_eq!(
            http_body(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
            None
        );
    }

    fn svcb_antwort(rd: &[u8]) -> Vec<u8> {
        let mut a = dns::build_query(7, "_dns.resolver.arpa", dns::SVCB, dns::IN).unwrap();
        a[2] |= 0x80;
        a[7] = 1;
        a.extend([0xC0, 0x0C, 0, 64, 0, 1, 0, 0, 1, 44]);
        a.extend((rd.len() as u16).to_be_bytes());
        a.extend(rd);
        a
    }

    #[test]
    fn svcb_answer_is_taken_apart() {
        let mut rd = vec![0, 1, 3, b'd', b'n', b's', 7];
        rd.extend(b"example");
        rd.extend([0, 0, 1, 0, 7, 2, b'h', b'2', 3, b'd', b'o', b't']);
        rd.extend([0, 3, 0, 2, 0x03, 0x55, 0, 4, 0, 4, 192, 0, 2, 1]);
        let a = svcb_antwort(&rd);
        let r = dns::parse_response(&a, 7, "_dns.resolver.arpa", dns::SVCB, dns::IN).unwrap();
        assert_eq!(
            r.records[0].data,
            "1 dns.example alpn=h2,dot port=853 ipv4hint=192.0.2.1"
        );
        // Parameter länger als der Eintrag: Fehler statt Absturz
        let n = rd.len();
        rd[n - 5] = 40;
        let a = svcb_antwort(&rd);
        assert!(dns::parse_response(&a, 7, "_dns.resolver.arpa", dns::SVCB, dns::IN).is_err());
    }

    fn m(dot: Status, doh: Status) -> DnsVerschluesselung {
        DnsVerschluesselung {
            dot,
            doh,
            ddr: None,
            zeit: 0,
        }
    }

    #[test]
    fn rules_follow_the_issue() {
        use Status::*;
        let frei = m(Erreichbar, Erreichbar);
        // Blockiert allein, ohne Änderung: nichts. Frei -> gesperrt: auffällig.
        assert!(
            unterschiede(&m(Blockiert, Erreichbar), &m(Blockiert, Erreichbar), false).is_empty()
        );
        let u = unterschiede(&frei, &m(Blockiert, Erreichbar), false);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].0, Klasse::Auffaellig);
        // Abgefangen: Alarm, hinter Portal nur auffällig.
        let u = unterschiede(&frei, &m(Abgefangen, Erreichbar), false);
        assert_eq!(u[0].0, Klasse::Alarm);
        let u = unterschiede(&frei, &m(Abgefangen, Erreichbar), true);
        assert_eq!(u[0].0, Klasse::Auffaellig);
    }

    #[test]
    fn several_targets_one_verdict() {
        use Status::*;
        assert_eq!(zusammen(&[Blockiert, Erreichbar]), Erreichbar);
        assert_eq!(zusammen(&[Erreichbar, Abgefangen]), Abgefangen);
        assert_eq!(zusammen(&[Blockiert, Blockiert]), Blockiert);
    }

    #[test]
    fn store_returns_the_previous_measurement() {
        let dir = std::env::temp_dir().join(format!("netzinfo-dv-{}", dns::random_id()));
        let path = dir.join("dnsverschl.json");
        let a = m(Status::Erreichbar, Status::Blockiert);
        assert_eq!(speichern(&path, "k", &a).unwrap(), None);
        let b = m(Status::Blockiert, Status::Blockiert);
        assert_eq!(speichern(&path, "k", &b).unwrap(), Some(a));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stored_measurement_is_fresh_for_one_day() {
        let dir = std::env::temp_dir().join(format!("netzinfo-dv-{}", dns::random_id()));
        let (np, sp) = (dir.join("netze.json"), dir.join("dnsverschl.json"));
        let fp = netze::Fingerprint {
            router: Ipv4Addr::new(192, 0, 2, 1),
            dhcp_server: Ipv4Addr::new(192, 0, 2, 1),
            domain: None,
            subnet: "192.0.2.0/24".into(),
            bssid_hash: None,
        };
        let netz = netze::Netz {
            name: "WG".into(),
            fingerprint: fp.clone(),
            meins: false,
            first_seen: 0,
            last_seen: 0,
        };
        netze::save(&np, &vec![netz]).unwrap();
        // Nie gemessen: nichts. Unbekannter Name: Fehler.
        assert_eq!(gespeichert_aus(&np, &sp, "WG", 1000).unwrap(), None);
        assert!(gespeichert_aus(&np, &sp, "Fremd", 1000).is_err());
        let mut a = m(Status::Erreichbar, Status::Blockiert);
        a.zeit = 1000;
        speichern(&sp, &fp.key(), &a).unwrap();
        let g = |jetzt| gespeichert_aus(&np, &sp, "WG", jetzt).unwrap().unwrap();
        assert!(g(1000 + 86399).frisch);
        assert!(!g(1000 + 86400).frisch);
        assert_eq!(g(1000).messung, a);
        // Uhr zurückgestellt: nicht „aus der Zukunft“ als alt werten.
        assert!(g(10).frisch);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod kaputt_tests {
    #[test]
    fn http_body_haelt_kaputte_antworten_aus() {
        // eine Chunk-Größe nahe usize::MAX darf nicht überlaufen
        let a = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\nab",
            usize::MAX
        );
        assert_eq!(super::http_body(a.as_bytes()), None);
        let echt = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\n\r\n";
        assert_eq!(super::http_body(echt), Some(b"ab".to_vec()));
        for p in crate::kaputte_pakete(echt) {
            let _ = super::http_body(&p);
        }
    }
}
