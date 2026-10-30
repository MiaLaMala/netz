//! `netzinfo portal`: is a captive portal (login page) in the way? Issue #32.
//!
//! Three plain HTTP check URLs of Apple, Google and Mozilla (state: October 2026, they belong to
//! third parties and may change). A portal answers them with a redirect or its own page.
//! Everything received is untrusted: size and time are capped, redirects are not followed,
//! nothing is rendered, only the status code and the redirect host are kept.

use crate::{dns, explain};
use serde::Serialize;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy)]
enum Erwartet {
    /// 200 and the body contains the text.
    Enthaelt(&'static str),
    /// 204 and no content.
    LeerOhneInhalt,
    /// 200 and the body is exactly the text (whitespace around it is ignored).
    Genau(&'static str),
}

/// name, host, path, expected answer
const QUELLEN: [(&str, &str, &str, Erwartet); 3] = [
    (
        "Apple",
        "captive.apple.com",
        "/",
        Erwartet::Enthaelt("Success"),
    ),
    (
        "Google",
        "connectivitycheck.gstatic.com",
        "/generate_204",
        Erwartet::LeerOhneInhalt,
    ),
    (
        "Firefox",
        "detectportal.firefox.com",
        "/success.txt",
        Erwartet::Genau("success"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Urteil {
    Erwartet,
    /// Other status or other content.
    Abweichung,
    /// 301, 302, 303 or 307.
    Umleitung,
    /// No usable answer (timeout, refused, garbage, truncated headers).
    Fehler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    Frei,
    PortalVermutet,
    Unklar,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuellErgebnis {
    pub quelle: String,
    pub host: String,
    pub http_status: Option<u16>,
    pub urteil: Urteil,
    /// Host of the redirect target, never the full URL.
    pub ziel_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortalErgebnis {
    pub status: Status,
    pub ziel_host: Option<String>,
    pub quellen: Vec<QuellErgebnis>,
    /// `captive.apple.com` resolves to a private or loopback address. `None`: not measurable.
    pub dns_verdaechtig: Option<bool>,
}

/// Splits a raw HTTP response into status code, `Location` header and body.
/// `None` for garbage or when the header block is not complete.
pub(crate) fn parse(raw: &[u8]) -> Option<(u16, Option<String>, &[u8])> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    if !first.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let code: u16 = first.next()?.parse().ok()?;
    let location = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("location"))
        .map(|(_, v)| v.trim().to_string());
    Some((code, location, &raw[end + 4..]))
}

/// Host of an absolute `http(s)://` URL. No userinfo, port or query; odd characters give `None`.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_ascii_lowercase();
    let ok = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    ok.then_some(host)
}

/// Judges one raw response against what the source should answer.
fn classify(raw: &[u8], erwartet: Erwartet) -> (Option<u16>, Urteil, Option<String>) {
    let Some((code, location, body)) = parse(raw) else {
        return (None, Urteil::Fehler, None);
    };
    let text = String::from_utf8_lossy(body);
    let ok = match erwartet {
        Erwartet::Enthaelt(t) => code == 200 && text.contains(t),
        Erwartet::LeerOhneInhalt => code == 204 && body.is_empty(),
        Erwartet::Genau(t) => code == 200 && text.trim() == t,
    };
    if ok {
        return (Some(code), Urteil::Erwartet, None);
    }
    if matches!(code, 301 | 302 | 303 | 307) {
        let host = location.as_deref().and_then(host_of);
        return (Some(code), Urteil::Umleitung, host);
    }
    (Some(code), Urteil::Abweichung, None)
}

/// One GET without redirects, at most `MAX_BYTES` and `TIMEOUT` in total.
pub(crate) fn fetch(addr: impl ToSocketAddrs, host: &str, path: &str) -> Result<Vec<u8>, String> {
    let start = Instant::now();
    let left = || {
        TIMEOUT
            .saturating_sub(start.elapsed())
            .max(Duration::from_millis(1))
    };
    let sa = addr
        .to_socket_addrs()
        .map_err(|_| {
            "Namensauflösung fehlgeschlagen. Prüfe den Namen und die Verbindung.".to_string()
        })?
        .next()
        .ok_or("Keine Adresse gefunden.")?;
    let mut s = TcpStream::connect_timeout(&sa, TIMEOUT).map_err(|e| crate::io_satz(&e))?;
    s.set_write_timeout(Some(left()))
        .map_err(|e| crate::io_satz(&e))?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: netzinfo\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())
        .map_err(|e| crate::io_satz(&e))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while buf.len() < MAX_BYTES && start.elapsed() < TIMEOUT {
        s.set_read_timeout(Some(left()))
            .map_err(|e| crate::io_satz(&e))?;
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    buf.truncate(MAX_BYTES);
    Ok(buf)
}

/// Combines the per-source verdicts. Two or more deviating sources mean a portal; one alone
/// is only a hint (that source may be blocked), unless DNS also looks fishy.
fn bewerten(quellen: Vec<QuellErgebnis>, dns_verdaechtig: Option<bool>) -> PortalErgebnis {
    let count = |f: fn(Urteil) -> bool| quellen.iter().filter(|q| f(q.urteil)).count();
    let abw = count(|u| matches!(u, Urteil::Abweichung | Urteil::Umleitung));
    let ok = count(|u| u == Urteil::Erwartet);
    let status = if abw >= 2 || (abw == 1 && dns_verdaechtig == Some(true)) {
        Status::PortalVermutet
    } else if ok >= 2 {
        Status::Frei
    } else {
        Status::Unklar
    };
    let ziel_host = quellen.iter().find_map(|q| q.ziel_host.clone());
    PortalErgebnis {
        status,
        ziel_host,
        quellen,
        dns_verdaechtig,
    }
}

fn private(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
}

/// Asks the three check URLs (in parallel) and the system resolver.
pub fn check() -> PortalErgebnis {
    let quellen = std::thread::scope(|s| {
        let jobs: Vec<_> = QUELLEN
            .iter()
            .map(|&(name, host, path, erw)| {
                s.spawn(move || {
                    let (http_status, urteil, ziel_host) = fetch((host, 80), host, path)
                        .map_or((None, Urteil::Fehler, None), |raw| classify(&raw, erw));
                    QuellErgebnis {
                        quelle: name.into(),
                        host: host.into(),
                        http_status,
                        urteil,
                        ziel_host,
                    }
                })
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    let dns_verdaechtig = dns::system_resolver()
        .and_then(|r| dns::query(r, "captive.apple.com", dns::A, dns::IN))
        .ok()
        .map(|a| {
            a.records
                .iter()
                .filter(|r| r.rtype == dns::A)
                .filter_map(|r| r.data.parse::<Ipv4Addr>().ok())
                .any(private)
        });
    bewerten(quellen, dns_verdaechtig)
}

pub const OPTIONEN: &[&str] = &["--json"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "portal")?;
    let mut json = false;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            _ => {
                return Err(format!(
                    "`netzinfo portal` kennt „{a}“ nicht. Erlaubt ist nur `--json`, `--erklaer` erklärt die Schritte."
                ));
            }
        }
    }
    explain(
        explain_on,
        "Ein Captive Portal ist eine Zwischenseite des Betreibers. Sie fängt deine erste Webanfrage ab und leitet dich zur Anmeldung um.",
    );
    explain(
        explain_on,
        "Ich rufe drei Adressen von Apple, Google und Mozilla ohne Verschlüsselung auf. Normalerweise antworten sie mit einer winzigen, bekannten Antwort. Ein Portal ersetzt sie durch eine Weiterleitung oder eine eigene Seite. Ich folge nichts und öffne nichts, ich lese nur den Statuscode und den Zielrechner.",
    );
    let r = check();
    if json {
        let s = serde_json::to_string_pretty(&r).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    for q in &r.quellen {
        let code = q
            .http_status
            .map_or("keine Antwort".into(), |c| format!("HTTP {c}"));
        let u = match q.urteil {
            Urteil::Erwartet => "wie erwartet",
            Urteil::Abweichung => "weicht ab",
            Urteil::Umleitung => "Weiterleitung",
            Urteil::Fehler => "nicht messbar",
        };
        let to = q
            .ziel_host
            .as_deref()
            .map_or(String::new(), |h| format!(" nach {h}"));
        println!("{} ({}): {code}, {u}{to}", q.quelle, q.host);
    }
    if let Some(d) = r.dns_verdaechtig {
        crate::satz(&format!(
            "captive.apple.com zeigt auf eine private Adresse: {}",
            if d { "ja" } else { "nein" }
        ));
    }
    match r.status {
        Status::Frei => {
            crate::satz("Kein Captive Portal. Das Netz lässt die Prüfseiten unverändert durch.")
        }
        Status::PortalVermutet => {
            let wohin = r
                .ziel_host
                .as_deref()
                .map_or(String::new(), |h| format!(" (Ziel: {h})"));
            crate::satz(&format!(
                "Portal vermutet{wohin}: Anmeldung nötig. Öffne eine Webseite im Browser und melde dich an."
            ));
        }
        Status::Unklar => crate::satz(
            "Unklar: zu wenige Antworten, um ein Portal auszuschließen. Ist das Internet erreichbar?",
        ),
    }
    if r.status == Status::Frei && r.quellen.iter().any(|q| q.urteil != Urteil::Erwartet) {
        crate::satz("Hinweis: Eine Quelle weicht ab, vielleicht ist sie in diesem Netz gesperrt.");
    }
    explain(
        explain_on,
        "Bei mindestens zwei abweichenden Quellen vermute ich ein Portal. Eine allein kann einfach gesperrt sein. Bis zur Anmeldung sind Ergebnisse zu DNS und Zertifikaten nicht aussagekräftig.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const APPLE: Erwartet = Erwartet::Enthaelt("Success");
    const GOOGLE: Erwartet = Erwartet::LeerOhneInhalt;
    const FOX: Erwartet = Erwartet::Genau("success");

    fn c(raw: &str, e: Erwartet) -> (Option<u16>, Urteil, Option<String>) {
        classify(raw.as_bytes(), e)
    }

    #[test]
    fn expected_answers() {
        let r = c(
            "HTTP/1.1 200 OK\r\n\r\n<HTML><BODY>Success</BODY></HTML>",
            APPLE,
        );
        assert_eq!(r, (Some(200), Urteil::Erwartet, None));
        assert_eq!(
            c("HTTP/1.1 204 No Content\r\n\r\n", GOOGLE).1,
            Urteil::Erwartet
        );
        assert_eq!(
            c("HTTP/1.1 200 OK\r\n\r\nsuccess\n", FOX).1,
            Urteil::Erwartet
        );
    }

    #[test]
    fn redirect_reports_host_only() {
        let r = "HTTP/1.1 302 Found\r\nlocation: http://Portal.example:8080/login?user=a&token=secret\r\n\r\n";
        assert_eq!(
            c(r, APPLE),
            (Some(302), Urteil::Umleitung, Some("portal.example".into()))
        );
        // relative or odd target: redirect without host
        let r = "HTTP/1.1 307 Temporary Redirect\r\nLocation: /login\r\n\r\n";
        assert_eq!(c(r, GOOGLE), (Some(307), Urteil::Umleitung, None));
        let r = "HTTP/1.1 302 Found\r\nLocation: http://a b\u{1}/x\r\n\r\n";
        assert_eq!(c(r, GOOGLE).2, None);
    }

    #[test]
    fn foreign_content_deviates() {
        let login = "HTTP/1.1 200 OK\r\n\r\n<html><form>Login</form></html>";
        assert_eq!(c(login, APPLE).1, Urteil::Abweichung);
        assert_eq!(c(login, GOOGLE).1, Urteil::Abweichung);
        assert_eq!(c(login, FOX).1, Urteil::Abweichung);
        // 204 with a body is not what Google sends
        assert_eq!(
            c("HTTP/1.1 204 No Content\r\n\r\nx", GOOGLE).1,
            Urteil::Abweichung
        );
        let r511 = "HTTP/1.1 511 Network Authentication Required\r\n\r\n";
        assert_eq!(c(r511, FOX).1, Urteil::Abweichung);
    }

    #[test]
    fn empty_truncated_and_garbage_are_errors() {
        for raw in [
            "",
            "HTTP/1.1 200 OK\r\nContent-Le",
            "\u{0}\u{1}garbage",
            "HTTP/1.1 abc\r\n\r\n",
            "SSH-2.0-x\r\n\r\n",
        ] {
            assert_eq!(c(raw, APPLE), (None, Urteil::Fehler, None), "{raw:?}");
        }
        let bad = [0xff, 0xfe, b'\r', b'\n', b'\r', b'\n'];
        assert_eq!(classify(&bad, APPLE).1, Urteil::Fehler);
    }

    fn q(u: Urteil) -> QuellErgebnis {
        QuellErgebnis {
            quelle: "x".into(),
            host: "x".into(),
            http_status: None,
            urteil: u,
            ziel_host: (u == Urteil::Umleitung).then(|| "portal.example".into()),
        }
    }

    fn st(a: Urteil, b: Urteil, c: Urteil, dns: Option<bool>) -> Status {
        bewerten(vec![q(a), q(b), q(c)], dns).status
    }

    #[test]
    fn verdict_combinations() {
        use Urteil::*;
        assert_eq!(st(Erwartet, Erwartet, Erwartet, Some(false)), Status::Frei);
        // one source differs: only a hint
        assert_eq!(st(Abweichung, Erwartet, Erwartet, None), Status::Frei);
        assert_eq!(
            st(Umleitung, Umleitung, Erwartet, None),
            Status::PortalVermutet
        );
        assert_eq!(
            st(Umleitung, Abweichung, Abweichung, None),
            Status::PortalVermutet
        );
        // one source plus fishy DNS
        assert_eq!(
            st(Umleitung, Erwartet, Fehler, Some(true)),
            Status::PortalVermutet
        );
        assert_eq!(st(Fehler, Fehler, Fehler, None), Status::Unklar);
        assert_eq!(st(Abweichung, Fehler, Fehler, Some(false)), Status::Unklar);
        assert_eq!(st(Erwartet, Fehler, Fehler, None), Status::Unklar);
        let r = bewerten(vec![q(Umleitung), q(Umleitung), q(Erwartet)], None);
        assert_eq!(r.ziel_host.as_deref(), Some("portal.example"));
        assert!(
            serde_json::to_string(&r)
                .unwrap()
                .contains("PortalVermutet")
        );
    }

    #[test]
    fn private_addresses() {
        assert!(private("192.168.178.1".parse().unwrap()));
        assert!(private("127.0.0.1".parse().unwrap()));
        assert!(!private("17.253.144.10".parse().unwrap()));
    }

    #[test]
    fn fetch_from_local_server_is_capped() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut req = [0u8; 512];
            let n = s.read(&mut req).unwrap();
            assert!(req[..n].starts_with(b"GET /x HTTP/1.1\r\n"));
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\n\r\n");
            let _ = s.write_all(&vec![b'a'; 200_000]);
        });
        let raw = fetch(("127.0.0.1", port), "portal.example", "/x").unwrap();
        t.join().unwrap();
        assert!(raw.len() <= MAX_BYTES && raw.len() > 1000);
        assert_eq!(classify(&raw, APPLE).1, Urteil::Abweichung);
    }
}

#[cfg(test)]
mod kaputt_tests {
    #[test]
    fn parse_haelt_kaputte_antworten_aus() {
        let echt = b"HTTP/1.1 302 Found\r\nLocation: http://x.example/\r\n\r\nbody";
        assert!(super::parse(echt).is_some());
        for p in crate::kaputte_pakete(echt) {
            let _ = super::parse(&p);
        }
    }
}
