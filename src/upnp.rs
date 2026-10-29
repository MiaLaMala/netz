//! `netzinfo upnp`: UPnP/SSDP devices with their description (issue #42).
//! SSDP is an HTTP dialect over UDP. Answers and descriptions are untrusted input: the
//! description is only fetched from the sender itself, from a private address, without redirects.

use crate::{geraete, netze};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORT: u16 = 1900;
const SAMMELZEIT: Duration = Duration::from_secs(3);
const HTTP_ZEIT: Duration = Duration::from_secs(2);
const MAX_BYTES: u64 = 64 * 1024;
const MAX_TEXT: usize = 120;
const SUCHE: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n";

/// What a device says about itself, stored at `Device.upnp`. Unconfirmed: a device can call
/// itself anything. The serial number is never read into this struct.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Upnp {
    pub udn: Option<String>,
    pub server: Option<String>,
    pub geraetetyp: Option<String>,
    pub hersteller: Option<String>,
    pub modell: Option<String>,
    pub name: Option<String>,
    /// Path of the description on the device, without host.
    pub location_pfad: String,
}

/// One SSDP answer.
#[derive(Debug, PartialEq)]
pub(crate) struct Antwort {
    pub(crate) location: String,
    server: Option<String>,
    usn: Option<String>,
}

pub(crate) fn parse_antwort(b: &[u8]) -> Option<Antwort> {
    let text = String::from_utf8_lossy(b);
    let mut zeilen = text.lines();
    if !zeilen.next()?.starts_with("HTTP/1.1 200") {
        return None; // search answers only; NOTIFY and byebye are not ours
    }
    let mut k: BTreeMap<String, String> = zeilen
        .filter_map(|z| z.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Some(Antwort {
        location: k.remove("location")?,
        server: k.remove("server").map(kuerzen),
        usn: k.remove("usn"),
    })
}

/// `uuid:...` part of the USN, before `::`.
fn udn(usn: &str) -> Option<String> {
    let u = usn.split("::").next()?;
    u.starts_with("uuid:").then(|| kuerzen(u.into()))
}

pub(crate) fn kuerzen(s: String) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

fn privat(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

/// Checks a LOCATION against the sender (SSRF guard) and returns (port, path).
pub(crate) fn location_pruefen(
    location: &str,
    absender: Ipv4Addr,
) -> Result<(u16, String), String> {
    let fremd = |grund: &str| {
        format!(
            "Die Beschreibung von {absender} wird nicht geholt: {grund}. Es wurde nichts gesendet."
        )
    };
    let rest = location
        .strip_prefix("http://")
        .ok_or_else(|| fremd("LOCATION ist kein http://-Link"))?;
    let (hostport, pfad) = rest.find('/').map_or((rest, "/"), |i| rest.split_at(i));
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| fremd("ungültiger Port"))?),
        None => (hostport, 80),
    };
    let ip: Ipv4Addr = host
        .parse()
        .map_err(|_| fremd("LOCATION nennt keine IPv4-Adresse"))?;
    if ip != absender {
        return Err(fremd(
            "LOCATION zeigt auf eine andere Adresse als der Absender",
        ));
    }
    if !privat(ip) {
        return Err(fremd("keine private Adresse"));
    }
    if pfad.chars().any(|c| c <= ' ') {
        return Err(fremd("Steuerzeichen im Pfad"));
    }
    Ok((port, pfad.into()))
}

/// Text between `<tag>` and `</tag>`. Entities: only the five predefined ones; DOCTYPE and
/// custom entities are never read, so `&x;` stays literal text (no XXE).
// ponytail: plain tag search, no namespaces or CDATA. quick-xml (already in Cargo.lock via the
// app) if a device needs it; using it here would be a new dependency of the core crate.
pub(crate) fn tag(xml: &str, name: &str) -> Option<String> {
    let start = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let ende = xml[start..].find(&format!("</{name}>"))?;
    let t = xml[start..start + ende]
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    Some(kuerzen(t.trim().into())).filter(|t| !t.is_empty())
}

/// First (= root device) values of the description XML. `serialNumber` is not read.
fn beschreibung_lesen(xml: &str, a: &Antwort, pfad: String) -> Upnp {
    Upnp {
        udn: a.usn.as_deref().and_then(udn).or_else(|| tag(xml, "UDN")),
        server: a.server.clone(),
        geraetetyp: tag(xml, "deviceType"),
        hersteller: tag(xml, "manufacturer"),
        modell: tag(xml, "modelName"),
        name: tag(xml, "friendlyName"),
        location_pfad: pfad,
    }
}

pub(crate) fn http_holen(ip: Ipv4Addr, port: u16, pfad: &str) -> Result<String, String> {
    let fehler = |e: &dyn std::fmt::Display| {
        format!(
            "Die UPnP-Beschreibung von {ip} ließ sich nicht holen ({e}). Prüfe die Rechte für das lokale Netzwerk."
        )
    };
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    let mut s = TcpStream::connect_timeout(&addr, HTTP_ZEIT).map_err(|e| fehler(&e))?;
    s.set_read_timeout(Some(HTTP_ZEIT))
        .map_err(|e| fehler(&e))?;
    s.set_write_timeout(Some(HTTP_ZEIT))
        .map_err(|e| fehler(&e))?;
    write!(
        s,
        "GET {pfad} HTTP/1.0\r\nHost: {ip}:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| fehler(&e))?;
    let mut buf = Vec::new();
    s.take(MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| fehler(&e))?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(fehler(&"Antwort größer als 64 KB"));
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (kopf, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| fehler(&"keine HTTP-Antwort"))?;
    if !kopf.starts_with("HTTP/1.1 200") && !kopf.starts_with("HTTP/1.0 200") {
        return Err(fehler(
            &"keine Antwort 200, Weiterleitungen folge ich nicht",
        ));
    }
    Ok(body.into())
}

/// Sends the M-SEARCH and collects answers for 3 seconds: one per distinct LOCATION.
// ponytail: IPv4, default interface, `ssdp:all` only. ff02::c and a second `upnp:rootdevice`
// search if devices stay silent.
fn sammeln() -> Result<BTreeMap<String, (Ipv4Addr, Antwort)>, String> {
    let net = |e: std::io::Error| {
        format!(
            "Der SSDP-Socket ließ sich nicht öffnen oder senden ({e}). Prüfe die Netzverbindung und die Rechte für das lokale Netzwerk."
        )
    };
    let u = UdpSocket::bind("0.0.0.0:0").map_err(net)?;
    u.set_multicast_ttl_v4(2).map_err(net)?;
    u.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(net)?;
    u.send_to(SUCHE.as_bytes(), (GROUP, PORT)).map_err(net)?;
    let (ende, mut buf, mut out) = (Instant::now() + SAMMELZEIT, [0u8; 4096], BTreeMap::new());
    while Instant::now() < ende {
        if let Ok((n, SocketAddr::V4(von))) = u.recv_from(&mut buf)
            && let Some(a) = parse_antwort(&buf[..n])
        {
            out.entry(a.location.clone()).or_insert((*von.ip(), a));
        }
    }
    Ok(out)
}

/// Fetches and reads each description; the first usable one per IP wins. Failures per device
/// come back as sentences next to the hits.
fn holen(
    antworten: BTreeMap<String, (Ipv4Addr, Antwort)>,
) -> (BTreeMap<Ipv4Addr, Upnp>, Vec<String>) {
    let ergebnisse: Vec<_> = std::thread::scope(|s| {
        let jobs: Vec<_> = antworten
            .values()
            .map(|(ip, a)| {
                s.spawn(move || {
                    let (port, pfad) = location_pruefen(&a.location, *ip)?;
                    let xml = http_holen(*ip, port, &pfad)?;
                    Ok::<_, String>((*ip, beschreibung_lesen(&xml, a, pfad)))
                })
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    let (mut treffer, mut fehler) = (BTreeMap::new(), Vec::new());
    for r in ergebnisse {
        match r {
            Ok((ip, u)) => {
                treffer.entry(ip).or_insert(u);
            }
            Err(e) => fehler.push(e),
        }
    }
    (treffer, fehler)
}

/// Nothing is sent outside networks marked „meins“.
fn gate(meins: bool) -> Result<(), String> {
    if meins {
        return Ok(());
    }
    Err("`netzinfo upnp` fragt nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es wurde nichts gesendet.".into())
}

/// Puts hits onto stored devices by IP; returns how many matched no stored device.
fn zuordnen(geraete: &mut [geraete::Device], treffer: &BTreeMap<Ipv4Addr, Upnp>) -> usize {
    let mut ohne = 0;
    for (ip, u) in treffer {
        match geraete.iter_mut().find(|g| g.ip == *ip) {
            Some(g) => g.upnp = Some(u.clone()),
            None => ohne += 1,
        }
    }
    ohne
}

fn speichern(treffer: &BTreeMap<Ipv4Addr, Upnp>) -> Result<usize, String> {
    let fp = netze::fingerprint(&crate::hier::collect()?)?;
    let path = netze::data_path()?.with_file_name("geraete.json");
    let mut store: BTreeMap<String, Vec<geraete::Device>> = netze::load_json(&path)?;
    let ohne = zuordnen(store.entry(fp.key()).or_default(), treffer);
    netze::save(&path, &store)?;
    Ok(ohne)
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let e = |t: &str| crate::explain(explain_on, t);
    if let Some(s) = args.first() {
        return Err(format!(
            "Unbekannte Option „{s}“. `netzinfo upnp` kennt nur `--erklaer`."
        ));
    }
    gate(netze::current_is_meins())?;
    e(
        "UPnP ist ein Verfahren, mit dem Geräte sich im Netz selbst vorstellen. Praktisch, aber ein Router mit UPnP kann Programmen erlauben, Ports ins Internet zu öffnen.",
    );
    e(
        "Dieses Netz ist „meins“, darum darf ich fragen: ein Rundruf (M-SEARCH) an 239.255.255.250, Port 1900. Geräte antworten mit einem Link auf ihre Beschreibung.",
    );
    println!("UPnP: 3 Sekunden fragen und zuhören");
    let antworten = sammeln()?;
    e(
        "Die Beschreibung hole ich nur, wenn der Link auf den Absender selbst zeigt und die Adresse privat ist, höchstens 64 KB, ohne Weiterleitung. Sonst könnte ein Gerät mich auf fremde Adressen schicken.",
    );
    let (treffer, fehler) = holen(antworten);
    if treffer.is_empty() {
        println!("Kein UPnP-Gerät antwortet.");
    }
    for (ip, u) in &treffer {
        let f = |o: &Option<String>| o.clone().unwrap_or_else(|| "?".into());
        println!(
            "  {ip:<15} {}  {} {}  {}",
            f(&u.name),
            f(&u.hersteller),
            f(&u.modell),
            f(&u.geraetetyp)
        );
    }
    for f in &fehler {
        eprintln!("  {f}");
    }
    if !treffer.is_empty() {
        let ohne = speichern(&treffer)?;
        e(
            "Gespeichert in geraete.json bei dem gemerkten Gerät mit dieser IP. Die Seriennummer lese ich nicht.",
        );
        if ohne > 0 {
            println!(
                "{ohne} Geräte sind noch nicht gemerkt. Starte `netzinfo geraete` und danach `netzinfo upnp` noch einmal."
            );
        }
    }
    e(
        "Grenzen: Viele Geräte antworten nicht, und jedes Gerät kann sich beliebig nennen. Ein Treffer beweist keine Identität.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANTWORT: &[u8] = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLocation: http://192.0.2.7:49152/desc.xml\r\nSERVER: Linux/5.4 UPnP/1.0 Beispiel/1.0\r\nST: upnp:rootdevice\r\nUSN: uuid:0000-example::upnp:rootdevice\r\n\r\n";
    const XML: &str = "<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Wohnzimmer &amp; Co</friendlyName><manufacturer>Beispiel GmbH</manufacturer><modelName>Box 1</modelName><serialNumber>SERIAL123</serialNumber><deviceList><device><friendlyName>Innen</friendlyName></device></deviceList></device></root>";
    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn parses_answer_and_udn() {
        let a = parse_antwort(ANTWORT).unwrap();
        assert_eq!(a.location, "http://192.0.2.7:49152/desc.xml");
        assert_eq!(udn(a.usn.as_deref().unwrap()).unwrap(), "uuid:0000-example");
        assert_eq!(a.server.as_deref(), Some("Linux/5.4 UPnP/1.0 Beispiel/1.0"));
        assert!(
            parse_antwort(b"NOTIFY * HTTP/1.1\r\nLOCATION: http://192.0.2.7/x\r\n\r\n").is_none()
        );
        assert!(parse_antwort(b"HTTP/1.1 200 OK\r\nST: x\r\n\r\n").is_none());
        assert!(parse_antwort(&[0xff, 0x00, 0x12]).is_none());
    }

    #[test]
    fn reads_description_without_serial() {
        let a = parse_antwort(ANTWORT).unwrap();
        let u = beschreibung_lesen(XML, &a, "/desc.xml".into());
        assert_eq!(u.name.as_deref(), Some("Wohnzimmer & Co"));
        assert_eq!(u.hersteller.as_deref(), Some("Beispiel GmbH"));
        assert_eq!(u.modell.as_deref(), Some("Box 1"));
        assert!(
            u.geraetetyp
                .as_deref()
                .unwrap()
                .ends_with("MediaRenderer:1")
        );
        assert!(!format!("{u:?}").contains("SERIAL123"));
        assert_eq!(beschreibung_lesen("<kaputt", &a, String::new()).name, None);
    }

    #[test]
    fn entities_are_not_resolved() {
        let xml = "<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><root><friendlyName>&x;</friendlyName></root>";
        assert_eq!(tag(xml, "friendlyName").as_deref(), Some("&x;"));
    }

    #[test]
    fn location_must_match_sender_and_be_private() {
        // 192.0.2.x is documentation space, not private: good for the refusals.
        let (s, lo) = (ip("192.0.2.7"), ip("127.0.0.1"));
        assert_eq!(
            location_pruefen("http://127.0.0.1:49152/d.xml", lo),
            Ok((49152, "/d.xml".into()))
        );
        assert_eq!(
            location_pruefen("http://127.0.0.1", lo),
            Ok((80, "/".into()))
        );
        assert!(location_pruefen("http://192.0.2.7/d.xml", s).is_err()); // public
        assert!(location_pruefen("http://127.0.0.2/d.xml", lo).is_err()); // not the sender
        assert!(location_pruefen("http://geraet.example/d.xml", lo).is_err());
        assert!(location_pruefen("https://127.0.0.1/d.xml", lo).is_err());
        assert!(location_pruefen("http://x@127.0.0.1/d.xml", lo).is_err());
        assert!(location_pruefen("http://127.0.0.1/d.xml\r\nX: y", lo).is_err());
    }

    #[test]
    fn gate_blocks_outside_meins_and_hits_join_by_ip() {
        assert!(gate(false).unwrap_err().contains("nichts gesendet"));
        assert!(gate(true).is_ok());
        let mut g = vec![geraete::Device {
            ip: ip("192.0.2.7"),
            ..Default::default()
        }];
        let u = Upnp {
            name: Some("A".into()),
            ..Default::default()
        };
        let t = BTreeMap::from([(ip("192.0.2.7"), u.clone()), (ip("192.0.2.8"), u.clone())]);
        assert_eq!(zuordnen(&mut g, &t), 1);
        assert_eq!(g[0].upnp, Some(u));
    }
}
