//! Uhrzeit des Netzes abgleichen. Issue #69.
//!
//! SNTP (RFC 4330) gegen öffentliche Server und, in „meinen“ Netzen, gegen den Router, dazu der
//! `Date`-Kopf einer HTTPS-Antwort. Versatz = Quelle minus Mac-Uhr. Die Systemzeit wird nie gestellt.
//! ponytail: DHCP-Option 42 wird nicht gelesen (hier.rs kennt sie nicht), der Router ist die
//! Netzquelle; Option 42 nachrüsten, wenn `ipconfig getpacket` sie in Mias Netzen zeigt.

use crate::explain;
use crate::netze;
use crate::pruefen::Klasse;
use crate::statistik::median;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NTP_ZU_UNIX: f64 = 2_208_988_800.0; // Sekunden zwischen 1900 und 1970
const HARMLOS_MS: i64 = 2_000;
const ALARM_MS: i64 = 60_000;
const FRIST: Duration = Duration::from_secs(2);
const SNTP_ZIELE: [&str; 2] = ["time.apple.com", "pool.ntp.org"];
const HTTPS_ZIEL: &str = "www.cloudflare.com";

/// Ergebnis einer Quelle; `versatz_ms` ist `None`, wenn sie nicht antwortete.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZeitBefund {
    pub quelle: String,
    /// Dieses Netz bietet die Quelle an (der Router), nicht eine öffentliche.
    pub netz: bool,
    pub versatz_ms: Option<i64>,
    pub fehler: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZeitErgebnis {
    pub befunde: Vec<ZeitBefund>,
    pub unterschied_netz_zu_oeffentlich_ms: Option<i64>,
    /// `None`: keine öffentliche Quelle antwortete, nichts zu beurteilen.
    pub klasse: Option<Klasse>,
    pub satz: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Gespeichert {
    zeit: u64,
    versatz_ms_netz: Option<i64>,
    versatz_ms_oeffentlich: i64,
}

fn jetzt() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// 48-Byte-Anfrage: Version 4, Modus 3, Transmit-Zeit `t1` (Unix-Sekunden).
fn anfrage(t1: f64) -> [u8; 48] {
    let mut p = [0u8; 48];
    p[0] = 0x23;
    let t = t1 + NTP_ZU_UNIX;
    p[40..44].copy_from_slice(&(t as u32).to_be_bytes());
    p[44..48].copy_from_slice(&((t.fract() * 4_294_967_296.0) as u32).to_be_bytes());
    p
}

fn ntp_sekunden(b: &[u8]) -> f64 {
    let s = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let f = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
    f64::from(s) + f64::from(f) / 4_294_967_296.0 - NTP_ZU_UNIX
}

/// Versatz der Quelle in ms nach `((T2-T1)+(T3-T4))/2`. `t4` ist die Ankunftszeit der Antwort.
fn auswerten(antwort: &[u8], gesendet: &[u8; 48], t4: f64) -> Result<i64, String> {
    if antwort.len() < 48 {
        return Err("Die Zeitantwort war zu kurz.".into());
    }
    if antwort[0] & 7 != 4 || antwort[1] == 0 {
        return Err("Der Zeitserver lehnt Anfragen ab oder ist nicht synchron.".into());
    }
    if antwort[24..32] != gesendet[40..48] {
        return Err("Die Zeitantwort gehört nicht zu meiner Anfrage.".into());
    }
    let (t1, t2, t3) = (
        ntp_sekunden(&gesendet[40..48]),
        ntp_sekunden(&antwort[32..40]),
        ntp_sekunden(&antwort[40..48]),
    );
    Ok((((t2 - t1) + (t3 - t4)) / 2.0 * 1000.0).round() as i64)
}

fn sntp(ziel: SocketAddr) -> Result<i64, String> {
    let sock = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("UDP-Socket nicht möglich ({}).", crate::io_satz(&e)))?;
    let _ = sock.set_read_timeout(Some(FRIST));
    let _ = sock.set_write_timeout(Some(FRIST));
    let p = anfrage(jetzt());
    sock.send_to(&p, ziel)
        .map_err(|e| format!("Senden nicht möglich ({}).", crate::io_satz(&e)))?;
    let mut buf = [0u8; 128];
    let n = sock
        .recv(&mut buf)
        .map_err(|_| "keine Antwort, NTP ist in manchen Netzen gesperrt".to_string())?;
    auswerten(&buf[..n], &p, jetzt())
}

fn sntp_name(host: &str) -> Result<i64, String> {
    let ziel = (host, 123)
        .to_socket_addrs()
        .map_err(|_| format!("{host} ließ sich nicht auflösen"))?
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| format!("{host} hat keine IPv4-Adresse"))?;
    sntp(ziel)
}

/// `Sun, 06 Nov 1994 08:49:37 GMT` zu Unix-Sekunden.
fn http_datum(s: &str) -> Option<i64> {
    let t: Vec<&str> = s.split([' ', ':']).filter(|x| !x.is_empty()).collect();
    let [_, d, m, y, h, mi, se, ..] = t[..] else {
        return None;
    };
    let monate = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = monate.iter().position(|n| *n == m)? as i64 + 1;
    let (y, d) = (y.parse::<i64>().ok()?, d.parse::<i64>().ok()?);
    // Howard Hinnants days_from_civil
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let tage = era * 146_097 + doe - 719_468;
    let uhr =
        h.parse::<i64>().ok()? * 3600 + mi.parse::<i64>().ok()? * 60 + se.parse::<i64>().ok()?;
    Some(tage * 86_400 + uhr)
}

fn date_kopf(antwort: &str) -> Option<i64> {
    antwort.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case("date").then(|| http_datum(v))?
    })
}

fn https_date(host: &str) -> Result<i64, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("TLS ließ sich nicht einrichten ({e})"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| format!("{host} ist kein gültiger Hostname"))?;
    let addr = (host, 443)
        .to_socket_addrs()
        .map_err(|_| format!("{host} ließ sich nicht auflösen"))?
        .next()
        .ok_or_else(|| format!("{host} hat keine Adresse"))?;
    let mut tcp =
        std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(4)).map_err(|e| {
            format!(
                "Verbindung zu {host} nicht möglich ({})",
                crate::io_satz(&e)
            )
        })?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(4)));
    let conn = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|e| format!("TLS ließ sich nicht starten ({e})"))?;
    let mut s = rustls::StreamOwned::new(conn, &mut tcp);
    s.write_all(format!("HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| {
            format!(
                "TLS-Verbindung zu {host} gescheitert ({})",
                crate::io_satz(&e)
            )
        })?;
    let mut kopf = Vec::new();
    let mut buf = [0u8; 1024];
    // Ein Server, der ohne close_notify schließt, gibt einen Fehler; der Kopf steht dann schon da.
    while !kopf.windows(4).any(|w| w == b"\r\n\r\n") && kopf.len() < 16_384 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => kopf.extend_from_slice(&buf[..n]),
        }
    }
    let t = jetzt();
    let d = date_kopf(&String::from_utf8_lossy(&kopf))
        .ok_or_else(|| format!("{host} schickte keinen lesbaren Date-Kopf"))?;
    // Der Kopf ist auf die Sekunde abgeschnitten, die wahre Zeit liegt im Schnitt 0,5 s darüber.
    Ok(((d as f64 + 0.5 - t) * 1000.0).round() as i64)
}

/// Urteil nach Issue #69: unter 2 s harmlos, bis 60 s auffällig, darüber zwischen Netz und
/// öffentlichen Quellen Alarm. `netz` und `oeff` sind Versätze zur Mac-Uhr in ms.
fn einstufen(netz: Option<i64>, oeff: &[i64]) -> Option<(Klasse, String, Option<i64>)> {
    let m = median(&oeff.iter().map(|v| *v as f64).collect::<Vec<_>>())? as i64;
    if let Some(n) = netz {
        let d = n - m;
        if d.abs() >= HARMLOS_MS {
            let klasse = if d.abs() > ALARM_MS {
                Klasse::Alarm
            } else {
                Klasse::Auffaellig
            };
            let satz = format!(
                "Die Uhr, die dieses Netz anbietet, geht {} {}. Dadurch können sichere Verbindungen fehlschlagen.",
                dauer(d),
                if d > 0 { "vor" } else { "nach" }
            );
            return Some((klasse, satz, Some(d)));
        }
    }
    let d = netz.map(|n| n - m);
    Some(if m.abs() < HARMLOS_MS {
        let n = oeff.len() + usize::from(netz.is_some());
        let satz = format!("Deine Uhr stimmt mit {n} unabhängigen Quellen überein.");
        (Klasse::Harmlos, satz, d)
    } else {
        let satz = format!(
            "Die Uhr deines Rechners geht {} {}. Dadurch können sichere Verbindungen fehlschlagen.",
            dauer(m),
            if m > 0 { "nach" } else { "vor" }
        );
        (Klasse::Auffaellig, satz, d)
    })
}

fn dauer(ms: i64) -> String {
    let s = ms.abs() / 1000;
    if s < 120 {
        format!("{s} Sekunden")
    } else {
        format!("{} Minuten", s / 60)
    }
}

/// Fragt alle Quellen gleichzeitig. `router`: Adresse, die als NTP-Quelle des Netzes dient.
pub fn messen(router: Option<Ipv4Addr>) -> ZeitErgebnis {
    type Antwort = (String, Result<i64, String>);
    let befund = |netz, (quelle, r): Antwort| ZeitBefund {
        quelle,
        netz,
        versatz_ms: r.as_ref().ok().copied(),
        fehler: r.err(),
    };
    let (netz_b, oeff_b) = std::thread::scope(|s| {
        let r = router.map(|ip| {
            s.spawn(move || -> Antwort {
                ("Router (NTP)".into(), sntp(SocketAddr::from((ip, 123))))
            })
        });
        let mut o: Vec<_> = SNTP_ZIELE
            .iter()
            .map(|h| s.spawn(move || -> Antwort { (format!("{h} (NTP)"), sntp_name(h)) }))
            .collect();
        o.push(s.spawn(|| -> Antwort {
            (format!("{HTTPS_ZIEL} (HTTPS Date)"), https_date(HTTPS_ZIEL))
        }));
        (
            r.and_then(|h| h.join().ok()).map(|x| befund(true, x)),
            o.into_iter()
                .filter_map(|h| h.join().ok())
                .map(|x| befund(false, x))
                .collect::<Vec<_>>(),
        )
    });
    let oeff: Vec<i64> = oeff_b.iter().filter_map(|b| b.versatz_ms).collect();
    let netz = netz_b.as_ref().and_then(|b| b.versatz_ms);
    let (klasse, satz, unterschied) = match einstufen(netz, &oeff) {
        Some((k, s, d)) => (Some(k), s, d),
        None => (
            None,
            "Nicht geprüft: Keine öffentliche Zeitquelle antwortete. NTP ist in manchen Netzen gesperrt."
                .into(),
            None,
        ),
    };
    ZeitErgebnis {
        befunde: netz_b.into_iter().chain(oeff_b).collect(),
        unterschied_netz_zu_oeffentlich_ms: unterschied,
        klasse,
        satz,
    }
}

/// Wie `messen`; der Router wird nur in „meinen“ Netzen angefragt (Schutzregel wie bei Geräten).
pub fn pruefen() -> ZeitErgebnis {
    messen(router_wenn_meins(netze::current_is_meins()))
}

fn router_wenn_meins(meins: bool) -> Option<Ipv4Addr> {
    if meins {
        crate::hier::collect().ok()?.router
    } else {
        None
    }
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let e = messen(router_wenn_meins(meins));
    let Some(klasse) = e.klasse else {
        return vec![(Klasse::Harmlos, e.satz)];
    };
    let pfad = daten.with_file_name("zeit.json");
    if let Ok(mut speicher) = netze::load_json::<BTreeMap<String, Gespeichert>>(&pfad) {
        let oeff: Vec<f64> = e
            .befunde
            .iter()
            .filter(|b| !b.netz)
            .filter_map(|b| b.versatz_ms)
            .map(|v| v as f64)
            .collect();
        speicher.insert(
            key.to_string(),
            Gespeichert {
                zeit: netze::now(),
                versatz_ms_netz: e.befunde.iter().find(|b| b.netz).and_then(|b| b.versatz_ms),
                versatz_ms_oeffentlich: median(&oeff).unwrap_or(0.0) as i64,
            },
        );
        let _ = netze::save(&pfad, &speicher);
    }
    vec![(klasse, e.satz)]
}

pub const OPTIONEN: &[&str] = &["--json"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "zeit")?;
    let json = match args.first().map(String::as_str) {
        None => false,
        Some("--json") if args.len() == 1 => true,
        Some(a) => {
            return Err(format!(
                "Unbekannte Option „{a}“. `netzinfo zeit` kennt nur `--json`."
            ));
        }
    };
    explain(
        explain_on,
        "Ich frage Zeitserver per NTP: ein kleines UDP-Paket an Port 123, die Antwort enthält die Uhrzeit des Servers. Aus der Laufzeit rechne ich aus, wie weit deine Uhr abweicht.",
    );
    explain(
        explain_on,
        "Zusätzlich lese ich den Date-Kopf einer HTTPS-Antwort. Der ist nur auf die Sekunde genau, dient aber als unabhängige Gegenprobe.",
    );
    let router = router_wenn_meins(netze::current_is_meins());
    explain(
        explain_on,
        if router.is_some() {
            "Dieses Netz ist als „meins“ markiert, deshalb frage ich auch den Router, ob er die Uhrzeit anbietet."
        } else {
            "Den Router frage ich nur in Netzen, die du als „meins“ markiert hast."
        },
    );
    let e = messen(router);
    if json {
        let s = serde_json::to_string_pretty(&e).map_err(|x| format!("JSON-Fehler: {x}"))?;
        println!("{s}");
        return Ok(());
    }
    for b in &e.befunde {
        match (b.versatz_ms, &b.fehler) {
            (Some(v), _) => println!("{:<30} {:+} ms", b.quelle, v),
            (None, f) => println!(
                "{:<30} {}",
                b.quelle,
                f.as_deref().unwrap_or("keine Antwort")
            ),
        }
    }
    println!("{}", e.satz);
    explain(
        explain_on,
        "Unter 2 Sekunden Abweichung ist harmlos, bis 60 Sekunden auffällig. Geht die Uhr des Netzes mehr als 60 Sekunden anders als die öffentlichen Quellen, schlägt `netzinfo pruefen` Alarm, weil Zertifikate dann zu früh oder zu spät gelten würden.",
    );
    explain(
        explain_on,
        "Ich stelle nie deine Uhr, ich zeige nur die Abweichung.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Echte Antwort von time.apple.com auf die Anfrage mit Transmit-Zeit eb0f3a1c80000000.
    const ANTWORT: &str = "240106e9000000000000000850545073ee8d7b565593bc72eb0f3a1c80000000ee8d7b575a129a18ee8d7b575a12f3b8";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn gesendet() -> [u8; 48] {
        let mut p = [0u8; 48];
        p[0] = 0x23;
        p[40..48].copy_from_slice(&hex("eb0f3a1c80000000"));
        p
    }

    #[test]
    fn aufgezeichnete_antwort() {
        let a = hex(ANTWORT);
        let t3 = ntp_sekunden(&a[40..48]);
        // Die Anfrage trug einen festen Dummy-T1 (nicht die echte Sendezeit), darum ist der Versatz
        // riesig, aber für diese Bytes eindeutig; geprüft wird, dass Parser und Formel stabil bleiben.
        assert_eq!(t3.floor() as i64, 1_793_260_759);
        let v = auswerten(&a, &gesendet(), t3 + 0.02).unwrap();
        assert_eq!(v, 29_302_941_416);
        let mut fremd = gesendet();
        fremd[47] = 1;
        assert!(auswerten(&a, &fremd, t3).is_err());
        assert!(auswerten(&a[..40], &gesendet(), t3).is_err());
        let mut kod = a.clone();
        kod[1] = 0;
        assert!(auswerten(&kod, &gesendet(), t3).is_err());
    }

    #[test]
    fn versatzformel_zahlenbeispiel() {
        // T1=0, T2=5,05, T3=5,06, T4=0,1: ((5,05-0)+(5,06-0,1))/2 = 5,005 s
        let mut p = [0u8; 48];
        p[40..44].copy_from_slice(&(NTP_ZU_UNIX as u32).to_be_bytes());
        let mut a = [0u8; 48];
        a[0] = 0x24;
        a[1] = 2;
        a[24..32].copy_from_slice(&p[40..48]);
        let ts = |s: f64| ((s + NTP_ZU_UNIX) * 4_294_967_296.0) as u64;
        a[32..40].copy_from_slice(&ts(5.05).to_be_bytes());
        a[40..48].copy_from_slice(&ts(5.06).to_be_bytes());
        assert_eq!(auswerten(&a, &p, 0.1).unwrap(), 5_005);
    }

    #[test]
    fn anfrage_aufbau() {
        let p = anfrage(1_000.5);
        assert_eq!(p[0], 0x23);
        assert!((ntp_sekunden(&p[40..48]) - 1_000.5).abs() < 1e-6);
    }

    #[test]
    fn schwellen() {
        let k = |n, o: &[i64]| einstufen(n, o).unwrap().0;
        assert_eq!(k(None, &[100, -300]), Klasse::Harmlos);
        assert_eq!(k(Some(1_900), &[0, 0]), Klasse::Harmlos);
        assert_eq!(k(Some(2_000), &[0, 0]), Klasse::Auffaellig);
        assert_eq!(k(Some(60_000), &[0, 0]), Klasse::Auffaellig);
        assert_eq!(k(Some(60_001), &[0, 0]), Klasse::Alarm);
        assert_eq!(k(Some(-240_000), &[0, 0]), Klasse::Alarm);
        // Alle Quellen weichen gleich ab: die Uhr des Macs geht falsch, nicht das Netz.
        let (kl, satz, d) = einstufen(Some(300_000), &[300_100, 299_900]).unwrap();
        assert_eq!(kl, Klasse::Auffaellig);
        assert!(
            satz.contains("Uhr deines Rechners") && satz.contains("nach"),
            "{satz}"
        );
        assert_eq!(d, Some(0));
        assert!(einstufen(Some(5), &[]).is_none());
    }

    #[test]
    fn date_kopf_lesen() {
        assert_eq!(
            http_datum("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        let kopf = "HTTP/1.1 200 OK\r\nServer: x\r\ndate: Thu, 01 Jan 1970 00:01:00 GMT\r\n\r\n";
        assert_eq!(date_kopf(kopf), Some(60));
        assert_eq!(date_kopf("HTTP/1.1 200 OK\r\n\r\n"), None);
        assert_eq!(http_datum("Quatsch"), None);
    }
}
