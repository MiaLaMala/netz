//! Ausgehende Ports prüfen: was darf aus diesem Netz raus. Issue #54.
//!
//! Pro Port ein TCP-Connect zu einem eigenen Testserver (`egress_ziel`), in zufälliger Reihenfolge,
//! höchstens 4 gleichzeitig, 100 ms Abstand (Idee der OONI-Spezifikation ts-038, nur die
//! Spezifikation gelesen). Ohne Ziel wird nichts gesendet. Nur TCP, kein Root.
//! ponytail: Das Ziel steht in `egress.json` neben `netze.json` (gesetzt mit `--ziel`), es gibt keine
//! eigene Konfigurationsdatei; bei mehr Einstellungen eine gemeinsame Konfiguration einführen.

use crate::netze;
use crate::pruefen::Klasse;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FRIST: Duration = Duration::from_secs(3);
const ABSTAND: Duration = Duration::from_millis(100);
const GLEICHZEITIG: usize = 4;
const PORTS: [(u16, &str); 16] = [
    (22, "SSH"),
    (25, "Mailversand zwischen Servern"),
    (53, "DNS über TCP"),
    (80, "HTTP"),
    (123, "NTP über TCP"),
    (143, "IMAP"),
    (443, "HTTPS"),
    (465, "SMTPS"),
    (587, "Mailversand"),
    (853, "DNS über TLS"),
    (993, "IMAPS"),
    (1194, "OpenVPN"),
    (3389, "Remotedesktop"),
    (5222, "XMPP-Chat"),
    (8080, "HTTP, Ausweichport"),
    (8443, "HTTPS, Ausweichport"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Offen,
    Abgelehnt,
    Verschluckt,
    Fehler,
}

/// `ports`: (Port, Status, Millisekunden bis zum Ergebnis), sortiert nach Port.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EgressErgebnis {
    pub ziel: String,
    pub ports: Vec<(u16, Status, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Gespeichert {
    zeit: u64,
    offen: Vec<u16>,
    zu: Vec<u16>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Speicher {
    ziel: Option<String>,
    netze: BTreeMap<String, Gespeichert>,
}

fn name(port: u16) -> &'static str {
    PORTS.iter().find(|p| p.0 == port).map_or("", |p| p.1)
}

fn klassifizieren(e: &std::io::Error) -> Status {
    match e.kind() {
        ErrorKind::ConnectionRefused => Status::Abgelehnt,
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Status::Verschluckt,
        _ => Status::Fehler,
    }
}

fn mischen(v: &mut [u16], mut seed: u64) {
    seed |= 1; // xorshift needs a non-zero state; mixing quality is irrelevant here
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        v.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

/// Probes every port with `probe`, shuffled by `seed`, at most `GLEICHZEITIG` at once, `abstand` between starts.
fn messen(
    ports: &[u16],
    seed: u64,
    abstand: Duration,
    probe: impl Fn(u16) -> Status + Sync,
) -> Vec<(u16, Status, u64)> {
    let mut reihe = ports.to_vec();
    mischen(&mut reihe, seed);
    let next = Mutex::new(0usize);
    let erg = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..GLEICHZEITIG {
            s.spawn(|| {
                loop {
                    let port = {
                        let Ok(mut i) = next.lock() else { return };
                        let Some(&p) = reihe.get(*i) else { return };
                        *i += 1;
                        std::thread::sleep(abstand); // under the lock: starts are `abstand` apart
                        p
                    };
                    let t = Instant::now();
                    let st = probe(port);
                    if let Ok(mut e) = erg.lock() {
                        e.push((port, st, t.elapsed().as_millis() as u64));
                    }
                }
            });
        }
    });
    let mut e = erg.into_inner().unwrap_or_default();
    e.sort_by_key(|p| p.0);
    e
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// Resolves `ziel` (IPv4 preferred) and probes all ports.
pub fn pruefen(ziel: &str) -> Result<EgressErgebnis, String> {
    let adressen: Vec<SocketAddr> = (ziel, 0)
        .to_socket_addrs()
        .map_err(|_| {
            format!(
                "Das Ziel „{ziel}“ ließ sich nicht auflösen. Prüfe den Namen und die Verbindung."
            )
        })?
        .collect();
    let ip = adressen
        .iter()
        .find(|a| a.is_ipv4())
        .or(adressen.first())
        .ok_or_else(|| format!("Zu „{ziel}“ gibt es keine Adresse. Prüfe den Namen."))?
        .ip();
    let ports: Vec<u16> = PORTS.iter().map(|p| p.0).collect();
    let ports = messen(
        &ports,
        seed(),
        ABSTAND,
        |p| match TcpStream::connect_timeout(&SocketAddr::new(ip, p), FRIST) {
            Ok(_) => Status::Offen,
            Err(e) => klassifizieren(&e),
        },
    );
    Ok(EgressErgebnis {
        ziel: ziel.to_string(),
        ports,
    })
}

fn gespeichert(e: &EgressErgebnis) -> Gespeichert {
    let mit = |f: fn(Status) -> bool| {
        e.ports
            .iter()
            .filter(|p| f(p.1))
            .map(|p| p.0)
            .collect::<Vec<_>>()
    };
    Gespeichert {
        zeit: netze::now(),
        offen: mit(|s| s == Status::Offen),
        zu: mit(|s| matches!(s, Status::Abgelehnt | Status::Verschluckt)),
    }
}

/// Without any open port the test server is probably unreachable: nothing to compare or store.
fn aussagekraeftig(g: &Gespeichert) -> bool {
    !g.offen.is_empty()
}

fn bezeichnung(port: u16) -> String {
    format!("{port} ({})", name(port))
}

/// Ports closed now that were not closed before. Never an alarm.
fn aenderung(vorher: &Gespeichert, jetzt: &Gespeichert, meins: bool) -> Vec<(Klasse, String)> {
    let neu: Vec<String> = jetzt
        .zu
        .iter()
        .filter(|p| !vorher.zu.contains(p))
        .map(|&p| bezeichnung(p))
        .collect();
    if neu.is_empty() {
        return Vec::new();
    }
    let klasse = if meins {
        Klasse::Auffaellig
    } else {
        Klasse::Harmlos
    };
    vec![(
        klasse,
        format!(
            "Dieses Netz lässt jetzt Port {} nicht mehr hinaus, vorher ging es. Mit `netzinfo ports --raus` siehst du alle Ports.",
            neu.join(", ")
        ),
    )]
}

fn pfad(daten: &Path) -> PathBuf {
    daten.with_file_name("egress.json")
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Measures only if a test server is configured.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let p = pfad(daten);
    let Ok(mut sp) = netze::load_json::<Speicher>(&p) else {
        return Vec::new();
    };
    let Some(ziel) = sp.ziel.clone() else {
        return Vec::new();
    };
    let Ok(e) = pruefen(&ziel) else {
        return Vec::new();
    };
    let jetzt = gespeichert(&e);
    if !aussagekraeftig(&jetzt) {
        return Vec::new();
    }
    let befund = sp
        .netze
        .get(key)
        .map(|v| aenderung(v, &jetzt, meins))
        .unwrap_or_default();
    sp.netze.insert(key.to_string(), jetzt);
    let _ = netze::save(&p, &sp);
    befund
}

fn satz(port: u16, s: Status) -> String {
    let n = bezeichnung(port);
    match (port, s) {
        (443, Status::Offen) => "Port 443 (HTTPS) ist offen. Normale Webseiten und die meisten VPNs über HTTPS funktionieren.".into(),
        (22, Status::Abgelehnt | Status::Verschluckt) => "Dieses Netz lässt Port 22 (SSH) nicht hinaus. Verbindungen per SSH nach außen funktionieren hier nicht.".into(),
        (_, Status::Offen) => format!("Port {n} ist offen."),
        (_, Status::Abgelehnt) => format!("Port {n} wird abgelehnt: Die Antwort kam sofort, typisch für eine Firewall, die ausdrücklich ablehnt."),
        (_, Status::Verschluckt) => format!("Port {n} wird verschluckt: Es kam keine Antwort, typisch für eine Firewall, die still verwirft."),
        (_, Status::Fehler) => format!("Port {n} ließ sich nicht prüfen (Netz oder Ziel nicht erreichbar)."),
    }
}

/// One row for CLI and app: `satz` is the explanation of this port's result.
#[derive(Debug, Clone, Serialize)]
pub struct PortZeile {
    pub port: u16,
    pub name: &'static str,
    pub status: Status,
    pub ms: u64,
    pub satz: String,
}

/// Result of one run; `hinweise` are changes, or why nothing was stored.
#[derive(Debug, Clone, Serialize)]
pub struct EgressAntwort {
    pub ziel: String,
    pub ports: Vec<PortZeile>,
    pub hinweise: Vec<String>,
}

/// Measures against `ziel` (or the saved one; a given `ziel` is saved) and, in a remembered network,
/// stores the result and reports newly closed ports. Used by CLI and app.
pub fn ausfuehren(ziel: Option<String>) -> Result<EgressAntwort, String> {
    let daten = netze::data_path()?;
    let p = pfad(&daten);
    let mut sp: Speicher = netze::load_json(&p)?;
    if let Some(z) = ziel {
        if z.is_empty() || z.contains(|c: char| c.is_whitespace() || c == '/') {
            return Err(format!(
                "„{z}“ ist kein gültiges Ziel. Gib einen Namen oder eine IP-Adresse an, z. B. `--ziel 203.0.113.7`."
            ));
        }
        sp.ziel = Some(z);
        netze::save(&p, &sp)?;
    }
    let ziel = sp.ziel.clone().ok_or("Es ist kein Testserver eingestellt. Stelle einen auf, der auf allen Ports der Liste lauscht (siehe README, „Ausgehende Ports“), und gib ihn einmal an: `netzinfo ports --raus --ziel <Server>`.")?;
    let e = pruefen(&ziel)?;
    let mut hinweise = Vec::new();
    let jetzt = gespeichert(&e);
    if !aussagekraeftig(&jetzt) {
        hinweise.push("Kein Port war offen. Läuft dein Testserver, und hast du Internet? Das Ergebnis wird nicht gespeichert.".to_string());
    } else {
        let bekannt = crate::hier::collect().ok().and_then(|info| {
            let fp = netze::fingerprint(&info).ok()?;
            let list = netze::load(&daten).ok()?;
            netze::find(&list, &fp).map(|i| (fp.key(), list[i].meins))
        });
        match bekannt {
            Some((key, meins)) => {
                if let Some(v) = sp.netze.get(&key) {
                    hinweise.extend(aenderung(v, &jetzt, meins).into_iter().map(|a| a.1));
                }
                sp.netze.insert(key, jetzt);
                netze::save(&p, &sp)?;
            }
            None => hinweise.push("Dieses Netz ist nicht gemerkt, deshalb speichere ich nichts. `netzinfo merken <Name>` legt es an.".to_string()),
        }
    }
    Ok(EgressAntwort {
        ziel: e.ziel,
        ports: e
            .ports
            .iter()
            .map(|&(port, status, ms)| PortZeile {
                port,
                name: name(port),
                status,
                ms,
                satz: satz(port, status),
            })
            .collect(),
        hinweise,
    })
}

pub const OPTIONEN: &[&str] = &["--raus", "--ziel"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "ports --raus")?;
    let mut ziel = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--raus" => {}
            "--ziel" => {
                ziel = Some(it.next().cloned().ok_or("Nach `--ziel` fehlt der Name oder die Adresse deines Testservers, z. B. `--ziel 203.0.113.7`.")?);
            }
            s => {
                return Err(format!(
                    "Unbekannte Option „{s}“. `netzinfo ports --raus` kennt nur `--ziel <Server>`."
                ));
            }
        }
    }
    crate::explain(
        explain_on,
        "Ich öffne zu jedem Port auf deinem Testserver eine TCP-Verbindung, in zufälliger Reihenfolge, höchstens vier gleichzeitig. Kommt die Verbindung zustande, darf der Port hinaus. Kommt sofort eine Ablehnung, lehnt eine Firewall ausdrücklich ab. Kommt nichts zurück, verwirft sie still.",
    );
    let a = ausfuehren(ziel)?;
    println!("Ausgehende Ports zu {}", a.ziel);
    for z in &a.ports {
        println!("  {}  ({} ms)", z.satz, z.ms);
    }
    for h in &a.hinweise {
        println!("{h}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn lokaler_server_offen_abgelehnt_und_timeout() {
        let a = TcpListener::bind("127.0.0.1:0").unwrap();
        let offen = a.local_addr().unwrap().port();
        let b = TcpListener::bind("127.0.0.1:0").unwrap();
        let zu = b.local_addr().unwrap().port();
        drop(b); // nobody listens: the kernel answers with RST
        let stumm = 1; // stands for a port whose SYN is dropped
        let r = messen(&[offen, zu, stumm], 7, Duration::ZERO, |p| {
            if p == stumm {
                klassifizieren(&std::io::Error::from(ErrorKind::TimedOut))
            } else {
                match TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], p)), FRIST) {
                    Ok(_) => Status::Offen,
                    Err(e) => klassifizieren(&e),
                }
            }
        });
        let st = |p| r.iter().find(|x| x.0 == p).unwrap().1;
        assert_eq!(st(offen), Status::Offen);
        assert_eq!(st(zu), Status::Abgelehnt);
        assert_eq!(st(stumm), Status::Verschluckt);
        assert_eq!(
            klassifizieren(&std::io::Error::from(ErrorKind::HostUnreachable)),
            Status::Fehler
        );
    }

    #[test]
    fn reihenfolge_gemischt_und_hoechstens_vier_gleichzeitig() {
        let alle: Vec<u16> = PORTS.iter().map(|p| p.0).collect();
        let (jetzt, spitze) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let r = messen(&alle, 42, Duration::ZERO, |_| {
            let n = jetzt.fetch_add(1, Ordering::SeqCst) + 1;
            spitze.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            jetzt.fetch_sub(1, Ordering::SeqCst);
            Status::Offen
        });
        assert_eq!(r.len(), 16);
        let s = spitze.load(Ordering::SeqCst);
        assert!(s <= GLEICHZEITIG, "Spitze {s}"); // a lower bound would depend on the scheduler
        let (mut a, mut b) = (alle.clone(), alle.clone());
        mischen(&mut a, 42);
        mischen(&mut b, 44); // 43 would equal 42: the lowest bit is forced to 1
        assert_ne!(a, alle);
        assert_ne!(a, b);
        a.sort();
        assert_eq!(a, alle);
    }

    #[test]
    fn abstand_zwischen_den_starts() {
        let t = Instant::now();
        messen(&[1, 2, 3, 4, 5], 1, Duration::from_millis(30), |_| {
            Status::Offen
        });
        assert!(t.elapsed() >= Duration::from_millis(150));
    }

    #[test]
    fn nur_neu_gesperrte_ports_sind_ein_befund() {
        let v = Gespeichert {
            zeit: 0,
            offen: vec![22, 443],
            zu: vec![25],
        };
        let j = Gespeichert {
            zeit: 1,
            offen: vec![443],
            zu: vec![22, 25],
        };
        let eigen = aenderung(&v, &j, true);
        assert_eq!(eigen.len(), 1);
        assert_eq!(eigen[0].0, Klasse::Auffaellig);
        assert!(eigen[0].1.contains("22 (SSH)") && !eigen[0].1.contains("25"));
        assert_eq!(aenderung(&v, &j, false)[0].0, Klasse::Harmlos);
        assert!(aenderung(&j, &j, true).is_empty());
        assert!(!aussagekraeftig(&Gespeichert {
            zeit: 0,
            offen: vec![],
            zu: vec![22]
        }));
    }
}
