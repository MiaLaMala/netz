//! Router-Oberfläche passiv einschätzen. Issue #67.
//!
//! Höchstens eine HTTP-Anfrage (`GET /`, ohne Redirect-Folge) und ein TLS-Handshake, nur an das
//! Gateway und nur in „meins“-Netzen. Gelesen werden Server-Kopfzeile, Seitentitel und Zertifikat.
//! Kein Login, keine Formulare, keine weiteren Pfade.

use crate::netze::{self, load_json, save};
use crate::tls::{self, ChainInfo, Verdict};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

const FRIST: Duration = Duration::from_secs(3);
/// Mehr als den Anfang der Startseite lesen wir nie.
const MAX_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Offen,
    /// Der Router lehnt die Verbindung aktiv ab.
    Zu,
    /// Keine Antwort oder ein anderes Problem, mit Satz.
    Fehler(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zert {
    pub selbstsigniert: bool,
    pub abgelaufen: bool,
    /// `JJJJ-MM-TT`.
    pub gueltig_bis: String,
    pub aussteller: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterSeite {
    pub http: Status,
    pub https: Status,
    /// Antwortet `http` mit einer Weiterleitung nach `https://`?
    pub umleitung: bool,
    pub zertifikat: Option<Zert>,
    pub server: Option<String>,
    pub titel: Option<String>,
}

impl RouterSeite {
    /// Der Befund: die Seite ist im Klartext erreichbar und führt nicht auf https.
    pub fn klartext_ohne_umleitung(&self) -> bool {
        self.http == Status::Offen && !self.umleitung
    }
}

/// Ergebnis einer Messung samt Vergleich mit der letzten.
#[derive(Debug, Serialize)]
pub struct Ergebnis {
    pub seite: RouterSeite,
    pub gateway: Ipv4Addr,
    /// Hinweise in ganzen Sätzen, jeder einmal.
    pub hinweise: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Gespeichert {
    zeit: u64,
    seite: RouterSeite,
}

/// Was die Startseite verrät.
#[derive(Debug, Default, PartialEq)]
struct Antwort {
    code: u16,
    server: Option<String>,
    titel: Option<String>,
    /// Eine Weiterleitung, deren Ziel auf `https://` führt.
    nach_https: bool,
}

fn kurz(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then(|| s.chars().take(100).collect())
}

fn parse_antwort(roh: &[u8]) -> Result<Antwort, String> {
    let text = String::from_utf8_lossy(roh);
    let (kopf, rumpf) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut zeilen = kopf.lines();
    let code = zeilen
        .next()
        .filter(|z| z.starts_with("HTTP/"))
        .and_then(|z| z.split_whitespace().nth(1)?.parse().ok())
        .ok_or("Der Router antwortet auf Port 80 nicht wie ein Webserver.")?;
    let mut a = Antwort {
        code,
        ..Antwort::default()
    };
    for z in zeilen {
        let Some((name, wert)) = z.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "server" => a.server = kurz(wert),
            "location" => {
                a.nach_https = [301, 302, 303, 307, 308].contains(&code)
                    && wert.trim().to_ascii_lowercase().starts_with("https://")
            }
            _ => {}
        }
    }
    // ASCII-Kleinschreibung ändert keine Byte-Längen, die Positionen gelten also auch im Original.
    let klein = rumpf.to_ascii_lowercase();
    if let Some(auf) = klein.find("<title") {
        let von = klein[auf..].find('>').map(|i| auf + i + 1);
        let bis = von.and_then(|v| klein[v..].find("</title").map(|i| v + i));
        if let (Some(v), Some(b)) = (von, bis) {
            a.titel = kurz(&rumpf[v..b]);
        }
    }
    Ok(a)
}

/// Wie `crate::io_satz`, nur dass „Ziel nicht erreichbar“ hier „Router nicht erreichbar“ heißt.
pub(crate) fn io_satz(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            "Router nicht erreichbar".into()
        }
        _ => crate::io_satz(e),
    }
}

fn fehlersatz(port: u16, e: &std::io::Error) -> Status {
    let hilfe = "Prüfe die Freigabe „Lokales Netzwerk“ für Netzinfo.";
    match e.kind() {
        ErrorKind::ConnectionRefused => Status::Zu,
        ErrorKind::TimedOut | ErrorKind::WouldBlock => {
            Status::Fehler(format!("Keine Antwort auf Port {port}. {hilfe}"))
        }
        _ => Status::Fehler(format!(
            "Verbindung auf Port {port} nicht möglich ({}). {hilfe}",
            io_satz(e)
        )),
    }
}

/// Eine Anfrage `GET /`, ohne Weiterleitungen zu folgen.
fn http_lesen(addr: SocketAddr) -> (Status, Antwort) {
    let mut sock = match TcpStream::connect_timeout(&addr, FRIST) {
        Ok(s) => s,
        Err(e) => return (fehlersatz(addr.port(), &e), Antwort::default()),
    };
    let _ = sock.set_read_timeout(Some(FRIST));
    let _ = sock.set_write_timeout(Some(FRIST));
    let frage = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nUser-Agent: netzinfo\r\nAccept: text/html\r\nConnection: close\r\n\r\n",
        addr.ip()
    );
    if let Err(e) = sock.write_all(frage.as_bytes()) {
        return (fehlersatz(addr.port(), &e), Antwort::default());
    }
    let mut roh = Vec::new();
    // Ein Lesefehler nach den ersten Bytes (Frist) ist kein Problem, die Kopfzeilen sind dann da.
    let _ = sock.take(MAX_BYTES).read_to_end(&mut roh);
    match parse_antwort(&roh) {
        Ok(a) => (Status::Offen, a),
        Err(s) => (Status::Fehler(s), Antwort::default()),
    }
}

/// Tage seit 1970 als `JJJJ-MM-TT` (Verfahren von Howard Hinnant).
fn datum(tage: u64) -> String {
    let z = tage as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let tag_der_era = z.rem_euclid(146_097);
    let jahr_der_era =
        (tag_der_era - tag_der_era / 1460 + tag_der_era / 36_524 - tag_der_era / 146_096) / 365;
    let tag_im_jahr = tag_der_era - (365 * jahr_der_era + jahr_der_era / 4 - jahr_der_era / 100);
    let mp = (5 * tag_im_jahr + 2) / 153;
    let tag = tag_im_jahr - (153 * mp + 2) / 5 + 1;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 };
    let jahr = jahr_der_era + era * 400 + i64::from(monat <= 2);
    format!("{jahr:04}-{monat:02}-{tag:02}")
}

fn zert_aus(c: &ChainInfo, urteil: &Verdict, heute: &str) -> Zert {
    Zert {
        // Ein Zertifikat, das nicht öffentlich beglaubigt ist und dessen Aussteller das oberste
        // Kettenglied ist, hat sich der Router selbst ausgestellt.
        selbstsigniert: *urteil != Verdict::Normal && c.leaf_issuer == c.top_subject,
        abgelaufen: c.not_after.as_str() < heute,
        gueltig_bis: c.not_after.clone(),
        aussteller: c.leaf_issuer.clone(),
    }
}

fn https_lesen(ziel: Ipv4Addr, heute: &str) -> (Status, Option<Zert>) {
    let bericht = match tls::check_host(&ziel.to_string()) {
        Ok(b) => b,
        Err(e) => return (Status::Fehler(e), None),
    };
    match (&bericht.chain, &bericht.verdict) {
        (Some(c), v) => (Status::Offen, Some(zert_aus(c, v, heute))),
        (None, Verdict::Fehler(m)) if m.contains("refused") => (Status::Zu, None),
        (None, Verdict::Fehler(m)) => (Status::Fehler(m.clone()), None),
        (None, _) => (
            Status::Fehler("Der Router hat kein Zertifikat geschickt.".into()),
            None,
        ),
    }
}

/// Die Schutzregel: nur in „meins“-Netzen und nur das erkannte Gateway.
fn tor(meins: bool, gateway: Option<Ipv4Addr>, ziel: Ipv4Addr) -> Result<(), String> {
    if !meins {
        return Err("`netzinfo router` läuft nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es wurde nichts gesendet.".into());
    }
    match gateway {
        Some(g) if g == ziel => Ok(()),
        Some(_) => Err("Netzinfo fragt nur das Gateway deines Netzes ab, kein anderes Gerät. Es wurde nichts gesendet.".into()),
        None => Err("In diesem Netz ist kein Gateway bekannt (VPN-Tunnel?). Es wurde nichts gesendet.".into()),
    }
}

/// Fragt `ziel` ab, nachdem `tor` es erlaubt hat.
fn abfragen(
    meins: bool,
    gateway: Option<Ipv4Addr>,
    ziel: Ipv4Addr,
    heute: &str,
) -> Result<RouterSeite, String> {
    tor(meins, gateway, ziel)?;
    let (http, antwort) = http_lesen(SocketAddr::from((ziel, 80)));
    let (https, zertifikat) = https_lesen(ziel, heute);
    Ok(RouterSeite {
        http,
        https,
        umleitung: antwort.nach_https,
        zertifikat,
        server: antwort.server,
        titel: antwort.titel,
    })
}

fn hinweise(neu: &RouterSeite, davor: Option<&Gespeichert>) -> Vec<String> {
    let mut v = Vec::new();
    if neu.klartext_ohne_umleitung() {
        v.push("Die Router-Seite ist über http ohne Verschlüsselung erreichbar und führt nicht auf https. Wer im Heimnetz mitlesen kann, sieht dort eingegebene Passwörter.".into());
    }
    if neu.zertifikat.as_ref().is_some_and(|z| z.abgelaufen) {
        v.push("Das Zertifikat des Routers ist abgelaufen.".into());
    }
    let alt_zert = davor.and_then(|d| d.seite.zertifikat.as_ref());
    if let (Some(a), Some(b)) = (alt_zert, neu.zertifikat.as_ref())
        && (a.aussteller != b.aussteller || a.gueltig_bis != b.gueltig_bis)
    {
        v.push("Das Zertifikat des Routers ist ein anderes als bei der letzten Messung. Nach einem Neustart oder einem Firmware-Update ist das üblich.".into());
    }
    if let (Some(a), Some(b)) = (davor.and_then(|d| d.seite.server.as_ref()), &neu.server)
        && a != b
    {
        v.push(format!(
            "Die Server-Kennung des Routers hat sich geändert (vorher „{a}“, jetzt „{b}“)."
        ));
    }
    v
}

/// Misst das Gateway des aktuellen Netzes und speichert das Ergebnis pro Netz.
pub fn router_jetzt() -> Result<Ergebnis, String> {
    let meins = netze::current_is_meins();
    let info = crate::hier::collect()?;
    let gateway = info.router;
    // Ohne Gateway oder „meins“ lehnt `abfragen` ab, bevor etwas gesendet wird.
    let ziel = gateway.unwrap_or(Ipv4Addr::UNSPECIFIED);
    let heute = datum(netze::now() / 86_400);
    let seite = abfragen(meins, gateway, ziel, &heute)?;
    let pfad = netze::data_path()?.with_file_name("routerseite.json");
    let key = netze::fingerprint(&info)?.key();
    let mut alle: BTreeMap<String, Gespeichert> = load_json(&pfad)?;
    let hinweise = hinweise(&seite, alle.get(&key));
    alle.insert(
        key,
        Gespeichert {
            zeit: netze::now(),
            seite: seite.clone(),
        },
    );
    save(&pfad, &alle)?;
    Ok(Ergebnis {
        seite,
        gateway: ziel,
        hinweise,
    })
}

fn status_text(s: &Status) -> String {
    match s {
        Status::Offen => "erreichbar".into(),
        Status::Zu => "geschlossen".into(),
        Status::Fehler(m) => m.clone(),
    }
}

pub const OPTIONEN: &[&str] = &["--json"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "router")?;
    let json = match args.first().map(String::as_str) {
        None => false,
        Some("--json") if args.len() == 1 => true,
        Some(a) => {
            return Err(format!(
                "Unbekannte Option „{a}“. `netzinfo router` kennt nur `--json`."
            ));
        }
    };
    let e = |t: &str| crate::explain(explain_on, t);
    e(
        "Netzinfo schickt zwei Anfragen an dein Gateway, den Router: eine normale Webanfrage für die Startseite auf Port 80 (http) und einen Verschlüsselungs-Handshake auf Port 443 (https). Mehr nicht, auch kein Anmelden.",
    );
    e(
        "Gelesen werden die Kennung des Webservers (Server-Kopfzeile), der Titel der Startseite und das Zertifikat. Weiterleitungen folgt Netzinfo nicht.",
    );
    let r = router_jetzt()?;
    if json {
        let s = serde_json::to_string_pretty(&r).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    let s = &r.seite;
    println!("Router {}", r.gateway);
    println!("  http  (80):  {}", status_text(&s.http));
    if s.http == Status::Offen {
        println!(
            "  Weiterleitung nach https: {}",
            if s.umleitung { "ja" } else { "nein" }
        );
    }
    println!("  https (443): {}", status_text(&s.https));
    if let Some(z) = &s.zertifikat {
        println!("  Zertifikat: ausgestellt von {}", z.aussteller);
        println!(
            "  Gültig bis {}{}{}",
            z.gueltig_bis,
            if z.abgelaufen { " (abgelaufen)" } else { "" },
            if z.selbstsigniert {
                ", selbst ausgestellt"
            } else {
                ""
            }
        );
        if z.selbstsigniert {
            e(
                "Das Zertifikat des Routers ist selbst ausgestellt. Das ist bei Routern üblich und kein Zeichen für einen Angriff.",
            );
        }
    }
    if let Some(v) = &s.server {
        println!("  Server: {v}");
    }
    if let Some(t) = &s.titel {
        println!("  Seitentitel: {t}");
    }
    for h in &r.hinweise {
        println!("  Hinweis: {h}");
    }
    if r.hinweise.is_empty() {
        println!("  Nichts Auffälliges.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn systemfehler_kommen_auf_deutsch() {
        let e = std::io::Error::from_raw_os_error(libc::EHOSTUNREACH); // englisch: „No route to host“
        assert_eq!(io_satz(&e), "Router nicht erreichbar");
        let e = std::io::Error::from_raw_os_error(1); // EPERM
        assert_eq!(io_satz(&e), "keine Berechtigung");
    }

    const BOX: &[u8] = b"HTTP/1.1 200 OK\r\nServer: Beispiel-Router\r\nContent-Type: text/html\r\n\r\n<html><head>\n<TITLE>\n  Beispiel Box  </TITLE></head></html>";
    const UMLEITUNG: &[u8] = b"HTTP/1.1 301 Moved Permanently\r\nlocation: https://192.0.2.1/\r\nServer: lighttpd/1.4\r\n\r\n";
    const NUR_INTERN: &[u8] = b"HTTP/1.0 302 Found\r\nLocation: /login.html\r\n\r\n";
    const KLARTEXT: &[u8] =
        b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1:8080/\r\nServer: x\r\n\r\n";

    fn gw() -> Ipv4Addr {
        Ipv4Addr::new(192, 0, 2, 1)
    }

    #[test]
    fn startseite_wird_gelesen() {
        let a = parse_antwort(BOX).unwrap();
        assert_eq!(a.code, 200);
        assert_eq!(a.server.as_deref(), Some("Beispiel-Router"));
        assert_eq!(a.titel.as_deref(), Some("Beispiel Box"));
        assert!(!a.nach_https);
    }

    #[test]
    fn nur_weiterleitung_nach_https_zaehlt() {
        assert!(parse_antwort(UMLEITUNG).unwrap().nach_https);
        assert!(!parse_antwort(NUR_INTERN).unwrap().nach_https);
        assert!(!parse_antwort(KLARTEXT).unwrap().nach_https);
        let a = parse_antwort(b"HTTP/1.1 200 OK\r\nLocation: https://192.0.2.1/\r\n\r\n").unwrap();
        assert!(!a.nach_https);
    }

    #[test]
    fn kein_webserver_ist_ein_fehler() {
        assert!(parse_antwort(b"SSH-2.0-irgendwas\r\n").is_err());
        assert!(parse_antwort(b"").is_err());
        assert!(parse_antwort(b"HTTP/1.1 abc\r\n\r\n").is_err());
    }

    /// Lokaler Testserver, der genau eine Antwort schickt.
    fn server(antwort: &'static [u8]) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 512];
                let n = s.read(&mut buf).unwrap_or(0);
                // passiv: genau `GET /`, kein anderer Pfad
                assert!(buf[..n].starts_with(b"GET / HTTP/1.1\r\n"));
                let _ = s.write_all(antwort);
            }
        });
        addr
    }

    #[test]
    fn testserver_mit_und_ohne_umleitung() {
        let (st, a) = http_lesen(server(UMLEITUNG));
        assert_eq!((st, a.nach_https), (Status::Offen, true));
        let (st, a) = http_lesen(server(BOX));
        assert_eq!((st, a.nach_https), (Status::Offen, false));
    }

    #[test]
    fn geschlossener_port_ist_zu() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert_eq!(http_lesen(addr).0, Status::Zu);
    }

    #[test]
    fn ziel_ausserhalb_des_gateways_wird_abgelehnt() {
        let fremd = Ipv4Addr::new(192, 0, 2, 77);
        assert!(
            tor(true, Some(gw()), fremd)
                .unwrap_err()
                .contains("nur das Gateway")
        );
        assert!(tor(true, None, gw()).is_err());
        assert!(tor(true, Some(gw()), gw()).is_ok());
        // Ohne „meins“ trägt der Satz die Worte, nach denen die App die Art erkennt.
        let e = tor(false, Some(gw()), gw()).unwrap_err();
        assert_eq!(netze::fehlerart(&e), netze::Fehlerart::NichtMeins);
        // `abfragen` kehrt in beiden Fällen sofort um, ohne etwas zu senden.
        assert!(abfragen(false, Some(gw()), gw(), "2026-01-01").is_err());
        assert!(abfragen(true, Some(gw()), fremd, "2026-01-01").is_err());
    }

    fn kette(aussteller: &str, oben: &str, bis: &str) -> ChainInfo {
        ChainInfo {
            leaf_issuer: aussteller.into(),
            top_subject: oben.into(),
            top_issuer: aussteller.into(),
            not_before: "2020-01-01".into(),
            not_after: bis.into(),
            issuer_sha256: None,
            product: None,
        }
    }

    #[test]
    fn zertifikat_selbstsigniert_und_abgelaufen() {
        let selbst = kette("CN=box", "CN=box", "2025-06-01");
        let z = zert_aus(&selbst, &Verdict::Aufgebrochen, "2026-10-29");
        assert!(z.selbstsigniert && z.abgelaufen);
        let ca = kette("CN=R11", "CN=ISRG Root", "2027-01-01");
        let z = zert_aus(&ca, &Verdict::Normal, "2026-10-29");
        assert!(!z.selbstsigniert && !z.abgelaufen);
    }

    #[test]
    fn datum_aus_tagen() {
        assert_eq!(datum(0), "1970-01-01");
        assert_eq!(datum(20_088), "2024-12-31");
        assert_eq!(datum(20_454), "2026-01-01");
    }

    fn seite(http: Status, umleitung: bool) -> RouterSeite {
        RouterSeite {
            http,
            https: Status::Offen,
            umleitung,
            zertifikat: None,
            server: Some("A".into()),
            titel: None,
        }
    }

    #[test]
    fn hinweise_nur_bei_befund_und_aenderung() {
        assert_eq!(hinweise(&seite(Status::Offen, false), None).len(), 1);
        assert!(hinweise(&seite(Status::Offen, true), None).is_empty());
        assert!(hinweise(&seite(Status::Zu, false), None).is_empty());
        let mut davor = seite(Status::Zu, false);
        davor.server = Some("B".into());
        let g = Gespeichert {
            zeit: 1,
            seite: davor,
        };
        let h = hinweise(&seite(Status::Zu, false), Some(&g));
        assert_eq!(h.len(), 1);
        assert!(h[0].contains("Server-Kennung"));
    }
}
