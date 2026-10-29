//! `netzinfo geraete`: devices in an own network, new and vanished (issue #8).

use crate::nachbarn::{self, Nachbar};
use crate::netze::{self, Fingerprint};
use crate::ports::{MAX_THREADS, State, classify};
use crate::subnetz::{self, Subnet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const PORTS: &[u16] = &[22, 23, 53, 80, 443, 445, 548, 631, 7000, 8009, 9100, 62078];
const TIMEOUT: Duration = Duration::from_millis(300);
/// Prefixes shorter than this (more than 1022 hosts) need `--gross`.
const MAX_PLAIN_HOSTS_PREFIX: u8 = 22;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub ip: Ipv4Addr,
    /// Reverse DNS name, if the resolver knows one.
    pub name: Option<String>,
    /// Open ports among the probed ones. Empty: the device refused every connection.
    pub ports: Vec<u16>,
    /// Unix seconds.
    pub first_seen: u64,
    pub last_seen: u64,
    /// Hardware address from the neighbour table (`--nachbarn`); the stable identity.
    #[serde(default)]
    pub mac: Option<String>,
    /// Earlier IPv4 addresses of the same MAC, oldest first.
    #[serde(default)]
    pub ip_verlauf: Vec<Ipv4Addr>,
    #[serde(default)]
    pub ipv6: Vec<Ipv6Addr>,
    /// Registered maker of the MAC (issue #27); `None` for random or unknown prefixes.
    #[serde(default)]
    pub hersteller: Option<String>,
    /// MAC is locally administered, i.e. made up by the device (private Wi-Fi address). It stays
    /// the same per network, so the same one at the next visit is the same device.
    #[serde(default)]
    pub mac_zufaellig: bool,
    /// Bonjour services of this device (issue #28), as last heard.
    #[serde(default)]
    pub dienste: Vec<crate::bonjour::Dienst>,
    /// Host name the device announces via mDNS, without `.local`.
    #[serde(default)]
    pub mdns_name: Option<String>,
    /// Correction by the user (issue #29). No scan overwrites it.
    #[serde(default)]
    pub von_mia: Option<crate::geraetekarte::VonMia>,
    /// Number of scans that found this device. 0 in files from before issue #29.
    #[serde(default)]
    pub besuche: u32,
    /// What the device says via UPnP/SSDP (issue #42). Scans keep it, `netzinfo upnp` sets it.
    #[serde(default)]
    pub upnp: Option<crate::upnp::Upnp>,
    /// Names from further sources (issue #43: NetBIOS, later UPnP), one per source. The names
    /// in `von_mia`, `mdns_name` and `name` are not copied here; `namen::kandidaten` joins all.
    #[serde(default)]
    pub namen: Vec<crate::namen::Name>,
}

impl Default for Device {
    /// Only for struct literals with `..Default::default()`: the IP is 0.0.0.0, set it.
    fn default() -> Self {
        Device {
            ip: Ipv4Addr::UNSPECIFIED,
            name: None,
            ports: vec![],
            first_seen: 0,
            last_seen: 0,
            mac: None,
            ip_verlauf: vec![],
            ipv6: vec![],
            hersteller: None,
            mac_zufaellig: false,
            dienste: vec![],
            mdns_name: None,
            von_mia: None,
            besuche: 0,
            upnp: None,
            namen: vec![],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct GeraeteErgebnis {
    /// Scanned range, e.g. `192.168.178.0/24`.
    pub bereich: String,
    /// Number of addresses probed.
    pub adressen: usize,
    /// Devices found now.
    pub geraete: Vec<Device>,
    /// Found now, unknown before. Always empty on the first run.
    pub neu: Vec<Device>,
    /// Known, inside the range, not found now.
    pub verschwunden: Vec<Device>,
    pub erster_lauf: bool,
}

/// Per network (key = `Fingerprint::key()`) the devices seen so far.
type Store = BTreeMap<String, Vec<Device>>;

fn refusal() -> String {
    "`netzinfo geraete` läuft nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es gibt dafür bewusst keine Option: Wer in Schule, Firma oder fremdem WLAN Geräte durchprobiert, bekommt schnell Ärger. Es wurde nichts gesendet.".into()
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
pub(crate) fn par<R: Send>(n: usize, f: impl Fn(usize) -> R + Sync) -> Vec<R> {
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
pub(crate) fn merge(
    known: &mut Vec<Device>,
    found: Vec<Device>,
    lo: Ipv4Addr,
    hi: Ipv4Addr,
) -> (Vec<Device>, Vec<Device>) {
    let mut new = Vec::new();
    let found_ips: Vec<Ipv4Addr> = found.iter().map(|d| d.ip).collect();
    for d in found {
        // same MAC = same device, even with a new IP; without MACs on both sides the IP decides
        let same = |k: &Device| match (&k.mac, &d.mac) {
            (Some(a), Some(b)) => a == b,
            _ => k.ip == d.ip,
        };
        match known.iter_mut().find(|k| same(k)) {
            Some(k) => {
                if k.ip != d.ip {
                    if !k.ip_verlauf.contains(&k.ip) {
                        k.ip_verlauf.push(k.ip);
                    }
                    k.ip = d.ip;
                }
                if d.mac.is_some() {
                    k.hersteller = d.hersteller;
                    k.mac_zufaellig = d.mac_zufaellig;
                }
                k.mac = d.mac.or(k.mac.take());
                if !d.ipv6.is_empty() {
                    k.ipv6 = d.ipv6;
                }
                k.name = d.name.or(k.name.take());
                for n in d.namen {
                    crate::namen::eintragen(&mut k.namen, n);
                }
                k.ports = d.ports;
                k.last_seen = d.last_seen;
                k.besuche = k.besuche.max(1) + 1;
            }
            None => {
                let d = Device { besuche: 1, ..d };
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

/// Local Network privacy of macOS silently drops our packets without permission and the table
/// stays empty. Heuristic: the router lies in the range but is missing from the table.
fn check_permission(table: &[Nachbar], router: Ipv4Addr, target: &Subnet) -> Result<(), String> {
    let in_range = (target.first..=target.last).contains(&router);
    if in_range && !table.iter().any(|n| n.ipv4.contains(&router)) {
        return Err(nachbarn::LEERE_TABELLE.into());
    }
    Ok(())
}

/// Puts MAC and IPv6 addresses on the devices found by TCP; neighbours in the range without
/// an open port become devices too (no open ports, but a MAC proves they exist).
fn add_neighbours(found: &mut Vec<Device>, table: &[Nachbar], target: &Subnet, t: u64) {
    for n in table {
        for ip in n
            .ipv4
            .iter()
            .filter(|a| (target.first..=target.last).contains(*a))
        {
            let i = match found.iter().position(|d| d.ip == *ip) {
                Some(i) => i,
                None => {
                    found.push(Device {
                        ip: *ip,
                        first_seen: t,
                        last_seen: t,
                        ..Default::default()
                    });
                    found.len() - 1
                }
            };
            found[i].mac = Some(n.mac.clone());
            if let Some(h) = crate::hersteller::fuer_text(&n.mac) {
                found[i].hersteller = h.name;
                found[i].mac_zufaellig = h.zufaellig;
            }
            found[i].ipv6 = n.ipv6.clone();
        }
    }
    found.sort_by_key(|d| d.ip);
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
        "  {:<15}  {:<5} {:<17}  {:<24}  {}  {}",
        d.ip.to_string(),
        tag,
        d.mac.as_deref().unwrap_or("-"),
        crate::hersteller::anzeige(d.mac.as_deref()),
        crate::namen::bester(d)
            .map(|n| format!("{} ({})", n.name, n.quelle.bezeichnung()))
            .unwrap_or_else(|| "-".into()),
        ports
    );
}

/// Searches devices in the current network. Refuses unless the network is marked „meins“, the
/// range lies inside the current subnet (or is 127.0.0.1) and, above /22, `gross` is set.
/// Nothing is sent before the checks pass.
pub fn suchen(cidr: Option<&str>, gross: bool) -> Result<GeraeteErgebnis, String> {
    suchen_mit(cidr, gross, false)
}

/// Like `suchen`; with `nachbarn` the neighbour table adds MAC addresses and devices that have
/// no open port (same „meins“ gate, the ping sweep runs after it).
pub fn suchen_mit(
    cidr: Option<&str>,
    gross: bool,
    nachbarn: bool,
) -> Result<GeraeteErgebnis, String> {
    let cidr = cidr
        .map(|c| subnetz::parse_args(&[c.to_string()]))
        .transpose()?;
    let meins = netze::current_is_meins();
    let info = crate::hier::collect()?;
    let fp = netze::fingerprint(&info)?;
    let current = info.subnet.ok_or("Das aktuelle Subnetz ist unbekannt.")?;
    let path = netze::data_path()?.with_file_name("geraete.json");
    let link = nachbarn.then_some(info.interface.as_str());
    suchen_in(&path, &fp, meins, &current, cidr, gross, link)
}

fn suchen_in(
    path: &Path,
    fp: &Fingerprint,
    meins: bool,
    current: &Subnet,
    cidr: Option<(Ipv4Addr, u8)>,
    gross: bool,
    link: Option<&str>,
) -> Result<GeraeteErgebnis, String> {
    let target = gate(meins, current, cidr, gross)?;
    let hosts: Vec<Ipv4Addr> = (u32::from(target.first)..=u32::from(target.last))
        .map(Ipv4Addr::from)
        .collect();
    // With the neighbour table the ping comes first: only devices in it (and we ourselves) can
    // answer on a TCP port, so the slow port probe skips the empty addresses.
    let table = match link {
        Some(link) => {
            nachbarn::anstossen(&hosts, link)?;
            let table = nachbarn::lesen(link)?;
            check_permission(&table, fp.router, &target)?;
            Some(table)
        }
        None => None,
    };
    let seen: Vec<Ipv4Addr> = table
        .iter()
        .flatten()
        .flat_map(|n| n.ipv4.iter().copied())
        .filter(|a| hosts.contains(a))
        .collect();
    let probe: Vec<Ipv4Addr> = if seen.is_empty() {
        hosts.clone()
    } else {
        hosts
            .iter()
            .copied()
            .filter(|h| seen.contains(h) || *h == current.ip)
            .collect()
    };
    let present = discover(&probe, PORTS);
    let resolver = crate::dns::system_resolver().ok();
    let t = netze::now();
    let mut found: Vec<Device> = present
        .into_iter()
        .map(|(ip, ports)| Device {
            ip,
            ports,
            first_seen: t,
            last_seen: t,
            ..Default::default()
        })
        .collect();
    if let Some(table) = &table {
        add_neighbours(&mut found, table, &target, t);
    }
    let names = par(found.len(), |i| {
        let ip = found[i].ip;
        // loopback is never worth a packet to the resolver
        resolver
            .filter(|_| !ip.is_loopback())
            .and_then(|r| reverse_name(r, ip))
    });
    for (d, name) in found.iter_mut().zip(names) {
        d.name = name;
    }
    // NetBIOS only for addresses from the neighbour table (`table` exists only after the gate)
    if let Some(table) = &table {
        let ips: Vec<Ipv4Addr> = found
            .iter()
            .map(|d| d.ip)
            .filter(|ip| table.iter().any(|n| n.ipv4.contains(ip)))
            .collect();
        for (ip, name) in ips.iter().zip(crate::namen::netbios_fragen(meins, &ips)) {
            if let (Some(name), Some(d)) = (name, found.iter_mut().find(|d| d.ip == *ip)) {
                d.namen.push(crate::namen::Name {
                    quelle: crate::namen::Quelle::NetBios,
                    name,
                    zuletzt: t,
                });
            }
        }
    }

    let mut store: Store = netze::load_json(path)?;
    let list = store.entry(fp.key()).or_default();
    let erster_lauf = list.is_empty();
    let geraete = found.clone();
    let (mut neu, verschwunden) = merge(list, found, target.first, target.last);
    if erster_lauf {
        neu.clear();
    }
    netze::save(path, &store)?;
    Ok(GeraeteErgebnis {
        bereich: format!("{}/{}", target.network, target.prefix),
        adressen: hosts.len(),
        geraete,
        neu,
        verschwunden,
        erster_lauf,
    })
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let e = |t: &str| crate::explain(explain_on, t);
    let mut cidr = None;
    let mut gross = false;
    let mut nachbarn = false;
    if args.iter().any(|a| a == "--karten") {
        if args.len() > 1 {
            return Err("`--karten` liest nur den Speicher und braucht keine weitere Option. Zum Suchen lass `--karten` weg.".into());
        }
        return crate::geraetekarte::zeigen(explain_on);
    }
    for a in args {
        match a.as_str() {
            "--gross" => gross = true,
            "--nachbarn" => nachbarn = true,
            s if s.starts_with("--") => {
                return Err(format!(
                    "Unbekannte Option „{s}“. Es gibt nur `--gross`, `--nachbarn`, `--karten` und `--erklaer`."
                ));
            }
            _ if cidr.is_none() => cidr = Some(a.as_str()),
            _ => {
                return Err(
                    "Es passt nur ein Bereich, z. B. `netzinfo geraete 192.168.178.0/24`.".into(),
                );
            }
        }
    }

    e(
        "Zuerst prüfe ich, ob dieses Netz als „meins“ markiert ist. Fremde Netze (Schule, Firma, Hotspot) durchzuprobieren gibt Ärger, deshalb gibt es hier keine Abkürzung wie bei `netzinfo ports`. Bis zu dieser Prüfung wird kein Paket gesendet.",
    );
    let r = suchen_mit(cidr, gross, nachbarn)?;
    e("Erlaubt: Das Netz ist „meins“, und der Bereich liegt ganz im eigenen Subnetz.");
    println!(
        "Suche Geräte in {} (Adressen: {}, Ports je Adresse: {}, Timeout {} ms)",
        r.bereich,
        r.adressen,
        PORTS.len(),
        TIMEOUT.as_millis()
    );
    e(
        "Bei jeder Adresse versuche ich einen TCP-Verbindungsaufbau auf ein paar gängige Ports (SSH, DNS, Web, Dateifreigabe, Drucker, AirPlay, iPhone-Sync). Dafür braucht es keine Root-Rechte.",
    );
    e(
        "Ein Gerät gilt als da, wenn ein Port offen ist ODER die Verbindung aktiv abgelehnt wird. Eine Ablehnung ist ein RST-Paket, und das kann nur ein laufendes Gerät schicken. Wo gar nichts antwortet, ist entweder niemand da oder eine Firewall schweigt: Solche Geräte sehe ich nicht.",
    );
    if nachbarn {
        e(
            "Mit `--nachbarn` schicke ich zusätzlich an jede Adresse einen Ping ohne Root-Rechte. Die Antwort ist egal: Um das Paket zuzustellen, fragt dein Mac per ARP „Wer hat diese IP?“ und merkt sich die Antwort in seiner Nachbartabelle. Diese Tabelle lese ich danach nur noch aus, so kommt zu jedem Gerät die MAC-Adresse, auch zu Geräten ohne offenen Port.",
        );
        e(
            "Die MAC-Adresse ist die Hardware-Nummer der Netzkarte. Sie bleibt gleich, auch wenn der Router dem Gerät morgen eine andere IP gibt. Darum führe ich Geräte über die MAC zusammen und merke mir die alten IPs.",
        );
        e(
            "Hat ein Gerät mehrere IPv6-Adressen, wechselt meist eine davon regelmäßig, damit man das Gerät im Internet nicht verfolgen kann.",
        );
        e(
            "Der Hersteller steht in der MAC-Adresse. Die ersten drei Byte vergibt die Normungsorganisation IEEE an Firmen; ich schlage sie in einer Liste nach, die in Netzinfo steckt (kein Abruf im Netz). Gemeint ist der Hersteller der Netzkarte, nicht unbedingt des Geräts: Viele Steckdosen und Lampen haben einen Chip derselben Firma.",
        );
        e(
            "Dieses Gerät benutzt eine zufällige Adresse, damit man es nicht über Netze hinweg verfolgen kann. Den Hersteller kann man deshalb nicht erkennen. Pro Netz bleibt die zufällige Adresse gleich, darum gilt sie beim nächsten Besuch als bekannt.",
        );
    }
    e(
        "Jedes Gerät zeigt den besten Namen mit seiner Quelle. Die Reihenfolge ist fest: dein eigener Name, der Bonjour-Name des Geräts, der UPnP-Name, der NetBIOS-Name, der Name aus dem Router (PTR), zuletzt Hersteller und Ende der MAC-Adresse. Dieser Name kommt vom Gerät selbst (Bonjour). Der Name aus dem Router kann abweichen. Namen sind selbst gewählt und nicht geprüft: Ein Gerät kann sich „Router“ nennen.",
    );
    if nachbarn {
        e(
            "NetBIOS: Ich frage jede Adresse aus der Nachbartabelle per UDP auf Port 137 „Wie heißt du?“ (Knotenstatus). Nur Windows- und Samba-Geräte antworten, wer schweigt, hat keinen NetBIOS-Namen. Das geschieht nur in „meins“-Netzen, mit einer Sekunde Wartezeit.",
        );
    }
    e(
        "Zu jedem gefundenen Gerät frage ich den DNS-Server deines Systems nach dem Namen. Das ist ein PTR-Eintrag (Pointer): die Rückwärtsauflösung von IP zu Name, abgefragt unter z. B. 20.178.168.192.in-addr.arpa. Viele Router tragen dort die Namen ihrer DHCP-Kunden ein, andere liefern nichts, dann bleibt der Name leer.",
    );

    println!("Gefundene Geräte: {}", r.geraete.len());
    for d in &r.geraete {
        let is_new = r.neu.iter().any(|n| n.ip == d.ip);
        show(d, if is_new { "neu" } else { "" });
    }
    if r.erster_lauf {
        println!("Erster Lauf in diesem Netz: alles ist der Ausgangsstand, nichts ist „neu“.");
    } else {
        println!("{} neu seit dem letzten Mal.", r.neu.len());
    }
    if !r.verschwunden.is_empty() {
        println!("Verschwunden (früher da, jetzt nicht gefunden):");
        for d in &r.verschwunden {
            show(d, "weg");
            println!(
                "                   zuletzt gesehen: {}",
                netze::format_utc(d.last_seen)
            );
        }
    }
    e(
        "Die Liste liegt pro Netz in geraete.json neben deiner Netzliste. Verschwunden heißt nur: heute nicht gefunden. Ein Handy im Standby oder ein ausgeschalteter Rechner fällt genauso heraus.",
    );
    Ok(())
}

/// Devices of network `key` with a random MAC that first showed up after `since` (Unix seconds);
/// `pruefen` turns them into a finding. Empty if `geraete` never ran there.
pub fn neue_zufaellige(path: &Path, key: &str, since: u64) -> Result<Vec<Device>, String> {
    let mut store: Store = netze::load_json(path)?;
    let mut list = store.remove(key).unwrap_or_default();
    list.retain(|d| d.mac_zufaellig && d.first_seen > since);
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn refusal_is_nicht_meins() {
        assert_eq!(netze::fehlerart(&refusal()), netze::Fehlerart::NichtMeins);
    }

    fn net() -> Subnet {
        subnetz::calc(ip("192.168.178.20"), 24).unwrap()
    }

    fn dev(a: &str, seen: u64) -> Device {
        Device {
            ip: ip(a),
            first_seen: seen,
            last_seen: seen,
            ..Default::default()
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

    fn fp() -> Fingerprint {
        Fingerprint {
            router: ip("192.168.178.1"),
            dhcp_server: ip("192.168.178.1"),
            domain: None,
            subnet: "192.168.178.0/24".into(),
            bssid_hash: None,
        }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("netz-geraete-{}-{name}", std::process::id()))
    }

    #[test]
    fn suchen_gate_sits_inside_and_sends_nothing_when_refused() {
        let fp = fp();
        let path = tmp("gate").join("geraete.json");
        let go =
            |meins, cidr: &str, gross| suchen_in(&path, &fp, meins, &net(), c(cidr), gross, None);
        assert!(
            go(false, "127.0.0.1/32", true)
                .unwrap_err()
                .contains("„meins“")
        );
        assert!(go(true, "10.0.0.1/32", true).is_err());
        assert!(go(true, "192.168.0.0/16", true).is_err());
        assert!(!path.exists(), "a refusal must not write anything");
    }

    #[test]
    fn suchen_on_loopback_first_run_then_second() {
        let dir = tmp("loop");
        let path = dir.join("geraete.json");
        let fp = fp();
        let r1 = suchen_in(&path, &fp, true, &net(), c("127.0.0.1/32"), false, None).unwrap();
        assert_eq!((r1.bereich.as_str(), r1.adressen), ("127.0.0.1/32", 1));
        assert!(r1.erster_lauf && r1.neu.is_empty());
        // loopback answers with at least a refusal, so it counts as a device
        assert_eq!(r1.geraete.len(), 1);
        let r2 = suchen_in(&path, &fp, true, &net(), c("127.0.0.1/32"), false, None).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!r2.erster_lauf && r2.neu.is_empty() && r2.verschwunden.is_empty());
        assert!(serde_json::to_string(&r2).unwrap().contains("verschwunden"));
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

    fn nb(mac: &str, v4: &[&str], v6: &[&str]) -> Nachbar {
        Nachbar {
            mac: mac.into(),
            ipv4: v4.iter().map(|a| ip(a)).collect(),
            ipv6: v6.iter().map(|a| a.parse().unwrap()).collect(),
            schnittstelle: "en0".into(),
        }
    }

    #[test]
    fn neighbours_join_tcp_results_and_merge_by_mac() {
        let t = subnetz::calc(ip("192.168.178.20"), 24).unwrap();
        let mut tcp = dev("192.168.178.2", 9);
        tcp.ports = vec![80];
        let mut found = vec![tcp];
        let table = [
            nb("02:00:00:00:00:01", &["192.168.178.2"], &["2001:db8::2"]),
            nb("02:00:00:00:00:02", &["192.168.178.7"], &[]), // no open port
            nb("02:00:00:00:00:03", &["10.0.0.5"], &[]),      // outside the range
        ];
        add_neighbours(&mut found, &table, &t, 9);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].mac.as_deref(), Some("02:00:00:00:00:01"));
        assert_eq!(found[0].ports, vec![80]);
        assert_eq!(found[0].ipv6.len(), 1);
        assert!(found[1].ports.is_empty() && found[1].ip == ip("192.168.178.7"));

        // day 2: the router gave the first device a new IP; same MAC means same device
        let mut known = found.clone();
        for k in &mut known {
            k.first_seen = 1;
        }
        let mut again = found[0].clone();
        again.ip = ip("192.168.178.30");
        let (lo, hi) = (ip("192.168.178.1"), ip("192.168.178.254"));
        let (new, gone) = merge(&mut known, vec![again], lo, hi);
        assert!(new.is_empty());
        assert_eq!(known.len(), 2);
        let k = known.iter().find(|d| d.mac == found[0].mac).unwrap();
        assert_eq!((k.ip, k.first_seen), (ip("192.168.178.30"), 1));
        assert_eq!(k.ip_verlauf, vec![ip("192.168.178.2")]);
        assert_eq!(gone.len(), 1); // the other one was not found this time
        // the same IP with another MAC is another device
        let mut other = found[0].clone();
        other.mac = Some("02:00:00:00:00:09".into());
        other.ip = ip("192.168.178.7");
        let (new, _) = merge(&mut known, vec![other], lo, hi);
        assert_eq!(new.len(), 1);
    }

    #[test]
    fn empty_table_with_router_in_range_means_missing_permission() {
        let t = subnetz::calc(ip("192.168.178.20"), 24).unwrap();
        let router = ip("192.168.178.1");
        let err = check_permission(&[], router, &t).unwrap_err();
        assert!(err.contains("Lokales Netzwerk"));
        let ok = [nb("02:00:00:00:00:01", &["192.168.178.1"], &[])];
        assert!(check_permission(&ok, router, &t).is_ok());
        // router outside the scanned range: nothing to conclude
        let small = subnetz::calc(ip("192.168.178.20"), 32).unwrap();
        assert!(check_permission(&[], router, &small).is_ok());
    }

    #[test]
    fn old_store_without_mac_still_loads() {
        let old = r#"{"k":[{"ip":"192.168.178.2","name":null,"ports":[80],"first_seen":1,"last_seen":2}]}"#;
        let s: Store = serde_json::from_str(old).unwrap();
        assert_eq!(s["k"][0].mac, None);
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

    #[test]
    fn old_store_without_hersteller_still_loads() {
        let old = r#"{"k":[{"ip":"192.168.178.2","name":null,"ports":[],"first_seen":1,"last_seen":2,"mac":"02:00:5e:00:53:01"}]}"#;
        let s: Store = serde_json::from_str(old).unwrap();
        assert_eq!(
            (s["k"][0].hersteller.clone(), s["k"][0].mac_zufaellig),
            (None, false)
        );
    }

    #[test]
    fn same_random_mac_is_known_and_new_one_is_listed() {
        let dir = std::env::temp_dir().join(format!("netz-zufall-{}", std::process::id()));
        let path = dir.join("geraete.json");
        let mut a = dev("192.168.178.2", 5);
        a.mac = Some("02:00:5e:00:53:01".into());
        a.mac_zufaellig = true;
        let mut b = dev("192.168.178.3", 50);
        b.mac = Some("02:00:5e:00:53:02".into());
        b.mac_zufaellig = true;
        let mut c = dev("192.168.178.4", 60);
        c.mac = Some("00:00:5e:00:53:03".into());
        let mut store = Store::new();
        store.insert("k".into(), vec![a.clone(), b, c]);
        netze::save(&path, &store).unwrap();
        let n = neue_zufaellige(&path, "k", 10).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].ip, ip("192.168.178.3"));
        // same random MAC seen again, other IP: merged, first_seen stays
        let mut known = vec![a];
        let mut again = dev("192.168.178.9", 100);
        again.mac = Some("02:00:5e:00:53:01".into());
        let (new, _) = merge(
            &mut known,
            vec![again],
            ip("192.168.178.0"),
            ip("192.168.178.255"),
        );
        assert!(new.is_empty() && known[0].first_seen == 5);
    }
}
