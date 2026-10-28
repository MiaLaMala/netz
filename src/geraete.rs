//! `netz geraete`: devices in an own network, new and vanished (issue #8).

use crate::netze::{self, Fingerprint};
use crate::ports::{MAX_THREADS, State, classify};
use crate::subnetz::{self, Subnet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const PORTS: &[u16] = &[22, 53, 80, 443, 445, 548, 631, 7000, 8009, 9100, 62078];
const TIMEOUT: Duration = Duration::from_millis(300);
/// Prefixes shorter than this (more than 1022 hosts) need `--gross`.
const MAX_PLAIN_HOSTS_PREFIX: u8 = 22;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Device {
    ip: Ipv4Addr,
    name: Option<String>,
    ports: Vec<u16>,
    first_seen: u64,
    last_seen: u64,
}

/// Per network (key = `Fingerprint::key()`) the devices seen so far.
type Store = BTreeMap<String, Vec<Device>>;

fn refusal() -> String {
    "`netz geraete` läuft nur in einem Netz, das du mit `netz merken <Name> --meins` als „meins“ markiert hast. Es gibt dafür bewusst keine Option: Wer in Schule, Firma oder fremdem WLAN Geräte durchprobiert, bekommt schnell Ärger. Es wurde nichts gesendet.".into()
}

/// The one place that decides whether a discovery may run; returns the range to scan.
/// Nothing is sent before this returns Ok.
fn gate(
    meins: bool,
    current: &Subnet,
    cidr: Option<(Ipv4Addr, u8)>,
    gross: bool,
) -> Result<Subnet, String> {
    if !meins {
        return Err(refusal());
    }
    let target = match cidr {
        Some((ip, prefix)) => subnetz::calc(ip, prefix)?,
        None => subnetz::calc(current.network, current.prefix)?,
    };
    let inside = target.network >= current.network && target.broadcast <= current.broadcast;
    let loopback = target.network.is_loopback() && target.broadcast.is_loopback();
    if !inside && !loopback {
        return Err(format!(
            "{}/{} liegt nicht ganz in deinem Netz {}/{}. Ich probiere nur Adressen im eigenen Subnetz durch (oder 127.0.0.1).",
            target.network, target.prefix, current.network, current.prefix
        ));
    }
    if target.prefix < MAX_PLAIN_HOSTS_PREFIX && !gross {
        return Err(format!(
            "/{} sind {} Adressen, das dauert lange und belastet das Netz. Größer als /{MAX_PLAIN_HOSTS_PREFIX} nur mit `--gross`, oder wähle ein kleineres Stück, z. B. {}/24.",
            target.prefix, target.hosts, target.network
        ));
    }
    Ok(target)
}

/// Runs `f(0..n)` on up to 64 threads; results in index order.
fn par<R: Send>(n: usize, f: impl Fn(usize) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(n));
    std::thread::scope(|s| {
        for _ in 0..MAX_THREADS.min(n) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = f(i);
                    if let Ok(mut v) = out.lock() {
                        v.push((i, r));
                    }
                }
            });
        }
    });
    let mut v = out.into_inner().unwrap_or_default();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, r)| r).collect()
}

/// Hosts with at least one open port or an active refusal (RST); with their open ports.
// ponytail: a host that drops every SYN (stealth firewall, phone asleep) is not found. Upgrade
// path: ARP/ping need root or raw sockets.
fn discover(hosts: &[Ipv4Addr], ports: &[u16]) -> Vec<(Ipv4Addr, Vec<u16>)> {
    let states = par(hosts.len() * ports.len(), |i| {
        let addr = SocketAddr::new(IpAddr::V4(hosts[i / ports.len()]), ports[i % ports.len()]);
        classify(&TcpStream::connect_timeout(&addr, TIMEOUT))
    });
    hosts
        .iter()
        .enumerate()
        .filter_map(|(h, ip)| {
            let own = &states[h * ports.len()..(h + 1) * ports.len()];
            let open = ports
                .iter()
                .zip(own)
                .filter(|(_, s)| **s == State::Open)
                .map(|(p, _)| *p)
                .collect::<Vec<_>>();
            (own.iter().any(|s| *s != State::Filtered)).then_some((*ip, open))
        })
        .collect()
}

fn ptr_query_name(ip: Ipv4Addr) -> String {
    let o = ip.octets();
    format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
}

/// Reverse DNS at the system resolver; None for any problem or no answer.
fn reverse_name(resolver: IpAddr, ip: Ipv4Addr) -> Option<String> {
    let answer = crate::dns::query(
        resolver,
        &ptr_query_name(ip),
        crate::dns::PTR,
        crate::dns::IN,
    )
    .ok()?;
    let r = answer.records.iter().find(|r| r.rtype == crate::dns::PTR)?;
    Some(r.data.trim_end_matches('.').to_string()).filter(|n| !n.is_empty())
}

/// Updates `known` with what was `found`. Returns (new, vanished); vanished are known devices
/// inside `lo..=hi` (the scanned range) that were not found. They stay in the list.
fn merge(
    known: &mut Vec<Device>,
    found: Vec<Device>,
    lo: Ipv4Addr,
    hi: Ipv4Addr,
) -> (Vec<Device>, Vec<Device>) {
    let mut new = Vec::new();
    let found_ips: Vec<Ipv4Addr> = found.iter().map(|d| d.ip).collect();
    for d in found {
        match known.iter_mut().find(|k| k.ip == d.ip) {
            Some(k) => {
                k.name = d.name.or(k.name.take());
                k.ports = d.ports;
                k.last_seen = d.last_seen;
            }
            None => {
                new.push(d.clone());
                known.push(d);
            }
        }
    }
    known.sort_by_key(|d| d.ip);
    let gone = known
        .iter()
        .filter(|d| (lo..=hi).contains(&d.ip) && !found_ips.contains(&d.ip))
        .cloned()
        .collect();
    (new, gone)
}

fn show(d: &Device, tag: &str) {
    let ports = if d.ports.is_empty() {
        "keine offenen Ports (Verbindung abgelehnt)".to_string()
    } else {
        format!(
            "offen: {}",
            d.ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    println!(
        "  {:<15}  {:<5} {}  {}",
        d.ip.to_string(),
        tag,
        d.name.as_deref().unwrap_or("-"),
        ports
    );
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let e = |t: &str| crate::explain(explain_on, t);
    let mut cidr = None;
    let mut gross = false;
    for a in args {
        match a.as_str() {
            "--gross" => gross = true,
            s if s.starts_with("--") => {
                return Err(format!(
                    "Unbekannte Option „{s}“. Es gibt nur `--gross` und `--erklaer`."
                ));
            }
            _ if cidr.is_none() => cidr = Some(subnetz::parse_args(std::slice::from_ref(a))?),
            _ => {
                return Err(
                    "Es passt nur ein Bereich, z. B. `netz geraete 192.168.178.0/24`.".into(),
                );
            }
        }
    }

    e(
        "Zuerst prüfe ich, ob dieses Netz als „meins“ markiert ist. Fremde Netze (Schule, Firma, Hotspot) durchzuprobieren gibt Ärger, deshalb gibt es hier keine Abkürzung wie bei `netz ports`. Bis zu dieser Prüfung wird kein Paket gesendet.",
    );
    let meins = netze::current_is_meins();
    let info = crate::hier::collect()?;
    let fp: Fingerprint = netze::fingerprint(&info)?;
    let current = info.subnet.ok_or("Das aktuelle Subnetz ist unbekannt.")?;
    let target = gate(meins, &current, cidr, gross)?;
    e("Erlaubt: Das Netz ist „meins“, und der Bereich liegt ganz im eigenen Subnetz.");

    let hosts: Vec<Ipv4Addr> = (u32::from(target.first)..=u32::from(target.last))
        .map(Ipv4Addr::from)
        .collect();
    println!(
        "Suche Geräte in {}/{} (Adressen: {}, Ports je Adresse: {}, Timeout {} ms)",
        target.network,
        target.prefix,
        hosts.len(),
        PORTS.len(),
        TIMEOUT.as_millis()
    );
    e(
        "Ohne Root-Rechte kann ich weder pingen noch ARP lesen. Also versuche ich bei jeder Adresse einen TCP-Verbindungsaufbau auf ein paar gängige Ports (SSH, DNS, Web, Dateifreigabe, Drucker, AirPlay, iPhone-Sync).",
    );
    e(
        "Ein Gerät gilt als da, wenn ein Port offen ist ODER die Verbindung aktiv abgelehnt wird. Eine Ablehnung ist ein RST-Paket, und das kann nur ein laufendes Gerät schicken. Wo gar nichts antwortet, ist entweder niemand da oder eine Firewall schweigt: Solche Geräte sehe ich nicht.",
    );
    let present = discover(&hosts, PORTS);

    let resolver = crate::dns::system_resolver().ok();
    e(
        "Zu jedem gefundenen Gerät frage ich den DNS-Server deines Systems nach dem Namen. Das ist ein PTR-Eintrag (Pointer): die Rückwärtsauflösung von IP zu Name, abgefragt unter z. B. 20.178.168.192.in-addr.arpa. Viele Router tragen dort die Namen ihrer DHCP-Kunden ein, andere liefern nichts, dann bleibt der Name leer.",
    );
    let t = netze::now();
    let names = par(present.len(), |i| {
        let ip = present[i].0;
        // loopback is never worth a packet to the resolver
        resolver
            .filter(|_| !ip.is_loopback())
            .and_then(|r| reverse_name(r, ip))
    });
    let found: Vec<Device> = present
        .into_iter()
        .zip(names)
        .map(|((ip, ports), name)| Device {
            ip,
            name,
            ports,
            first_seen: t,
            last_seen: t,
        })
        .collect();

    let path = netze::data_path()?.with_file_name("geraete.json");
    let mut store: Store = netze::load_json(&path)?;
    let list = store.entry(fp.key()).or_default();
    let had_any = !list.is_empty();
    let all = found.clone();
    let (new, gone) = merge(list, found, target.first, target.last);

    println!("Gefundene Geräte: {}", all.len());
    for d in &all {
        let is_new = had_any && new.iter().any(|n| n.ip == d.ip);
        show(d, if is_new { "neu" } else { "" });
    }
    if had_any {
        println!("{} neu seit dem letzten Mal.", new.len());
    } else {
        println!("Erster Lauf in diesem Netz: alles ist der Ausgangsstand, nichts ist „neu“.");
    }
    if !gone.is_empty() {
        println!("Verschwunden (früher da, jetzt nicht gefunden):");
        for d in &gone {
            show(d, "weg");
            println!(
                "                   zuletzt gesehen: {}",
                netze::format_utc(d.last_seen)
            );
        }
    }
    netze::save(&path, &store)?;
    e(
        "Die Liste liegt pro Netz in geraete.json neben deiner Netzliste. Verschwunden heißt nur: heute nicht gefunden. Ein Handy im Standby oder ein ausgeschalteter Rechner fällt genauso heraus.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    fn net() -> Subnet {
        subnetz::calc(ip("192.168.178.20"), 24).unwrap()
    }

    fn dev(a: &str, seen: u64) -> Device {
        Device {
            ip: ip(a),
            name: None,
            ports: vec![],
            first_seen: seen,
            last_seen: seen,
        }
    }

    fn c(s: &str) -> Option<(Ipv4Addr, u8)> {
        Some(subnetz::parse_args(&[s.to_string()]).unwrap())
    }

    #[test]
    fn gate_refuses_unmarked_network() {
        assert!(gate(false, &net(), None, false).is_err());
        assert!(gate(false, &net(), c("127.0.0.1/32"), true).is_err());
    }

    #[test]
    fn gate_cidr_must_lie_inside_subnet() {
        let n = net();
        assert_eq!(gate(true, &n, None, false).unwrap().prefix, 24);
        assert!(gate(true, &n, c("192.168.178.64/26"), false).is_ok());
        assert!(gate(true, &n, c("192.168.178.20/32"), false).is_ok());
        assert!(gate(true, &n, c("192.168.179.0/24"), false).is_err());
        assert!(gate(true, &n, c("192.168.0.0/16"), true).is_err());
        assert!(gate(true, &n, c("10.0.0.1/32"), true).is_err());
        assert!(gate(true, &n, c("192.168.178.250/23"), false).is_err());
    }

    #[test]
    fn gate_needs_gross_above_slash_22() {
        let big = subnetz::calc(ip("10.1.0.5"), 16).unwrap();
        assert!(gate(true, &big, None, false).is_err());
        assert!(gate(true, &big, None, true).is_ok());
        assert!(gate(true, &big, c("10.1.4.0/22"), false).is_ok());
        assert!(gate(true, &big, c("10.1.0.0/21"), false).is_err());
    }

    #[test]
    fn gate_allows_loopback() {
        let t = gate(true, &net(), c("127.0.0.1/32"), false).unwrap();
        assert_eq!(t.first, ip("127.0.0.1"));
        assert!(gate(true, &net(), c("127.0.0.0/8"), false).is_err());
    }

    #[test]
    fn ptr_name_is_reversed() {
        assert_eq!(
            ptr_query_name(ip("192.168.178.20")),
            "20.178.168.192.in-addr.arpa"
        );
    }

    #[test]
    fn merge_finds_new_and_vanished() {
        let mut known = vec![dev("192.168.178.2", 1), dev("192.168.178.3", 1)];
        let mut again = dev("192.168.178.2", 9);
        again.ports = vec![80];
        let found = vec![again, dev("192.168.178.4", 9)];
        let (lo, hi) = (ip("192.168.178.1"), ip("192.168.178.254"));
        let (new, gone) = merge(&mut known, found.clone(), lo, hi);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].ip, ip("192.168.178.4"));
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].ip, ip("192.168.178.3"));
        assert_eq!(known.len(), 3);
        assert_eq!(known[0].first_seen, 1);
        assert_eq!(known[0].last_seen, 9);
        assert_eq!(known[0].ports, vec![80]);
        // a small scan does not declare the rest of the network vanished
        let (_, gone) = merge(&mut known, found, ip("192.168.178.2"), ip("192.168.178.2"));
        assert!(gone.is_empty());
    }

    #[test]
    fn store_round_trip() {
        let dir = std::env::temp_dir().join(format!("netz-geraete-{}", std::process::id()));
        let path = dir.join("geraete.json");
        let mut store = Store::new();
        let mut d = dev("192.168.178.2", 5);
        d.name = Some("fritz.box".into());
        d.ports = vec![53, 80];
        store.insert("k".into(), vec![d]);
        netze::save(&path, &store).unwrap();
        let back: Store = netze::load_json(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(back, store);
    }

    #[test]
    fn discover_finds_open_and_refused_on_loopback() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let open = l.local_addr().unwrap().port();
        let closed = {
            let t = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            t.local_addr().unwrap().port()
        };
        let lo = ip("127.0.0.1");
        let r = discover(&[lo], &[open, closed]);
        assert_eq!(r, vec![(lo, vec![open])]);
        // refused only: still present, no open ports
        assert_eq!(discover(&[lo], &[closed]), vec![(lo, vec![])]);
    }
}
