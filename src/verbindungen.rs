//! `netzinfo verbindungen`: which program talks to which target right now (issue #39).

use crate::dns;
use crate::geraete::Device;
pub mod herkunft_ip;
use crate::netze;
use crate::offen::{Socket, sockets};
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// At most this many targets are remembered per program and network.
const MAX_ZIELE: usize = 200;
/// Time limit for all reverse lookups of one run (they run in parallel).
const PTR_FRIST: Duration = Duration::from_millis(500);
/// More unknown targets than this are shown with their IP only.
const MAX_ANFRAGEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Art {
    /// This Mac itself (127.0.0.1, ::1).
    Lokal,
    /// Private, link-local or shared address space: a device in a network.
    Netz,
    Internet,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verbindung {
    pub protokoll: String,
    pub lokal: String,
    pub ziel: String,
    pub ziel_port: u16,
    /// Well-known service of the target port, empty if unknown.
    pub dienst: String,
    pub zustand: String,
    pub pid: u32,
    pub programm: String,
    /// Executable path as `ps` reports it.
    pub pfad: Option<String>,
    pub art: Art,
    /// Device name or reverse DNS name of the target.
    pub name: Option<String>,
    /// "Gerät" or "PTR": where `name` comes from.
    pub name_quelle: Option<String>,
    /// Program and target were not seen in this network before.
    pub neu: bool,
    /// Country name and provider (AS name) from the offline database, only for public targets.
    pub land: Option<String>,
    pub asn: Option<u32>,
    pub anbieter: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Gruppe {
    pub programm: String,
    pub neu: usize,
    pub verbindungen: Vec<Verbindung>,
}

#[derive(Debug, Serialize)]
pub struct Ergebnis {
    pub gruppen: Vec<Gruppe>,
    /// Name of the stored network, if this one is stored.
    pub netz_name: Option<String>,
    /// No earlier data of this network: nothing can be „neu“.
    pub erster_lauf: bool,
    pub gespeichert: bool,
    /// Build month of the offline database (`JJJJ-MM`); None: no database, country and provider stay empty.
    pub datenbank_stand: Option<String>,
}

/// Per network (key = `Fingerprint::key()`) program name to remembered targets.
type Speicher = BTreeMap<String, BTreeMap<String, Vec<String>>>;

pub fn art(ip: IpAddr) -> Art {
    match ip.to_canonical() {
        IpAddr::V4(a) if a.is_loopback() => Art::Lokal,
        IpAddr::V4(a)
            if a.is_private()
                || a.is_link_local()
                || (a.octets()[0] == 100 && a.octets()[1] & 0xc0 == 64) =>
        {
            Art::Netz
        }
        IpAddr::V6(a) if a.is_loopback() => Art::Lokal,
        IpAddr::V6(a)
            if a.segments()[0] & 0xfe00 == 0xfc00 || a.segments()[0] & 0xffc0 == 0xfe80 =>
        {
            Art::Netz
        }
        _ => Art::Internet,
    }
}

/// Own TCP connections in state ESTABLISHED, without names yet.
pub fn verbunden() -> Result<Vec<Verbindung>, String> {
    Ok(aus_sockets(sockets()?, &pfade))
}

fn aus_sockets(
    socks: Vec<Socket>,
    pfade: &dyn Fn(&[u32]) -> BTreeMap<u32, String>,
) -> Vec<Verbindung> {
    let socks: Vec<Socket> = socks
        .into_iter()
        .filter(|s| s.protokoll == "TCP" && s.zustand == "ESTABLISHED" && s.ziel.is_some())
        .collect();
    let mut pids: Vec<u32> = socks.iter().map(|s| s.pid).collect();
    pids.sort_unstable();
    pids.dedup();
    let pfad = pfade(&pids);
    socks
        .into_iter()
        .filter_map(|s| {
            let (ziel, ziel_port) = s.ziel?;
            let ip: IpAddr = ziel.parse().ok()?;
            let pfad = pfad.get(&s.pid).cloned();
            // lsof cuts program names at 9 characters; the file name of the path is complete.
            let programm = pfad
                .as_deref()
                .and_then(|p| p.rsplit('/').next())
                .filter(|n| n.starts_with(&s.programm))
                .map_or(s.programm.clone(), str::to_string);
            Some(Verbindung {
                protokoll: s.protokoll,
                lokal: format!("{}:{}", s.lokal.0, s.lokal.1),
                art: art(ip),
                dienst: dienst(ziel_port),
                ziel,
                ziel_port,
                zustand: s.zustand,
                pid: s.pid,
                programm,
                pfad,
                name: None,
                name_quelle: None,
                neu: false,
                land: None,
                asn: None,
                anbieter: None,
            })
        })
        .collect()
}

/// Executable paths for `pids` from `ps`; empty on any problem.
fn pfade(pids: &[u32]) -> BTreeMap<u32, String> {
    if pids.is_empty() {
        return BTreeMap::new();
    }
    let liste: Vec<String> = pids.iter().map(u32::to_string).collect();
    let Ok(out) = std::process::Command::new("ps")
        .args(["-o", "pid=,comm=", "-p", &liste.join(",")])
        .output()
    else {
        return BTreeMap::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (pid, pfad) = l.trim().split_once(' ')?;
            Some((pid.parse().ok()?, pfad.trim().to_string()))
        })
        .collect()
}

/// Service name of a target port: a few extras on top of the port list of `netzinfo ports`.
pub fn dienst(port: u16) -> String {
    match port {
        853 => "DNS über TLS".into(),
        5223 => "Apple Push".into(),
        5228 => "Google Push".into(),
        465 | 587 => "SMTP (Mail senden)".into(),
        _ => crate::ports::service_name(port),
    }
}

fn ptr_name(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(a) => {
            let o = a.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(a) => {
            let nibbles: String = a
                .octets()
                .iter()
                .rev()
                .map(|b| format!("{:x}.{:x}.", b & 0xf, b >> 4))
                .collect();
            format!("{nibbles}ip6.arpa")
        }
    }
}

type Aufloeser = Arc<dyn Fn(IpAddr) -> Option<String> + Send + Sync>;
type Zwischenspeicher = BTreeMap<IpAddr, Option<String>>;
static NAMEN: Mutex<Zwischenspeicher> = Mutex::new(BTreeMap::new());

/// Reverse DNS name at the system resolver; None for any problem or no answer.
fn ptr_system(resolver: IpAddr, ip: IpAddr) -> Option<String> {
    let a = dns::query(resolver, &ptr_name(ip), dns::PTR, dns::IN).ok()?;
    let r = a.records.iter().find(|r| r.rtype == dns::PTR)?;
    Some(r.data.trim_end_matches('.').to_string()).filter(|n| !n.is_empty())
}

/// Looks up `ips` in parallel and stores the answers in `cache`. Lookups still running after
/// `frist` are given up on and not cached, so the next run asks again.
fn namen(ips: &[IpAddr], cache: &mut Zwischenspeicher, aufloeser: &Aufloeser, frist: Duration) {
    let (tx, rx) = mpsc::channel();
    let mut offen = 0;
    for &ip in ips
        .iter()
        .filter(|ip| !cache.contains_key(ip))
        .take(MAX_ANFRAGEN)
    {
        let (tx, f) = (tx.clone(), Arc::clone(aufloeser));
        std::thread::spawn(move || {
            let _ = tx.send((ip, f(ip)));
        });
        offen += 1;
    }
    let ende = Instant::now() + frist;
    while offen > 0 {
        let Some(rest) = ende.checked_duration_since(Instant::now()) else {
            break;
        };
        let Ok((ip, name)) = rx.recv_timeout(rest) else {
            break;
        };
        cache.insert(ip, name);
        offen -= 1;
    }
}

/// Key under which a target is remembered: its name, else its provider (`AS<nr>`), else the
/// network prefix (/24 or /48).
fn schluessel(v: &Verbindung) -> String {
    if let Some(n) = &v.name {
        return n.to_lowercase();
    }
    if let (Some(asn), Some(_)) = (v.asn, &v.anbieter) {
        return format!("AS{asn}");
    }
    match v.ziel.parse::<IpAddr>().map(|ip| ip.to_canonical()) {
        Ok(IpAddr::V4(a)) => {
            let o = a.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        Ok(IpAddr::V6(a)) => {
            let s = a.segments();
            format!("{:x}:{:x}:{:x}::/48", s[0], s[1], s[2])
        }
        Err(_) => v.ziel.clone(),
    }
}

/// Marks what is new against `speicher[key]` and adds it in memory (capped per program).
/// Returns `erster_lauf`: the network had no data before.
fn auswerten(speicher: &mut Speicher, key: &str, vs: &mut [Verbindung]) -> bool {
    let erster = !speicher.contains_key(key);
    let bekannt = speicher.entry(key.to_string()).or_default();
    for v in vs.iter_mut().filter(|v| v.art != Art::Lokal) {
        let ziel = schluessel(v);
        let liste = bekannt.entry(v.programm.clone()).or_default();
        let da = liste.contains(&ziel);
        v.neu = !erster && !da;
        if !da && liste.len() < MAX_ZIELE {
            liste.push(ziel);
        }
    }
    erster
}

pub fn gruppieren(mut vs: Vec<Verbindung>) -> Vec<Gruppe> {
    vs.sort_by(|a, b| {
        let k = |v: &Verbindung| {
            (
                v.name.clone().unwrap_or_else(|| v.ziel.clone()),
                v.ziel_port,
            )
        };
        (&a.programm, k(a)).cmp(&(&b.programm, k(b)))
    });
    let mut gruppen: Vec<Gruppe> = Vec::new();
    for v in vs {
        match gruppen.last_mut() {
            Some(g) if g.programm == v.programm => g.verbindungen.push(v),
            _ => gruppen.push(Gruppe {
                programm: v.programm.clone(),
                neu: 0,
                verbindungen: vec![v],
            }),
        }
    }
    for g in &mut gruppen {
        g.neu = g.verbindungen.iter().filter(|v| v.neu).count();
    }
    gruppen.sort_by(|a, b| {
        b.verbindungen
            .len()
            .cmp(&a.verbindungen.len())
            .then(a.programm.cmp(&b.programm))
    });
    gruppen
}

/// The current network if it is stored: (key, name, meins).
fn aktuelles_netz() -> Option<(String, String, bool)> {
    let fp = netze::fingerprint(&crate::hier::collect().ok()?).ok()?;
    let list = netze::load(&netze::data_path().ok()?).ok()?;
    let n = &list[netze::find(&list, &fp)?];
    Some((fp.key(), n.name.clone(), n.meins))
}

/// Active connections grouped by program, with target names and „neu“ marks.
/// The store is only written for „meins“ networks or with `speichern`.
pub fn sammeln(speichern: bool) -> Result<Ergebnis, String> {
    let mut vs = verbunden()?;
    let netz = aktuelles_netz();
    let pfad = netze::data_path()?.with_file_name("verbindungen.json");
    let geraete: BTreeMap<String, Vec<Device>> =
        netze::load_json(&netze::data_path()?.with_file_name("geraete.json")).unwrap_or_default();
    let geraete = netz.as_ref().and_then(|n| geraete.get(&n.0));
    let mut cache = NAMEN.lock().unwrap_or_else(|e| e.into_inner());
    // ponytail: the app runs for days; start over instead of an LRU once the cache is big.
    if cache.len() > 4096 {
        cache.clear();
    }
    let resolver = dns::system_resolver().ok();
    let aufloeser: Aufloeser = Arc::new(move |ip| ptr_system(resolver?, ip));
    let ips: Vec<IpAddr> = vs
        .iter()
        .filter(|v| v.art != Art::Lokal)
        .filter_map(|v| v.ziel.parse().ok())
        .collect();
    namen(&ips, &mut cache, &aufloeser, PTR_FRIST);
    for v in vs.iter_mut().filter(|v| v.art != Art::Lokal) {
        let Ok(ip) = v.ziel.parse::<IpAddr>() else {
            continue;
        };
        let geraet = geraete.and_then(|g| {
            g.iter()
                .find(|d| IpAddr::V4(d.ip) == ip.to_canonical())
                .and_then(|d| d.name.clone())
        });
        (v.name, v.name_quelle) = match (geraet, cache.get(&ip).cloned().flatten()) {
            (Some(n), _) => (Some(n), Some("Gerät".into())),
            (None, Some(n)) => (Some(n), Some("PTR".into())),
            _ => (None, None),
        };
    }
    drop(cache);
    let db = herkunft_ip::datenbank();
    for v in &mut vs {
        if let Some(z) = v.ziel.parse().ok().and_then(|ip| db.nachschlagen(ip)) {
            (v.land, v.asn, v.anbieter) = (z.land, z.asn, z.anbieter);
        }
    }
    let mut r = sammeln_in(&pfad, netz, speichern, vs)?;
    r.datenbank_stand = db.stand();
    Ok(r)
}

fn sammeln_in(
    pfad: &Path,
    netz: Option<(String, String, bool)>,
    speichern: bool,
    mut vs: Vec<Verbindung>,
) -> Result<Ergebnis, String> {
    let (mut erster_lauf, mut gespeichert, mut netz_name) = (true, false, None);
    if let Some((key, name, meins)) = netz {
        let mut speicher: Speicher = netze::load_json(pfad)?;
        erster_lauf = auswerten(&mut speicher, &key, &mut vs);
        if meins || speichern {
            netze::save(pfad, &speicher)?;
            gespeichert = true;
        }
        netz_name = Some(name);
    }
    Ok(Ergebnis {
        gruppen: gruppieren(vs),
        netz_name,
        erster_lauf,
        gespeichert,
        datenbank_stand: None,
    })
}

/// Deletes everything remembered about the current network. False: there was nothing.
pub fn loeschen() -> Result<bool, String> {
    let Some((key, ..)) = aktuelles_netz() else {
        return Err("Dieses Netz ist nicht gemerkt, also gibt es dazu auch nichts zu löschen. Mit `netzinfo merken <Name>` merkst du es dir.".into());
    };
    let pfad = netze::data_path()?.with_file_name("verbindungen.json");
    let mut speicher: Speicher = netze::load_json(&pfad)?;
    let da = speicher.remove(&key).is_some();
    if da {
        netze::save(&pfad, &speicher)?;
    }
    Ok(da)
}

pub const OPTIONEN: &[&str] = &["--json", "--merken", "--loeschen", "--programm"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "verbindungen")?;
    let (mut json, mut speichern, mut weg, mut prog) = (false, false, false, None::<String>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--merken" => speichern = true,
            "--loeschen" => weg = true,
            "--programm" => {
                prog = Some(it.next().cloned().ok_or("Nach `--programm` fehlt der Name. Beispiel: `netzinfo verbindungen --programm Safari`.")?);
            }
            _ => {
                return Err(format!(
                    "Unbekannte Option „{a}“. `netzinfo verbindungen` kennt `--programm NAME`, `--json`, `--merken` und `--loeschen`."
                ));
            }
        }
    }
    let e = |t: &str| crate::explain(explain_on, t);
    if weg {
        e(
            "Ich lösche nur, was Netzinfo über dieses Netz in verbindungen.json gemerkt hat. Dein Rechner und das Netz ändern sich nicht.",
        );
        println!(
            "{}",
            if loeschen()? {
                "Gemerkte Verbindungen dieses Netzes gelöscht."
            } else {
                "Zu diesem Netz war nichts gemerkt."
            }
        );
        return Ok(());
    }
    e(
        "Ich frage mit `lsof` ab, welche Programme dieses Rechners gerade eine Verbindung offen haben (Zustand ESTABLISHED: beide Seiten haben sich per Handschlag verbunden). Das ist nur Lesen.",
    );
    e(
        "Die Namen der Ziele frage ich per Rückwärts-DNS (PTR) beim System-DNS-Server, höchstens 500 ms lang. Das ist die einzige Anfrage ins Netz. Oft steht dort der Name des Hosters statt des Dienstes.",
    );
    let mut r = sammeln(speichern)?;
    if let Some(p) = &prog {
        let p = p.to_lowercase();
        r.gruppen.retain(|g| g.programm.to_lowercase().contains(&p));
    }
    if json {
        let s = serde_json::to_string_pretty(&r).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    if r.gruppen.is_empty() {
        println!("Keine aktiven Verbindungen von dir gefunden.");
    }
    for g in &r.gruppen {
        let n = g.verbindungen.len();
        println!(
            "{} ({n} Verbindung{})",
            g.programm,
            if n == 1 { "" } else { "en" }
        );
        let mut zaehler: BTreeMap<String, usize> = BTreeMap::new();
        for v in &g.verbindungen {
            let ziel = v.name.as_deref().unwrap_or(&v.ziel);
            *zaehler.entry(ziel.to_string()).or_default() += 1;
            let art = match v.art {
                Art::Netz => "  Netz",
                Art::Lokal => "  lokal",
                Art::Internet => "",
            };
            let dienst = if v.dienst.is_empty() {
                String::new()
            } else {
                format!("  {}", v.dienst)
            };
            let herkunft = match (&v.land, &v.anbieter) {
                (Some(l), Some(a)) => format!("  {l}, {a}"),
                (Some(x), None) | (None, Some(x)) => format!("  {x}"),
                _ => String::new(),
            };
            let neu = if v.neu { "  neu" } else { "" };
            let quelle = v
                .name_quelle
                .as_deref()
                .map_or(String::new(), |q| format!("  [{q}]"));
            let ziel = if ziel.contains(':') {
                format!("[{ziel}]")
            } else {
                ziel.to_string()
            };
            println!(
                "  {ziel}:{}{dienst}{art}{herkunft}{quelle}{neu}",
                v.ziel_port
            );
        }
        if let Some((ziel, k)) = zaehler.iter().max_by_key(|(_, k)| **k) {
            e(&format!(
                "Dieses Programm hat {n} offene Verbindungen, die meisten ({k}) zu {ziel}."
            ));
        }
    }
    let neu: usize = r.gruppen.iter().map(|g| g.neu).sum();
    e(
        "Land und Anbieter schlage ich nur in einer Datei nach, die mit Netzinfo kommt. Dafür geht keine Adresse ins Netz. Das Land kann bei Anbietern mit vielen Standorten falsch sein, und die Adresse allein sagt nicht, welcher Dienst dahinter steckt.",
    );
    e(
        "Ein Ziel mit „neu“ hat dein Rechner in diesem Netz noch nie angesprochen. Das ist nicht gefährlich, aber neu. Netzinfo sperrt nichts.",
    );
    match (&r.netz_name, r.erster_lauf, r.gespeichert) {
        (None, ..) => println!(
            "Dieses Netz ist nicht gemerkt, deshalb gibt es kein „neu“ und nichts wird gespeichert."
        ),
        (Some(n), true, g) => println!(
            "Erster Lauf im Netz „{n}“: Das ist der Ausgangsstand, nichts ist „neu“.{}",
            if g {
                ""
            } else {
                " Gespeichert wird nichts (`--merken` schaltet das ein)."
            }
        ),
        (Some(n), false, g) => println!(
            "{neu} neu im Netz „{n}“.{}",
            if g {
                ""
            } else {
                " Gespeichert wird nichts (`--merken` schaltet das ein)."
            }
        ),
    }
    println!(
        "Programme anderer Nutzer sind ohne Root nicht sichtbar. Die Namen stammen aus PTR und sind oft die des Hosters."
    );
    let mut laender: BTreeMap<&str, usize> = BTreeMap::new();
    for v in r.gruppen.iter().flat_map(|g| &g.verbindungen) {
        if let Some(l) = &v.land {
            *laender.entry(l).or_default() += 1;
        }
    }
    if !laender.is_empty() {
        let mut nach: Vec<_> = laender.into_iter().collect();
        nach.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let teile: Vec<String> = nach.iter().map(|(l, n)| format!("{l} {n}")).collect();
        println!("Nach Land: {}", teile.join(", "));
    }
    match &r.datenbank_stand {
        Some(s) => println!(
            "Land und Anbieter stammen aus der eingebauten Datenbank (Stand {s}, IP Geolocation by DB-IP, https://db-ip.com, und IPtoASN). Sie kann falsch liegen, vor allem bei Anbietern mit weltweiten Adressen."
        ),
        None => println!(
            "Die Datenbank für Land und Anbieter fehlt, deshalb bleiben diese Angaben leer (Ordner mit land.mmdb und asn.mmdb: Umgebungsvariable NETZ_IPDB, Bauen: tools/ipdb-bauen.sh)."
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(prog: &str, ziel: &str, port: u16) -> Verbindung {
        Verbindung {
            protokoll: "TCP".into(),
            lokal: "192.0.2.5:50000".into(),
            ziel: ziel.into(),
            ziel_port: port,
            dienst: dienst(port),
            zustand: "ESTABLISHED".into(),
            pid: 1,
            programm: prog.into(),
            pfad: None,
            art: art(ziel.parse().unwrap()),
            name: None,
            name_quelle: None,
            neu: false,
            land: None,
            asn: None,
            anbieter: None,
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn classifies_targets() {
        for a in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.1",
            "fd00::1",
            "fe80::1",
        ] {
            assert_eq!(art(ip(a)), Art::Netz, "{a}");
        }
        for a in [
            "203.0.113.9",
            "100.128.0.1",
            "100.63.0.1",
            "2001:db8::1",
            "::ffff:203.0.113.9",
        ] {
            assert_eq!(art(ip(a)), Art::Internet, "{a}");
        }
        assert_eq!(art(ip("::ffff:10.0.0.1")), Art::Netz);
        assert_eq!(
            (art(ip("127.0.0.1")), art(ip("::1"))),
            (Art::Lokal, Art::Lokal)
        );
    }

    #[test]
    fn groups_by_program_biggest_first() {
        let g = gruppieren(vec![
            v("b", "203.0.113.1", 443),
            v("a", "203.0.113.2", 443),
            v("b", "203.0.113.3", 80),
            v("b", "203.0.113.4", 80),
        ]);
        let t: Vec<(&str, usize)> = g
            .iter()
            .map(|g| (g.programm.as_str(), g.verbindungen.len()))
            .collect();
        assert_eq!(t, [("b", 3), ("a", 1)]);
    }

    #[test]
    fn new_against_example_store() {
        let mut s = Speicher::new();
        s.entry("k".into()).or_default().insert(
            "a".into(),
            vec!["203.0.113.0/24".into(), "mail.example".into()],
        );
        let mut named = v("a", "203.0.113.50", 993);
        named.name = Some("Mail.Example".into());
        let mut vs = vec![
            v("a", "203.0.113.7", 443),   // same /24: known
            named,                        // name matches ignoring case: known
            v("a", "198.51.100.1", 443),  // new prefix
            v("zzz", "203.0.113.7", 443), // new program
            v("a", "127.0.0.1", 8080),    // local: never new
        ];
        assert!(!auswerten(&mut s, "k", &mut vs));
        let neu: Vec<bool> = vs.iter().map(|v| v.neu).collect();
        assert_eq!(neu, [false, false, true, true, false]);
        // an unknown network has nothing to compare with
        let mut vs = vec![v("a", "203.0.113.7", 443)];
        assert!(auswerten(&mut s, "andere", &mut vs));
        assert!(!vs[0].neu);
    }

    #[test]
    fn provider_is_the_store_key_without_a_name() {
        let mut x = v("a", "203.0.113.7", 443);
        assert_eq!(schluessel(&x), "203.0.113.0/24");
        (x.asn, x.anbieter) = (Some(13335), Some("CLOUDFLARENET".into()));
        assert_eq!(schluessel(&x), "AS13335");
        x.name = Some("Host.Example".into());
        assert_eq!(schluessel(&x), "host.example");
    }

    #[test]
    fn store_is_capped_at_200_per_program() {
        let mut s = Speicher::new();
        let mut vs: Vec<Verbindung> = (0..250)
            .map(|i| {
                let mut x = v("a", "203.0.113.1", 443);
                x.name = Some(format!("h{i}.example"));
                x
            })
            .collect();
        auswerten(&mut s, "k", &mut vs);
        assert_eq!(s["k"]["a"].len(), MAX_ZIELE);
    }

    #[test]
    fn storing_is_off_unless_meins_or_switch() {
        let dir = std::env::temp_dir().join(format!("netz-verb-{}", std::process::id()));
        let pfad = dir.join("verbindungen.json");
        let netz = |meins| Some(("k".to_string(), "Testnetz".to_string(), meins));
        let vs = || vec![v("a", "203.0.113.7", 443)];
        let r = sammeln_in(&pfad, netz(false), false, vs()).unwrap();
        assert!(!r.gespeichert && !pfad.exists());
        let r = sammeln_in(&pfad, None, true, vs()).unwrap();
        assert!(
            !r.gespeichert && !pfad.exists(),
            "unknown network: never stored"
        );
        let r = sammeln_in(&pfad, netz(false), true, vs()).unwrap();
        assert!(r.gespeichert && r.erster_lauf && pfad.exists());
        let r = sammeln_in(&pfad, netz(true), false, vec![v("a", "198.51.100.1", 443)]).unwrap();
        assert!(r.gespeichert && !r.erster_lauf && r.gruppen[0].neu == 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reverse_lookup_gives_up_after_the_limit() {
        // a resolver that never answers
        let nie: Aufloeser = Arc::new(|_| {
            std::thread::sleep(Duration::from_secs(30));
            None
        });
        let mut cache = Zwischenspeicher::new();
        let t = Instant::now();
        namen(
            &[ip("203.0.113.1"), ip("203.0.113.2")],
            &mut cache,
            &nie,
            Duration::from_millis(100),
        );
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(cache.is_empty(), "timeouts are not cached");
        // answers come in and are cached; a second call does not ask again
        let ja: Aufloeser = Arc::new(|_| Some("host.example".into()));
        namen(
            &[ip("203.0.113.1")],
            &mut cache,
            &ja,
            Duration::from_secs(2),
        );
        assert_eq!(cache[&ip("203.0.113.1")].as_deref(), Some("host.example"));
        namen(
            &[ip("203.0.113.1")],
            &mut cache,
            &nie,
            Duration::from_millis(100),
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn ptr_names() {
        assert_eq!(ptr_name(ip("192.0.2.5")), "5.2.0.192.in-addr.arpa");
        assert!(ptr_name(ip("2001:db8::1")).starts_with("1.0.0.0."));
        assert!(ptr_name(ip("2001:db8::1")).ends_with("8.b.d.0.1.0.0.2.ip6.arpa"));
    }

    #[test]
    fn established_tcp_only_and_full_program_name() {
        let text = "p7\ncSafari Te\nLuser\nf5\nPTCP\ntIPv4\nn192.0.2.5:50000->203.0.113.9:443\nTST=ESTABLISHED\nf6\nPTCP\ntIPv4\nn*:80\nTST=LISTEN\nf7\nPUDP\ntIPv4\nn192.0.2.5:123->203.0.113.9:123\nf8\nPTCP\ntIPv6\nn[2001:db8::5]:50001->[2001:db8::9]:853\nTST=CLOSE_WAIT\n";
        let socks = crate::offen::parse_sockets(text);
        let pf = |_: &[u32]| {
            BTreeMap::from([(7, "/Applications/Safari Technology Preview".to_string())])
        };
        let vs = aus_sockets(socks, &pf);
        assert_eq!(vs.len(), 1);
        assert_eq!(
            (vs[0].ziel.as_str(), vs[0].ziel_port, vs[0].dienst.as_str()),
            ("203.0.113.9", 443, "HTTPS")
        );
        assert_eq!(vs[0].programm, "Safari Technology Preview");
        assert_eq!(dienst(853), "DNS über TLS");
    }
}
