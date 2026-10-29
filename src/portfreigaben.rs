//! `netzinfo portfreigaben`: ports the router opened to the internet via UPnP IGD, plus the
//! class of its external address (issue #45). Read-only: only the three actions in `AKTIONEN`
//! are ever sent. Answers are untrusted input, same guards as `upnp` (sender IP, private, 64 KB).

use crate::netze;
use crate::pruefen::Klasse;
use crate::upnp::{http_holen, kuerzen, location_pruefen, parse_antwort, tag};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::time::{Duration, Instant};

const SSDP_PORT: u16 = 1900;
const WARTEZEIT: Duration = Duration::from_secs(2);
const HTTP_ZEIT: Duration = Duration::from_secs(2);
const MAX_BYTES: u64 = 64 * 1024;
const MAX_EINTRAEGE: u32 = 200;
const SUCHE: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
/// The only SOAP actions this module may send. Nothing that changes the router.
const AKTIONEN: [&str; 3] = [
    "GetExternalIPAddress",
    "GetStatusInfo",
    "GetGenericPortMappingEntry",
];
/// Service types we accept, in order of preference. The one used is always one of these
/// constants, never text from the router.
const DIENSTE: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];
/// External ports that are an alarm when open: SSH, Telnet, Remote Desktop, SMB.
/// The sentence for a list the router only gives after login; the app tests against it.
const LISTE_LOGIN: &str = "Der Router zeigt die Liste der Portfreigaben ohne Anmeldung nicht (Fehler 401). Das ist bei einer Fritz!Box üblich.";
const RISKANT: [(u16, &str); 4] = [
    (22, "SSH"),
    (23, "Telnet"),
    (3389, "Remotedesktop"),
    (445, "Dateifreigabe (SMB)"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Freigabe {
    #[serde(rename = "extern")]
    pub extern_port: u16,
    pub intern_ip: String,
    pub intern_port: u16,
    pub protokoll: String,
    pub beschreibung: String,
    pub aktiv: bool,
    pub zuerst_gesehen: u64,
}

impl Freigabe {
    fn id(&self) -> (u16, &str, &str, u16) {
        (
            self.extern_port,
            &self.protokoll,
            &self.intern_ip,
            self.intern_port,
        )
    }
}

/// Per network. The external address itself is never stored, only its class.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Gespeichert {
    pub zeit: u64,
    pub externe_ip_klasse: String,
    pub portfreigaben: Vec<Freigabe>,
}

/// One reading. `externe_ip` is shown live and not written to disk.
#[derive(Debug)]
pub struct Messung {
    pub gateway: Ipv4Addr,
    pub dienst: &'static str,
    pub verbindung: String,
    pub externe_ip: String,
    pub klasse: &'static str,
    pub freigaben: Vec<Freigabe>,
    /// Set when the list could not be read; `freigaben` is then empty and not compared.
    pub listenfehler: Option<String>,
}

/// `privat` and `cgnat` both mean double NAT: the router has no public address.
fn ip_klasse(ip: &str) -> &'static str {
    match ip.parse::<Ipv4Addr>() {
        Ok(a) if a.is_private() || a.is_loopback() || a.is_link_local() => "privat",
        Ok(a) if a.octets()[0] == 100 && (64..128).contains(&a.octets()[1]) => "cgnat",
        Ok(_) => "öffentlich",
        Err(_) => "unbekannt",
    }
}

/// Builds the POST for a whitelisted action. Refuses everything else.
fn anfrage(
    ip: Ipv4Addr,
    port: u16,
    pfad: &str,
    dienst: &str,
    aktion: &str,
    argumente: &str,
) -> Result<String, String> {
    if !AKTIONEN.contains(&aktion) {
        return Err(format!(
            "Die Aktion „{aktion}“ sendet Netzinfo nicht: Es werden nur Portfreigaben gelesen, nie geändert."
        ));
    }
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{aktion} xmlns:u=\"{dienst}\">{argumente}</u:{aktion}></s:Body></s:Envelope>"
    );
    Ok(format!(
        "POST {pfad} HTTP/1.0\r\nHost: {ip}:{port}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{dienst}#{aktion}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

/// Status line and body of a SOAP answer: `Ok(Ok(body))`, `Ok(Err(code))` for a SOAP fault
/// (typically HTTP 500 with `errorCode`), `Err` when it is neither.
fn antwort_lesen(roh: &str) -> Result<Result<String, u32>, String> {
    let kaputt = || "Der Router antwortet nicht wie ein UPnP-Gerät.".to_string();
    let (kopf, body) = roh.split_once("\r\n\r\n").ok_or_else(kaputt)?;
    let status: u16 = kopf
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(kaputt)?;
    if status == 200 {
        return Ok(Ok(body.into()));
    }
    match tag(body, "errorCode").and_then(|c| c.parse().ok()) {
        Some(code) => Ok(Err(code)),
        None => Err(format!("Der Router antwortet mit HTTP {status}.")),
    }
}

fn senden(ip: Ipv4Addr, port: u16, anfrage: &str) -> Result<String, String> {
    let fehler = |e: &dyn std::fmt::Display| {
        format!(
            "Der Router {ip} ließ sich nicht abfragen ({e}). Prüfe die Rechte für das lokale Netzwerk."
        )
    };
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    let mut s = TcpStream::connect_timeout(&addr, HTTP_ZEIT).map_err(|e| fehler(&e))?;
    s.set_read_timeout(Some(HTTP_ZEIT))
        .map_err(|e| fehler(&e))?;
    s.set_write_timeout(Some(HTTP_ZEIT))
        .map_err(|e| fehler(&e))?;
    s.write_all(anfrage.as_bytes()).map_err(|e| fehler(&e))?;
    let mut buf = Vec::new();
    s.take(MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| fehler(&e))?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(fehler(&"Antwort größer als 64 KB"));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn eintrag_lesen(xml: &str, jetzt: u64) -> Result<Freigabe, String> {
    let kaputt = || "Ein Eintrag der Portfreigaben ist nicht lesbar.".to_string();
    let t = |n: &str| tag(xml, n);
    Ok(Freigabe {
        extern_port: t("NewExternalPort")
            .and_then(|p| p.parse().ok())
            .ok_or_else(kaputt)?,
        intern_ip: t("NewInternalClient").ok_or_else(kaputt)?,
        intern_port: t("NewInternalPort")
            .and_then(|p| p.parse().ok())
            .ok_or_else(kaputt)?,
        protokoll: t("NewProtocol").unwrap_or_default().to_ascii_uppercase(),
        beschreibung: t("NewPortMappingDescription").unwrap_or_default(),
        aktiv: t("NewEnabled").is_none_or(|e| e != "0"),
        zuerst_gesehen: jetzt,
    })
}

/// Counts the index up until the router says the list ends (713, or 714 on some routers).
/// Any other fault ends the reading with a sentence; 401 is the usual „not without login“.
fn liste_lesen(
    mut abfrage: impl FnMut(u32) -> Result<Result<String, u32>, String>,
    jetzt: u64,
) -> Result<Vec<Freigabe>, String> {
    let mut out = Vec::new();
    // ponytail: stops at 200 entries without saying so; raise MAX_EINTRAEGE if a router has more.
    for i in 0..MAX_EINTRAEGE {
        match abfrage(i)? {
            Ok(xml) => out.push(eintrag_lesen(&xml, jetzt)?),
            Err(713 | 714) => break,
            Err(401 | 501) => {
                return Err(LISTE_LOGIN.into());
            }
            Err(c) => {
                return Err(format!(
                    "Der Router lehnt die Liste der Portfreigaben ab (Fehler {c})."
                ));
            }
        }
    }
    Ok(out)
}

/// Asks the gateway directly (unicast M-SEARCH) for its IGD description link.
// ponytail: unicast to the gateway only; a multicast search if routers stay silent.
fn igd_suchen(gw: Ipv4Addr) -> Result<Option<(u16, String)>, String> {
    let net = |e: std::io::Error| {
        format!(
            "Der SSDP-Socket ließ sich nicht öffnen oder senden ({e}). Prüfe die Netzverbindung und die Rechte für das lokale Netzwerk."
        )
    };
    let u = UdpSocket::bind("0.0.0.0:0").map_err(net)?;
    u.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(net)?;
    u.send_to(SUCHE.as_bytes(), (gw, SSDP_PORT)).map_err(net)?;
    let (ende, mut buf) = (Instant::now() + WARTEZEIT, [0u8; 4096]);
    while Instant::now() < ende {
        if let Ok((n, SocketAddr::V4(von))) = u.recv_from(&mut buf)
            && *von.ip() == gw
            && let Some(a) = parse_antwort(&buf[..n])
        {
            return location_pruefen(&a.location, gw).map(Some);
        }
    }
    Ok(None)
}

/// Port and path of the preferred WAN connection service's `controlURL`.
fn dienst_finden(xml: &str, gw: Ipv4Addr) -> Option<(&'static str, u16, String)> {
    let dienste: Vec<(String, String)> = xml
        .split("<service>")
        .skip(1)
        .filter_map(|s| Some((tag(s, "serviceType")?, tag(s, "controlURL")?)))
        .collect();
    DIENSTE.iter().find_map(|d| {
        let (_, url) = dienste.iter().find(|(t, _)| t == d)?;
        let (port, pfad) = if url.starts_with("http://") {
            location_pruefen(url, gw).ok()?
        } else {
            (80, format!("/{}", url.trim_start_matches('/')))
        };
        Some((*d, port, pfad))
    })
}

/// The router's UPnP, read-only. `Ok(None)`: no UPnP answer, not an error.
pub fn messen(gw: Ipv4Addr, jetzt: u64) -> Result<Option<Messung>, String> {
    let Some((port, pfad)) = igd_suchen(gw)? else {
        return Ok(None);
    };
    let xml = http_holen(gw, port, &pfad)?;
    let Some((dienst, cport, cpfad)) = dienst_finden(&xml, gw) else {
        return Ok(None);
    };
    let aufruf = |aktion: &str, arg: &str| {
        let roh = senden(gw, cport, &anfrage(gw, cport, &cpfad, dienst, aktion, arg)?)?;
        antwort_lesen(&roh)
    };
    let lesen = |aktion: &str, feld: &str| -> Result<String, String> {
        match aufruf(aktion, "")? {
            Ok(x) => Ok(tag(&x, feld).unwrap_or_default()),
            Err(c) => Err(format!(
                "Der Router lehnt „{aktion}“ ab (Fehler {c}). UPnP ist dort vermutlich eingeschränkt."
            )),
        }
    };
    let externe_ip = lesen("GetExternalIPAddress", "NewExternalIPAddress")?;
    let verbindung = lesen("GetStatusInfo", "NewConnectionStatus")?;
    let liste = liste_lesen(
        |i| {
            aufruf(
                "GetGenericPortMappingEntry",
                &format!("<NewPortMappingIndex>{i}</NewPortMappingIndex>"),
            )
        },
        jetzt,
    );
    let (freigaben, listenfehler) = match liste {
        Ok(l) => (l, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    Ok(Some(Messung {
        gateway: gw,
        dienst,
        klasse: ip_klasse(&externe_ip),
        externe_ip: kuerzen(externe_ip),
        verbindung: kuerzen(verbindung),
        freigaben,
        listenfehler,
    }))
}

/// Carries `zuerst_gesehen` over from the last visit and rates the list. Without an earlier
/// visit nothing is „new“: the first list is the baseline. Risky ports are an alarm every time.
fn vergleichen(alt: Option<&Gespeichert>, neu: &mut [Freigabe]) -> Vec<(Klasse, String)> {
    let mut out = Vec::new();
    for f in neu.iter_mut() {
        let bekannt = alt.and_then(|a| a.portfreigaben.iter().find(|o| o.id() == f.id()));
        if let Some(o) = bekannt {
            f.zuerst_gesehen = o.zuerst_gesehen;
        }
        let ziel = format!(
            "{}:{}{}",
            f.intern_ip,
            f.intern_port,
            if f.beschreibung.is_empty() {
                String::new()
            } else {
                format!(" („{}“)", f.beschreibung)
            }
        );
        if let (true, Some((_, name))) = (f.aktiv, RISKANT.iter().find(|r| r.0 == f.extern_port)) {
            out.push((Klasse::Alarm, format!("Der Router hat Port {} ({}) ins Internet geöffnet, weiter an {ziel}. Das ist der Port für {name}, und der wird aus dem Internet ständig angegriffen. Schließe ihn in der Router-Oberfläche, wenn du ihn nicht selbst brauchst.", f.extern_port, f.protokoll)));
        } else if f.aktiv && alt.is_some() && bekannt.is_none() {
            // A disabled mapping opens nothing, so a new one is no finding.
            out.push((Klasse::Auffaellig, format!("Neue Portfreigabe im Router: Port {} ({}) zeigt auf {ziel}. Sie ist seit dem letzten Besuch dazugekommen. Spielkonsolen und Programme legen so etwas oft selbst an; erwartest du es nicht, schau in der Router-Oberfläche nach.", f.extern_port, f.protokoll)));
        }
    }
    out
}

type Ergebnis = Option<(Messung, Vec<(Klasse, String)>)>;

/// Measures, compares with the last visit and stores. `None`: UPnP is off or unreachable.
fn jetzt_pruefen(daten: &Path, key: &str, gw: Ipv4Addr) -> Result<Ergebnis, String> {
    let jetzt = netze::now();
    let Some(mut m) = messen(gw, jetzt)? else {
        return Ok(None);
    };
    let pfad = daten.with_file_name("portfreigaben.json");
    let mut alle: BTreeMap<String, Gespeichert> = netze::load_json(&pfad)?;
    let mut befunde = Vec::new();
    if m.listenfehler.is_none() {
        befunde = vergleichen(alle.get(key), &mut m.freigaben);
        alle.insert(
            key.into(),
            Gespeichert {
                zeit: jetzt,
                externe_ip_klasse: m.klasse.into(),
                portfreigaben: m.freigaben.clone(),
            },
        );
        netze::save(&pfad, &alle)?;
    }
    Ok(Some((m, befunde)))
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Only in „meins“ networks (it sends); failures are silent here, `netzinfo portfreigaben`
/// shows the reason.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let Some(gw) = meins.then(|| crate::hier::collect().ok()?.router).flatten() else {
        return Vec::new();
    };
    jetzt_pruefen(daten, key, gw)
        .ok()
        .flatten()
        .map(|(_, b)| b)
        .unwrap_or_default()
}

fn gate(meins: bool) -> Result<(), String> {
    if meins {
        return Ok(());
    }
    Err("`netzinfo portfreigaben` fragt nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es wurde nichts gesendet.".into())
}

/// What the app view shows (`netzinfo portfreigaben` as data). `upnp: false`: the router does not
/// answer, everything else is then empty.
#[derive(Debug, Serialize)]
pub struct Anzeige {
    pub gateway: Ipv4Addr,
    pub upnp: bool,
    pub verbindung: String,
    pub externe_ip_klasse: String,
    pub freigaben: Vec<Freigabe>,
    /// The list is only given after login (usual for a Fritz!Box), not a fault.
    pub liste_nur_mit_login: bool,
    pub listenfehler: Option<String>,
    /// `(alarm, sentence)`
    pub befunde: Vec<(bool, String)>,
}

/// Same „meins“ gate as `run`; stores like `run`. For the app.
pub fn anzeige() -> Result<Anzeige, String> {
    gate(netze::current_is_meins())?;
    let info = crate::hier::collect()?;
    let gateway = info.router.ok_or(
        "Kein Gateway gefunden. Verbinde dich mit einem Netz und versuche es noch einmal.",
    )?;
    let key = netze::fingerprint(&info)?.key();
    let leer = Anzeige {
        gateway,
        upnp: false,
        verbindung: String::new(),
        externe_ip_klasse: String::new(),
        freigaben: Vec::new(),
        liste_nur_mit_login: false,
        listenfehler: None,
        befunde: Vec::new(),
    };
    Ok(match jetzt_pruefen(&netze::data_path()?, &key, gateway)? {
        None => leer,
        Some((m, befunde)) => Anzeige {
            upnp: true,
            verbindung: m.verbindung,
            externe_ip_klasse: m.klasse.into(),
            freigaben: m.freigaben,
            liste_nur_mit_login: m.listenfehler.as_deref() == Some(LISTE_LOGIN),
            listenfehler: m.listenfehler.filter(|f| f != LISTE_LOGIN),
            befunde: befunde
                .into_iter()
                .map(|(k, t)| (k == Klasse::Alarm, t))
                .collect(),
            ..leer
        },
    })
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let e = |t: &str| crate::explain(explain_on, t);
    if let Some(s) = args.first() {
        return Err(format!(
            "Unbekannte Option „{s}“. `netzinfo portfreigaben` kennt nur `--erklaer`."
        ));
    }
    gate(netze::current_is_meins())?;
    let info = crate::hier::collect()?;
    let gw = info.router.ok_or(
        "Kein Gateway gefunden. Verbinde dich mit einem Netz und versuche es noch einmal.",
    )?;
    let key = netze::fingerprint(&info)?.key();
    e(
        "Ich frage deinen Router per UPnP, welche Ports er ins Internet geöffnet hat. Dazu ein Rundruf direkt an den Router (Port 1900), dann die Beschreibung und drei Nur-Lesen-Fragen: externe Adresse, Verbindungsstatus, Liste der Freigaben. Ändern kann Netzinfo nichts.",
    );
    let Some((m, befunde)) = jetzt_pruefen(&netze::data_path()?, &key, gw)? else {
        println!("UPnP ist aus oder nicht erreichbar: Router {gw} antwortet nicht auf die Suche.");
        e(
            "Viele Router haben UPnP ausgeschaltet. Das ist kein Fehler, eher gut: Dann kann kein Programm selbst Ports öffnen. Von Hand im Router angelegte Freigaben sehe ich so nicht.",
        );
        return Ok(());
    };
    println!("Router {}", m.gateway);
    println!("  Verbindung:      {}", m.verbindung);
    println!("  Externe Adresse: {} ({})", m.externe_ip, m.klasse);
    if matches!(m.klasse, "privat" | "cgnat") {
        println!("  Doppel-NAT: Der Router hat keine öffentliche Adresse.");
        e(
            "Deine Internetverbindung teilt sich eine Adresse mit anderen Kunden (CGNAT) oder hängt hinter einem zweiten Router. Von außen kannst du dein Netz dann nicht erreichen, und Portfreigaben bringen nichts.",
        );
    }
    if let Some(f) = &m.listenfehler {
        println!("  Portfreigaben: {f}");
        e(
            "Fritz!Box und andere Router zeigen die Liste nur nach einer Anmeldung. Dann bleiben nur externe Adresse und Status.",
        );
    } else if m.freigaben.is_empty() {
        println!("  Portfreigaben: keine");
    } else {
        println!("  Portfreigaben ({}):", m.freigaben.len());
        for f in &m.freigaben {
            println!(
                "    {:<4} {:>5} -> {}:{}  {}  {}",
                f.protokoll,
                f.extern_port,
                f.intern_ip,
                f.intern_port,
                if f.aktiv { "an " } else { "aus" },
                f.beschreibung
            );
        }
        e(
            "Jede Zeile ist eine offene Tür: Was aus dem Internet an diesen Port kommt, geht an das Gerät im Heimnetz. Spielkonsolen machen das oft selbst, zum Beispiel Port 3074.",
        );
    }
    for (klasse, text) in &befunde {
        let wort = if *klasse == Klasse::Alarm {
            "Alarm"
        } else {
            "Auffällig"
        };
        println!("  {wort}: {text}");
    }
    e(
        "Grenzen: Von Hand im Router angelegte Freigaben erscheinen nicht, und die Beschreibungen kann jedes Programm frei wählen. Die externe Adresse speichere ich nicht, nur ihre Klasse.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GW: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
    const EINTRAG: &str = "<s:Envelope><s:Body><u:GetGenericPortMappingEntryResponse><NewRemoteHost></NewRemoteHost><NewExternalPort>3074</NewExternalPort><NewProtocol>udp</NewProtocol><NewInternalPort>3074</NewInternalPort><NewInternalClient>192.0.2.20</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>Konsole</NewPortMappingDescription><NewLeaseDuration>0</NewLeaseDuration></u:GetGenericPortMappingEntryResponse></s:Body></s:Envelope>";
    fn fehler(code: u32) -> String {
        format!(
            "HTTP/1.1 500 Internal Server Error\r\n\r\n<s:Envelope><s:Body><s:Fault><detail><UPnPError><errorCode>{code}</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>"
        )
    }
    fn ok(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\n\r\n{body}")
    }
    /// Feeds raw answers by index, like a router would.
    fn router(antworten: Vec<String>) -> impl FnMut(u32) -> Result<Result<String, u32>, String> {
        move |i| antwort_lesen(&antworten[i as usize])
    }
    fn freigabe(port: u16, aktiv: bool) -> Freigabe {
        Freigabe {
            extern_port: port,
            intern_ip: "192.0.2.20".into(),
            intern_port: port,
            protokoll: "TCP".into(),
            beschreibung: String::new(),
            aktiv,
            zuerst_gesehen: 5,
        }
    }

    #[test]
    fn reads_entries_until_713() {
        let l = liste_lesen(router(vec![ok(EINTRAG), ok(EINTRAG), fehler(713)]), 9).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            (
                l[0].extern_port,
                l[0].protokoll.as_str(),
                l[0].intern_ip.as_str()
            ),
            (3074, "UDP", "192.0.2.20")
        );
        assert_eq!(
            (l[0].beschreibung.as_str(), l[0].aktiv, l[0].zuerst_gesehen),
            ("Konsole", true, 9)
        );
    }

    #[test]
    fn empty_list_and_714_end_the_list() {
        assert!(
            liste_lesen(router(vec![fehler(713)]), 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            liste_lesen(router(vec![ok(EINTRAG), fehler(714)]), 0)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fault_401_and_garbage_are_sentences() {
        let e = liste_lesen(router(vec![fehler(401)]), 0).unwrap_err();
        assert!(e.contains("ohne Anmeldung"));
        assert!(liste_lesen(router(vec![fehler(501)]), 0).is_err());
        assert!(
            liste_lesen(router(vec![fehler(402)]), 0)
                .unwrap_err()
                .contains("402")
        );
        assert!(liste_lesen(router(vec!["HTTP/1.1 404 Not Found\r\n\r\nnix".into()]), 0).is_err());
        assert!(liste_lesen(router(vec!["Quatsch".into()]), 0).is_err());
        assert!(liste_lesen(router(vec![ok("<NewExternalPort>x</NewExternalPort>")]), 0).is_err());
    }

    #[test]
    fn only_read_actions_are_built() {
        for a in AKTIONEN {
            assert!(anfrage(GW, 80, "/c", DIENSTE[0], a, "").is_ok());
        }
        for a in [
            "AddPortMapping",
            "DeletePortMapping",
            "ForceTermination",
            "SetConnectionType",
            "",
        ] {
            assert!(anfrage(GW, 80, "/c", DIENSTE[0], a, "").is_err(), "{a}");
        }
        let r = anfrage(GW, 80, "/c", DIENSTE[1], "GetStatusInfo", "").unwrap();
        assert!(r.contains(
            "SOAPAction: \"urn:schemas-upnp-org:service:WANIPConnection:1#GetStatusInfo\""
        ));
    }

    #[test]
    fn external_ip_is_classed_and_never_stored() {
        assert_eq!(ip_klasse("203.0.113.9"), "öffentlich");
        assert_eq!(ip_klasse("10.1.2.3"), "privat");
        assert_eq!(ip_klasse("100.64.0.1"), "cgnat");
        assert_eq!(ip_klasse("100.127.255.254"), "cgnat");
        assert_eq!(ip_klasse("100.128.0.1"), "öffentlich");
        assert_eq!(ip_klasse(""), "unbekannt");
        let g = Gespeichert {
            zeit: 1,
            externe_ip_klasse: ip_klasse("203.0.113.9").into(),
            portfreigaben: vec![freigabe(3074, true)],
        };
        let json = serde_json::to_string(&g).unwrap();
        assert!(!json.contains("203.0.113.9"));
        assert!(json.contains("\"extern\":3074"));
    }

    #[test]
    fn finds_service_and_rates_new_and_risky_entries() {
        let xml = "<root><device><serviceList><service><serviceType>urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1</serviceType><controlURL>/x</controlURL></service><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>ctl/IPConn</controlURL></service></serviceList></device></root>";
        assert_eq!(
            dienst_finden(xml, GW),
            Some((DIENSTE[1], 80, "/ctl/IPConn".into()))
        );
        assert_eq!(dienst_finden("<root></root>", GW), None);

        let alt = Gespeichert {
            portfreigaben: vec![freigabe(8080, true)],
            ..Default::default()
        };
        let mut neu = vec![
            freigabe(8080, true),
            freigabe(3074, true),
            freigabe(22, true),
            freigabe(445, false),
        ];
        let b = vergleichen(Some(&alt), &mut neu);
        assert_eq!(
            b.iter().map(|x| x.0).collect::<Vec<_>>(),
            vec![Klasse::Auffaellig, Klasse::Alarm]
        );
        // Known entry keeps its first-seen time.
        let mut bekannt = vec![freigabe(8080, true)];
        bekannt[0].zuerst_gesehen = 99;
        vergleichen(Some(&alt), &mut bekannt);
        assert_eq!(bekannt[0].zuerst_gesehen, 5);
        // First visit: baseline, only the alarm remains.
        let mut erste = vec![freigabe(3074, true), freigabe(3389, true)];
        assert_eq!(vergleichen(None, &mut erste).len(), 1);
    }

    #[test]
    fn gate_blocks_outside_meins() {
        assert!(gate(false).unwrap_err().contains("nichts gesendet"));
        assert!(fuer_pruefen(Path::new("/nonexistent/netze.json"), "k", false).is_empty());
        let e = gate(false).unwrap_err();
        assert_eq!(netze::fehlerart(&e), netze::Fehlerart::NichtMeins);
    }

    #[test]
    fn login_sentence_is_the_401_one() {
        let e = liste_lesen(router(vec![fehler(401)]), 0).unwrap_err();
        assert_eq!(e, LISTE_LOGIN);
    }
}
