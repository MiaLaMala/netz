//! `netz offen`: which programs of this Mac accept connections, and from where (issue #38).

use serde::Serialize;
use std::net::IpAddr;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Reichweite {
    /// Only this Mac (127.0.0.1, ::1).
    Lokal,
    /// Wildcard or a network address: others on the network can connect.
    Netz,
    Unbekannt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lauscher {
    /// "TCP" or "UDP".
    pub protokoll: String,
    /// "*" for all addresses.
    pub adresse: String,
    pub port: u16,
    pub pid: u32,
    pub programm: String,
    pub nutzer: String,
    /// Well-known service of the port, empty if unknown.
    pub dienst: String,
    pub reichweite: Reichweite,
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let json = match args.first().map(String::as_str) {
        None => false,
        Some("--json") if args.len() == 1 => true,
        Some(a) => {
            return Err(format!(
                "Unbekannte Option „{a}“. `netz offen` kennt nur `--json`."
            ));
        }
    };
    if json {
        let s = serde_json::to_string_pretty(&offen()?).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    let e = |t: &str| crate::explain(explain_on, t);
    e(
        "Ich frage mit `lsof` ab, welche Programme dieses Rechners auf einem Port auf Verbindungen warten. Das ist nur Lesen, nichts geht ins Netz und es braucht kein Root.",
    );
    e(
        "Ohne Root sehe ich nur deine eigenen Programme. Systemdienste und Programme anderer Nutzer fehlen in der Liste, auch wenn sie lauschen.",
    );
    let liste = offen()?;
    e(
        "„NETZ“ heißt: Das Programm lauscht auf allen Adressen (`*`) oder auf einer Netzadresse. Jeder im selben WLAN kann den Port ansprechen.",
    );
    e(
        "„lokal“ heißt: Das Programm lauscht auf 127.0.0.1 oder ::1. Nur dieser Mac selbst kommt hin, von außen niemand.",
    );
    e("UDP kennt keine Verbindung, ein gebundener Port nimmt einfach Pakete an.");
    if liste.is_empty() {
        println!("Keine lauschenden Programme von dir gefunden.");
    }
    for l in &liste {
        let wo = match l.reichweite {
            Reichweite::Netz => "NETZ ",
            Reichweite::Lokal => "lokal",
            Reichweite::Unbekannt => "?    ",
        };
        let dienst = if l.dienst.is_empty() {
            String::new()
        } else {
            format!("  {}", l.dienst)
        };
        println!(
            "  {wo}  {} {:>5}  {:<15}  {} (PID {}){dienst}",
            l.protokoll, l.port, l.adresse, l.programm, l.pid
        );
    }
    let netz = liste
        .iter()
        .filter(|l| l.reichweite == Reichweite::Netz)
        .count();
    println!(
        "{} lauschend, davon {netz} für das Netz erreichbar. Programme anderer Nutzer sind ohne Root nicht sichtbar.",
        liste.len()
    );
    Ok(())
}

/// Listening TCP and bound UDP sockets of the current user, network-reachable first.
pub fn offen() -> Result<Vec<Lauscher>, String> {
    let out = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-iUDP", "-F", "pcLfPtn"])
        .output()
        .map_err(|_| {
            "`lsof` ließ sich nicht starten. Es liegt unter macOS in /usr/sbin/lsof, prüfe, ob es fehlt."
                .to_string()
        })?;
    // lsof exits 1 when it finds nothing, so only the text counts.
    let mut v = parse_lsof(&String::from_utf8_lossy(&out.stdout));
    v.sort_by_key(|l| (l.reichweite != Reichweite::Netz, l.port, l.pid));
    Ok(v)
}

/// Parses `lsof -F pcLfPtn` output (one field per line, first char = field id).
/// Unknown or missing fields and unusable records are skipped.
pub(crate) fn parse_lsof(text: &str) -> Vec<Lauscher> {
    let (mut pid, mut prog, mut user) = (None, String::new(), String::new());
    let (mut proto, mut name) = (String::new(), String::new());
    let mut out: Vec<Lauscher> = Vec::new();
    let flush = |out: &mut Vec<Lauscher>,
                 pid: Option<u32>,
                 prog: &str,
                 user: &str,
                 proto: &str,
                 name: &str| {
        let (Some(pid), Some((host, port))) = (pid, split_addr(name)) else {
            return;
        };
        let l = Lauscher {
            protokoll: proto.to_string(),
            reichweite: reichweite(&host),
            adresse: host,
            port,
            pid,
            programm: prog.to_string(),
            nutzer: user.to_string(),
            dienst: crate::ports::service_name(port),
        };
        if (proto == "TCP" || proto == "UDP") && !out.contains(&l) {
            out.push(l);
        }
    };
    for line in text.lines() {
        let mut c = line.chars();
        let Some(id) = c.next() else { continue };
        let val = c.as_str();
        match id {
            'p' => {
                flush(&mut out, pid, &prog, &user, &proto, &name);
                (pid, prog, user) = (val.parse().ok(), String::new(), String::new());
                (proto, name) = (String::new(), String::new());
            }
            'f' => {
                flush(&mut out, pid, &prog, &user, &proto, &name);
                (proto, name) = (String::new(), String::new());
            }
            'c' => prog = val.to_string(),
            'L' => user = val.to_string(),
            'P' => proto = val.to_string(),
            'n' => name = val.to_string(),
            _ => {}
        }
    }
    flush(&mut out, pid, &prog, &user, &proto, &name);
    out
}

/// "*:80", "127.0.0.1:631", "[::1]:8080" to (host, port); None for "*:*" and connected "a->b".
fn split_addr(s: &str) -> Option<(String, u16)> {
    if s.contains("->") {
        return None;
    }
    let (host, port) = s.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host.to_string(), port.parse().ok()?))
}

fn reichweite(host: &str) -> Reichweite {
    if host == "*" {
        return Reichweite::Netz;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) if ip.is_loopback() => Reichweite::Lokal,
        Ok(_) => Reichweite::Netz,
        Err(_) => Reichweite::Unbekannt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "p100\ncserver\nLuser\nf5\nPTCP\ntIPv4\nn*:8080\nf6\nPTCP\ntIPv6\nn[::1]:8080\nf7\nPUDP\ntIPv4\nn*:5353\nf8\nPUDP\ntIPv4\nn192.0.2.5:123->192.0.2.9:123\np200\ncExampleApp\nLuser\nf3\nPTCP\ntIPv4\nn127.0.0.1:631\n";

    #[test]
    fn parses_normal_ipv6_and_udp() {
        let v = parse_lsof(SAMPLE);
        // the connected UDP socket is dropped
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].reichweite, Reichweite::Netz);
        assert_eq!(
            (v[0].port, v[0].pid, v[0].programm.as_str()),
            (8080, 100, "server")
        );
        assert_eq!(v[0].dienst, "HTTP (alternativ)");
        assert_eq!(
            (v[1].adresse.as_str(), v[1].reichweite),
            ("::1", Reichweite::Lokal)
        );
        assert_eq!((v[2].protokoll.as_str(), v[2].port), ("UDP", 5353));
        assert_eq!((v[3].pid, v[3].programm.as_str()), (200, "ExampleApp"));
        assert_eq!(v[3].reichweite, Reichweite::Lokal);
    }

    #[test]
    fn tolerates_truncated_and_garbage() {
        assert!(parse_lsof("").is_empty());
        assert!(parse_lsof("p1\ncx\nf3\nPTCP\nn127.0.0.1:").is_empty());
        assert!(parse_lsof("p\nn*:80\n").is_empty());
        assert!(parse_lsof("\u{0}\u{ff}garbage\n\n::\nn\nnnn:99999\nPTCP\nn*:*").is_empty());
        // last record cut off after the name still counts
        assert_eq!(parse_lsof("p1\ncx\nf3\nPTCP\nn*:80").len(), 1);
        // no protocol: dropped
        assert!(parse_lsof("p1\ncx\nf3\nn*:80").is_empty());
    }

    #[test]
    fn scope() {
        assert_eq!(reichweite("*"), Reichweite::Netz);
        assert_eq!(reichweite("127.0.0.1"), Reichweite::Lokal);
        assert_eq!(reichweite("::1"), Reichweite::Lokal);
        assert_eq!(reichweite("192.0.2.5"), Reichweite::Netz);
        assert_eq!(reichweite("host"), Reichweite::Unbekannt);
    }

    #[test]
    #[cfg(target_os = "macos")] // lsof lives in /usr/sbin only there
    fn own_test_port_shows_up_as_local() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let v = offen().unwrap();
        let me = v
            .iter()
            .find(|x| x.port == port && x.pid == std::process::id())
            .expect("lsof should list our own listening test port");
        assert_eq!(
            (me.protokoll.as_str(), me.reichweite),
            ("TCP", Reichweite::Lokal)
        );
    }
}
