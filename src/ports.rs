//! `netzinfo ports`: TCP connect scan of an own device (issue #4).

use serde::Serialize;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub(crate) const MAX_THREADS: usize = 64;
const DEFAULT_TIMEOUT_MS: u64 = 500;

const COMMON: &[(u16, &str)] = &[
    (21, "FTP"),
    (22, "SSH"),
    (23, "Telnet"),
    (25, "SMTP"),
    (53, "DNS"),
    (80, "HTTP"),
    (110, "POP3"),
    (111, "RPC"),
    (123, "NTP"),
    (139, "NetBIOS"),
    (143, "IMAP"),
    (443, "HTTPS"),
    (445, "SMB"),
    (548, "AFP (Dateifreigabe am Mac)"),
    (631, "IPP (Drucker)"),
    (993, "IMAPS"),
    (995, "POP3S"),
    (1883, "MQTT"),
    (3306, "MySQL"),
    (3389, "RDP"),
    (5000, "AirPlay / UPnP"),
    (5432, "PostgreSQL"),
    (5900, "VNC (Bildschirmfreigabe)"),
    (6379, "Redis"),
    (7000, "AirPlay"),
    (8006, "Proxmox"),
    (8080, "HTTP (alternativ)"),
    (8443, "HTTPS (alternativ)"),
    (9000, "Webdienst (z. B. Portainer)"),
    (32400, "Plex"),
];

#[derive(Debug, PartialEq)]
pub(crate) enum State {
    Open,
    Closed,
    Filtered,
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    if args.iter().any(|a| a == "--raus") {
        return crate::egress::run(args, explain_on);
    }
    let mut target = None;
    let mut meins = false;
    let mut ports = None;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--meins" => meins = true,
            "--ports" => {
                let v = it.next().ok_or("Nach `--ports` fehlt die Portliste, z. B. `--ports 1-1024` oder `--ports 22,80,443`.")?;
                ports = Some(parse_ports(v)?);
            }
            "--timeout" => {
                let v = it
                    .next()
                    .ok_or("Nach `--timeout` fehlen die Millisekunden, z. B. `--timeout 300`.")?;
                timeout_ms = match v.parse::<u64>() {
                    Ok(ms) if ms > 0 => ms,
                    _ => {
                        return Err(format!(
                            "„{v}“ ist kein gültiger Timeout. Gib Millisekunden an, z. B. `--timeout 300`."
                        ));
                    }
                };
            }
            s if s.starts_with("--") => {
                return Err(format!(
                    "Unbekannte Option „{s}“. `netzinfo hilfe` zeigt alle Optionen."
                ));
            }
            _ if target.is_none() => target = Some(a.as_str()),
            _ => {
                return Err(
                    "Es passt nur ein Ziel, z. B. `netzinfo ports 192.168.178.20 --meins`.".into(),
                );
            }
        }
    }
    let target =
        target.ok_or("Es fehlt das Ziel, z. B. `netzinfo ports 192.168.178.20 --meins`.")?;

    crate::explain(
        explain_on,
        "Ich löse das Ziel in IP-Adressen auf (bei einer IP-Adresse passiert dabei nichts im Netz).",
    );
    let r = scannen(target, meins, ports, timeout_ms)?;
    crate::explain(
        explain_on,
        &if let Some(grund) = grund(meins) {
            format!("Alle Adressen des Ziels sind privat, und {grund}. Der Scan darf starten.")
        } else {
            "Das Ziel ist die eigene Maschine (Loopback). Der Scan darf starten.".to_string()
        },
    );
    explain_handshake(explain_on);
    println!(
        "Scanne {} ({} Ports, Timeout {timeout_ms} ms)",
        r.ip,
        r.ports.len()
    );
    for p in r.ports.iter().filter(|p| p.zustand == Zustand::Offen) {
        println!("  {:>5}  offen        {}", p.port, p.dienst);
    }
    println!(
        "{} offen, {} geschlossen, {} gefiltert",
        r.offen, r.geschlossen, r.gefiltert
    );
    if r.gefiltert > 0 && r.gefiltert == r.ports.len() {
        println!("Alles gefiltert: Eine Firewall verwirft die Pakete, oder das Gerät ist aus.");
    }
    Ok(())
}

/// Why a scan of a non-loopback target is allowed; None: only loopback is.
fn grund(flag: bool) -> Option<&'static str> {
    if flag {
        Some("du hast mit `--meins` bestätigt, dass es dir gehört")
    } else if crate::netze::current_is_meins() {
        Some("dieses Netz ist mit `netzinfo merken` als „meins“ markiert")
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Zustand {
    Offen,
    Geschlossen,
    Gefiltert,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortStatus {
    pub port: u16,
    /// Well-known service, empty if not in the list.
    pub dienst: String,
    pub zustand: Zustand,
}

#[derive(Debug, Serialize)]
pub struct PortErgebnis {
    pub ip: IpAddr,
    /// In scan order (as given, else the common list).
    pub ports: Vec<PortStatus>,
    pub offen: usize,
    pub geschlossen: usize,
    pub gefiltert: usize,
}

/// Scans `ziel` (IP or name). `meins`: the caller confirms the device is theirs; a network
/// marked „meins“ counts too. Without that only loopback is scanned; with it only private
/// addresses. `ports`: `None` = common ports. Nothing is sent, and no name resolved, before
/// the checks pass.
pub fn scannen(
    ziel: &str,
    meins: bool,
    ports: Option<Vec<u16>>,
    timeout_ms: u64,
) -> Result<PortErgebnis, String> {
    let meins = meins || crate::netze::current_is_meins();
    scannen_mit(ziel, meins, ports, timeout_ms)
}

fn scannen_mit(
    ziel: &str,
    meins: bool,
    ports: Option<Vec<u16>>,
    timeout_ms: u64,
) -> Result<PortErgebnis, String> {
    // Refuse before resolving, so a hostname causes no DNS traffic without --meins.
    // Loopback literals are always fine.
    if !meins && !ziel.parse::<IpAddr>().is_ok_and(|a| a.is_loopback()) {
        return Err(refusal_text(ziel));
    }
    let addrs: Vec<IpAddr> = (ziel, 0)
        .to_socket_addrs()
        .map_err(|_| {
            format!(
                "„{ziel}“ lässt sich nicht auflösen. Prüfe die Schreibweise und deine Verbindung."
            )
        })?
        .map(|s| s.ip())
        .collect();
    check_target(&addrs, meins)?;
    let ip = addrs[0];
    let list: Vec<(u16, String)> = match ports {
        Some(p) => p.into_iter().map(|p| (p, service_name(p))).collect(),
        None => COMMON.iter().map(|&(p, n)| (p, n.to_string())).collect(),
    };
    let nums: Vec<u16> = list.iter().map(|(p, _)| *p).collect();
    let results = scan(ip, &nums, Duration::from_millis(timeout_ms.max(1)));
    let count = |s: State| results.iter().filter(|r| **r == s).count();
    let (offen, geschlossen, gefiltert) = (
        count(State::Open),
        count(State::Closed),
        count(State::Filtered),
    );
    Ok(PortErgebnis {
        ip,
        ports: list
            .into_iter()
            .zip(results)
            .map(|((port, dienst), s)| PortStatus {
                port,
                dienst,
                zustand: match s {
                    State::Open => Zustand::Offen,
                    State::Closed => Zustand::Geschlossen,
                    State::Filtered => Zustand::Gefiltert,
                },
            })
            .collect(),
        offen,
        geschlossen,
        gefiltert,
    })
}

fn explain_handshake(on: bool) {
    let e = |t: &str| crate::explain(on, t);
    e(
        "TCP baut jede Verbindung mit dem Drei-Wege-Handschlag auf: SYN (Client fragt an), SYN-ACK (Server stimmt zu), ACK (Client bestätigt).",
    );
    e(
        "Ein Connect-Scan führt diesen Handschlag für jeden Port wirklich aus. Das geht ohne Root-Rechte.",
    );
    e("Offen: Es kommt ein SYN-ACK zurück, ein Dienst hört auf dem Port.");
    e(
        "Geschlossen: Es kommt ein RST, der Rechner lehnt die Verbindung aktiv ab (connection refused). Das Gerät ist da, aber kein Dienst hört.",
    );
    e(
        "Gefiltert: Es kommt keine Antwort, bis der Timeout abläuft. Meist verwirft eine Firewall das SYN lautlos.",
    );
}

/// The one place that decides whether a scan may run. `meins` is true when the user passed
/// `--meins` or the current network is stored as „meins“. Loopback is always allowed.
fn check_target(addrs: &[IpAddr], meins: bool) -> Result<(), String> {
    if addrs.is_empty() {
        return Err("Das Ziel hat keine Adresse. Prüfe die Schreibweise.".into());
    }
    if addrs.iter().all(|a| a.is_loopback()) {
        return Ok(());
    }
    if !meins {
        return Err(refusal_text("das Ziel"));
    }
    if let Some(bad) = addrs.iter().find(|a| !is_private(**a)) {
        return Err(format!(
            "{bad} ist keine private Adresse. `netzinfo ports` scannt nur eigene Geräte in privaten Netzen (10.x, 172.16 bis 172.31, 192.168.x, 100.64 bis 100.127, Loopback, IPv6 fc00::/7 und fe80::/10). Ein Portscan auf fremde Rechner gibt Ärger."
        ));
    }
    Ok(())
}

fn refusal_text(target: &str) -> String {
    format!(
        "Ohne `--meins` scanne ich {target} nicht. In Netzen, die dir nicht gehören (Schule, Arbeit, Hotspot), gibt ein Portscan ohne Erlaubnis Ärger. Wenn Gerät und Netz dir gehören: `netzinfo ports {target} --meins`, oder einmalig `netzinfo merken <Name> --meins` im richtigen Netz."
    )
}

fn is_private(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || v4.is_loopback() || (o[0] == 100 && (o[1] & 0xc0) == 0x40)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments()[0];
            v6.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
        }
    }
}

pub(crate) fn classify(r: &std::io::Result<TcpStream>) -> State {
    match r {
        Ok(_) => State::Open,
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => State::Closed,
        Err(_) => State::Filtered,
    }
}

fn scan(ip: IpAddr, ports: &[u16], timeout: Duration) -> Vec<State> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, State)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..MAX_THREADS.min(ports.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&port) = ports.get(i) else { break };
                    let state = classify(&TcpStream::connect_timeout(
                        &SocketAddr::new(ip, port),
                        timeout,
                    ));
                    if let Ok(mut v) = out.lock() {
                        v.push((i, state));
                    }
                }
            });
        }
    });
    let mut v = out.into_inner().unwrap_or_default();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, s)| s).collect()
}

pub(crate) fn service_name(port: u16) -> String {
    COMMON
        .iter()
        .find(|(p, _)| *p == port)
        .map_or("", |(_, n)| n)
        .to_string()
}

/// Parses "22,80,443", "1-1024" or a mix; sorted, no duplicates, no port 0.
fn parse_ports(s: &str) -> Result<Vec<u16>, String> {
    let bad = |p: &str| {
        format!(
            "„{p}“ ist keine gültige Portangabe. Erlaubt sind Zahlen von 1 bis 65535, Bereiche wie 1-1024 und Listen wie 22,80,443."
        )
    };
    let num = |p: &str| {
        p.trim()
            .parse::<u16>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| bad(p))
    };
    let mut v = Vec::new();
    for part in s.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (num(a)?, num(b)?);
                if a > b {
                    return Err(bad(part));
                }
                v.extend(a..=b);
            }
            None => v.push(num(part)?),
        }
    }
    v.sort_unstable();
    v.dedup();
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn scannen_gate_and_loopback_result() {
        // refused before anything is resolved or sent
        assert!(scannen_mit("192.168.178.20", false, None, 100).is_err());
        assert!(scannen_mit("localhost", false, None, 100).is_err());
        assert!(scannen_mit("8.8.8.8", true, None, 100).is_err());
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let open = l.local_addr().unwrap().port();
        let r = scannen_mit("127.0.0.1", false, Some(vec![open]), 500).unwrap();
        assert_eq!((r.offen, r.geschlossen, r.gefiltert), (1, 0, 0));
        assert_eq!(r.ports[0].zustand, Zustand::Offen);
        assert!(serde_json::to_string(&r).unwrap().contains("Offen"));
    }

    #[test]
    fn parses_lists_and_ranges() {
        assert_eq!(parse_ports("22,80,443").unwrap(), vec![22, 80, 443]);
        assert_eq!(parse_ports("1-3").unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_ports("80,22,22,20-22").unwrap(), vec![20, 21, 22, 80]);
    }

    #[test]
    fn rejects_bad_ports() {
        for s in ["", "0", "65536", "abc", "5-1", "1-", "22,,80"] {
            assert!(parse_ports(s).is_err(), "{s}");
        }
    }

    #[test]
    fn private_and_public_targets() {
        for s in [
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.1",
            "192.168.178.20",
            "100.64.0.1",
            "100.127.1.1",
            "127.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.178.20",
        ] {
            assert!(is_private(ip(s)), "{s}");
        }
        for s in [
            "1.1.1.1",
            "192.0.2.1",
            "203.0.113.7",
            "172.32.0.1",
            "100.128.0.1",
            "100.63.0.1",
            "2001:db8::1",
            "::ffff:203.0.113.7",
        ] {
            assert!(!is_private(ip(s)), "{s}");
        }
    }

    #[test]
    fn gate_needs_meins_and_every_address_private() {
        let own = [ip("192.168.178.20")];
        assert!(check_target(&own, true).is_ok());
        assert!(check_target(&own, false).is_err());
        assert!(check_target(&[ip("203.0.113.7")], true).is_err());
        // hostname resolving to one private and one public address is refused
        assert!(check_target(&[ip("10.0.0.5"), ip("203.0.113.7")], true).is_err());
        assert!(check_target(&[], true).is_err());
        // loopback needs no marking
        assert!(check_target(&[ip("127.0.0.1")], false).is_ok());
        assert!(check_target(&[ip("::1")], false).is_ok());
        assert!(check_target(&[ip("127.0.0.1"), ip("192.168.178.20")], false).is_err());
    }

    #[test]
    fn classifies_connect_results() {
        let refused = Err(std::io::Error::from(ErrorKind::ConnectionRefused));
        let timeout = Err(std::io::Error::from(ErrorKind::TimedOut));
        assert_eq!(classify(&refused), State::Closed);
        assert_eq!(classify(&timeout), State::Filtered);
    }

    #[test]
    fn scan_finds_open_and_closed_on_loopback() {
        let open = TcpListener::bind("127.0.0.1:0").unwrap();
        let open_port = open.local_addr().unwrap().port();
        let closed_port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let r = scan(
            ip("127.0.0.1"),
            &[open_port, closed_port],
            Duration::from_millis(500),
        );
        assert_eq!(r, vec![State::Open, State::Closed]);
    }

    #[test]
    fn service_names_known_and_unknown() {
        assert_eq!(service_name(8006), "Proxmox");
        assert_eq!(service_name(4), "");
    }
}
