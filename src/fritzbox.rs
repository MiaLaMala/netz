//! `netzinfo fritzbox`: die Geräteliste der Fritz!Box über TR-064 (Issue #65).
//!
//! Nur lesend: `Hosts:1` mit `GetHostNumberOfEntries` und `GetGenericHostEntry` (laut AVM-Dokument
//! „TR-064 Support - Hosts“ ohne besonderes Recht). Anmeldung per HTTP Digest (MD5, RFC 2617),
//! wie „TR-064 - First Steps“ es beschreibt. Das Passwort liegt nur im Schlüsselbund, geht nie
//! als Argument an einen anderen Prozess und steht in keiner Datei. Antworten sind
//! unbekannte Eingaben: private Adresse, 64 KB, kein Umleiten.

use crate::geraete::Device;
use crate::namen::{self, Name, Quelle};
use crate::netze;
use crate::upnp::tag;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

const PORT: u16 = 49000;
const ZEIT: Duration = Duration::from_secs(3);
const MAX_BYTES: u64 = 64 * 1024;
const MAX_GERAETE: u32 = 512;
const DIENST: &str = "urn:dslforum-org:service:Hosts:1";
/// Die einzigen SOAP-Aktionen dieses Moduls. Nichts, was die Fritz!Box ändert.
const AKTIONEN: [&str; 2] = ["GetHostNumberOfEntries", "GetGenericHostEntry"];

/// Was ein Abruf ergeben hat; die App zeigt je Wert einen eigenen Zustand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zustand {
    Ok,
    KeinZugang,
    ZugangFalsch,
    Tr064Aus,
    NichtErreichbar,
    KeineFritzbox,
}

#[derive(Debug, Serialize)]
pub struct Abruf {
    pub zustand: Zustand,
    /// Ein Satz, was los ist und was zu tun ist; leer bei `Ok`.
    pub satz: String,
    pub gefunden: usize,
    /// Neu in der eigenen Liste: Geräte, die auf keinen Scan geantwortet haben.
    pub neu: usize,
}

/// Was der Router zu einem Gerät in `Device::router` sagt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterInfo {
    /// „LAN“, „WLAN“, „Powerline“ oder „unbekannt“.
    pub verbindung: String,
    pub aktiv: bool,
    /// Unix-Sekunden des letzten Abrufs.
    pub zuletzt: u64,
    /// Nur der Router kennt das Gerät: Ein Scan hat es noch nie gefunden.
    #[serde(default)]
    pub nur_router: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Host {
    mac: String,
    ip: Option<Ipv4Addr>,
    name: Option<String>,
    verbindung: &'static str,
    aktiv: bool,
}

type Stoerung = (Zustand, String);

fn stoerung(z: Zustand, satz: &str) -> Stoerung {
    (z, satz.into())
}

// ---- Zugangsdaten (nur Schlüsselbund) ----

#[derive(Clone, PartialEq)]
pub struct Zugang {
    pub benutzer: String,
    pub passwort: String,
}

impl std::fmt::Debug for Zugang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Zugang({}, ***)", self.benutzer)
    }
}

#[cfg(target_os = "macos")]
mod schluesselbund {
    use super::Zugang;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    const DIENST: &str = "dev.mia-gruenwald.netzinfo.fritzbox";
    const KONTO: &str = "tr064";
    const NICHT_GEFUNDEN: i32 = -25300;

    fn fehler(e: security_framework::base::Error) -> String {
        format!(
            "Der Schlüsselbund hat den Zugriff abgelehnt ({e}). Erlaube Netzinfo den Zugriff, wenn macOS fragt."
        )
    }

    // Benutzer und Passwort in einem Eintrag, getrennt durch einen Zeilenumbruch (darin nicht erlaubt).
    pub fn lesen() -> Result<Option<Zugang>, String> {
        match get_generic_password(DIENST, KONTO) {
            Ok(b) => {
                let t = String::from_utf8_lossy(&b).into_owned();
                Ok(t.split_once('\n').map(|(b, p)| Zugang {
                    benutzer: b.into(),
                    passwort: p.into(),
                }))
            }
            Err(e) if e.code() == NICHT_GEFUNDEN => Ok(None),
            Err(e) => Err(fehler(e)),
        }
    }

    pub fn speichern(z: &Zugang) -> Result<(), String> {
        set_generic_password(
            DIENST,
            KONTO,
            format!("{}\n{}", z.benutzer, z.passwort).as_bytes(),
        )
        .map_err(fehler)
    }

    pub fn entfernen() -> Result<(), String> {
        match delete_generic_password(DIENST, KONTO) {
            Err(e) if e.code() != NICHT_GEFUNDEN => Err(fehler(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod schluesselbund {
    use super::Zugang;

    fn nein<T>() -> Result<T, String> {
        Err("Zugangsdaten für die Fritz!Box legt Netzinfo nur im macOS-Schlüsselbund ab. Unter Linux und Windows ist das noch nicht unterstützt. Eine Datei mit dem Passwort gibt es bewusst nicht.".into())
    }
    pub fn lesen() -> Result<Option<Zugang>, String> {
        nein()
    }
    pub fn speichern(_: &Zugang) -> Result<(), String> {
        nein()
    }
    pub fn entfernen() -> Result<(), String> {
        nein()
    }
}

/// Liest den Zugang aus dem Schlüsselbund (`None`: noch keiner angelegt).
pub fn zugang_lesen() -> Result<Option<Zugang>, String> {
    schluesselbund::lesen()
}

pub fn zugang_speichern(benutzer: &str, passwort: &str) -> Result<(), String> {
    let (b, p) = (benutzer.trim(), passwort);
    if b.is_empty() || p.is_empty() {
        return Err("Trage Benutzer und Passwort ein.".into());
    }
    if [b, p].iter().any(|s| s.contains(['\n', '\r', '"', '\\'])) {
        return Err(
            "Benutzer und Passwort dürfen keinen Zeilenumbruch, kein „\"“ und kein „\\“ enthalten."
                .into(),
        );
    }
    schluesselbund::speichern(&Zugang {
        benutzer: b.into(),
        passwort: p.into(),
    })
}

pub fn zugang_entfernen() -> Result<(), String> {
    schluesselbund::entfernen()
}

// ---- HTTP Digest (RFC 2617) ----

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `response` für `qop=auth`, ohne `qop` die ältere Form nach RFC 2069.
fn digest_antwort(
    z: &Zugang,
    methode: &str,
    uri: &str,
    h: &Herausforderung,
    nc: &str,
    cnonce: &str,
) -> String {
    let ha1 = md5_hex(&format!("{}:{}:{}", z.benutzer, h.realm, z.passwort));
    let ha2 = md5_hex(&format!("{methode}:{uri}"));
    if h.qop {
        md5_hex(&format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", h.nonce))
    } else {
        md5_hex(&format!("{ha1}:{}:{ha2}", h.nonce))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Herausforderung {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    qop: bool,
}

/// Wert von `name="..."` aus dem Kopf, nur an einer Wortgrenze (`nonce` trifft nicht `cnonce`).
fn parameter(kopf: &str, name: &str) -> Option<String> {
    let such = format!("{name}=\"");
    let mut ab = 0;
    while let Some(i) = kopf[ab..].find(&such) {
        let start = ab + i;
        let wortanfang = kopf[..start]
            .chars()
            .next_back()
            .is_none_or(|c| c == ' ' || c == ',');
        if wortanfang {
            let rest = &kopf[start + such.len()..];
            return rest.split_once('"').map(|(w, _)| w.to_string());
        }
        ab = start + such.len();
    }
    None
}

fn herausforderung(kopf: &str) -> Option<Herausforderung> {
    let zeile = kopf
        .lines()
        .find(|z| z.to_ascii_lowercase().starts_with("www-authenticate:"))?;
    let (_, werte) = zeile.split_once(':')?;
    let werte = werte.trim().strip_prefix("Digest ")?;
    Some(Herausforderung {
        realm: parameter(werte, "realm")?,
        nonce: parameter(werte, "nonce")?,
        opaque: parameter(werte, "opaque"),
        qop: parameter(werte, "qop").is_some_and(|q| q.split(',').any(|w| w.trim() == "auth")),
    })
}

struct Sitzung {
    h: Herausforderung,
    nc: u32,
}

fn zufall_hex() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 8];
    // Schlägt der Zufall fehl, bleibt die Zahl 0: schwächer, aber die Anmeldung geht weiter.
    let _ = SystemRandom::new().fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn auth_kopf(z: &Zugang, s: &mut Sitzung, pfad: &str) -> String {
    s.nc += 1;
    let (nc, cnonce) = (format!("{:08x}", s.nc), zufall_hex());
    let r = digest_antwort(z, "POST", pfad, &s.h, &nc, &cnonce);
    let mut k = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{pfad}\", response=\"{r}\", algorithm=MD5",
        z.benutzer, s.h.realm, s.h.nonce
    );
    if s.h.qop {
        k += &format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\"");
    }
    if let Some(o) = &s.h.opaque {
        k += &format!(", opaque=\"{o}\"");
    }
    k
}

// ---- SOAP ----

/// Baut den POST für eine erlaubte Aktion. Alles andere wird abgelehnt, bevor etwas gesendet wird.
fn anfrage(
    ip: Ipv4Addr,
    port: u16,
    pfad: &str,
    aktion: &str,
    argumente: &str,
    auth: Option<&str>,
) -> Result<String, String> {
    if !AKTIONEN.contains(&aktion) {
        return Err(format!(
            "Die Aktion „{aktion}“ sendet Netzinfo nicht: Es wird nur die Geräteliste gelesen, nie etwas geändert."
        ));
    }
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{aktion} xmlns:u=\"{DIENST}\">{argumente}</u:{aktion}></s:Body></s:Envelope>"
    );
    let auth = auth.map_or(String::new(), |a| format!("Authorization: {a}\r\n"));
    Ok(format!(
        "POST {pfad} HTTP/1.0\r\nHost: {ip}:{port}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{DIENST}#{aktion}\"\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

fn senden(ip: Ipv4Addr, port: u16, anfrage: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(ip), port), ZEIT)?;
    s.set_read_timeout(Some(ZEIT))?;
    s.set_write_timeout(Some(ZEIT))?;
    s.write_all(anfrage.as_bytes())?;
    let mut buf = Vec::new();
    s.take(MAX_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(std::io::Error::other("Antwort größer als 64 KB"));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn io_stoerung(ip: Ipv4Addr, e: &std::io::Error) -> Stoerung {
    if e.kind() == std::io::ErrorKind::ConnectionRefused {
        return stoerung(Zustand::Tr064Aus, SATZ_AUS);
    }
    let warum = match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            "antwortet nicht innerhalb von 3 s".to_string()
        }
        _ => format!("ist nicht erreichbar ({})", crate::router::io_satz(e)),
    };
    (
        Zustand::NichtErreichbar,
        format!(
            "Die Fritz!Box {ip} {warum}. Prüfe die Verbindung und die Rechte für das lokale Netzwerk."
        ),
    )
}

const SATZ_AUS: &str = "TR-064 ist in der Fritz!Box aus. Schalte es ein unter Heimnetz, Netzwerk, Netzwerkeinstellungen, Heimnetzfreigaben: „Zugriff für Anwendungen zulassen“.";

/// Statuszeile, Kopf und Körper einer HTTP-Antwort.
fn zerlegen(roh: &str) -> Option<(u16, &str, &str)> {
    let (kopf, body) = roh.split_once("\r\n\r\n")?;
    let status = kopf.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, kopf, body))
}

/// Eine Aktion mit Anmeldung. Der erste Versuch zeigt die Herausforderung (401) oder nutzt die
/// Sitzung weiter; nach einem 401 gibt es genau einen zweiten Versuch mit frischem `nonce`.
fn aufruf(
    ip: Ipv4Addr,
    port: u16,
    pfad: &str,
    aktion: &str,
    arg: &str,
    z: &Zugang,
    sitzung: &mut Option<Sitzung>,
) -> Result<String, Stoerung> {
    for versuch in 0..2 {
        let auth = sitzung.as_mut().map(|s| auth_kopf(z, s, pfad));
        let a = anfrage(ip, port, pfad, aktion, arg, auth.as_deref())
            .map_err(|e| (Zustand::NichtErreichbar, e))?;
        let roh = senden(ip, port, &a).map_err(|e| io_stoerung(ip, &e))?;
        let Some((status, kopf, body)) = zerlegen(&roh) else {
            return Err(stoerung(
                Zustand::KeineFritzbox,
                "Der Router antwortet nicht wie eine Fritz!Box.",
            ));
        };
        match status {
            200 => return Ok(body.into()),
            401 if versuch == 0 => {
                let Some(h) = herausforderung(kopf) else {
                    return Err(stoerung(
                        Zustand::NichtErreichbar,
                        "Die Fritz!Box verlangt eine Anmeldung, die Netzinfo nicht kennt.",
                    ));
                };
                *sitzung = Some(Sitzung { h, nc: 0 });
            }
            401 => {
                return Err(stoerung(
                    Zustand::ZugangFalsch,
                    "Die Fritz!Box lehnt Benutzer oder Passwort ab. Prüfe sie unter System, Fritz!Box-Benutzer, oder richte den Zugang neu ein.",
                ));
            }
            503 => {
                return Err(stoerung(
                    Zustand::NichtErreichbar,
                    "Die Fritz!Box nimmt gerade keine Anfragen an (nach zu vielen Versuchen). Warte eine Minute und versuche es noch einmal.",
                ));
            }
            _ => {
                return match tag(body, "errorCode").and_then(|c| c.parse::<u32>().ok()) {
                    Some(606) => Err(stoerung(
                        Zustand::ZugangFalsch,
                        "Dieser Benutzer darf die Geräteliste nicht lesen. Gib ihm in der Fritz!Box das Recht „FRITZ!Box Einstellungen“.",
                    )),
                    Some(c) => Err((
                        Zustand::NichtErreichbar,
                        format!("Die Fritz!Box lehnt die Abfrage ab (Fehler {c})."),
                    )),
                    None => Err((
                        Zustand::NichtErreichbar,
                        format!("Die Fritz!Box antwortet mit HTTP {status}."),
                    )),
                };
            }
        }
    }
    Err(stoerung(
        Zustand::ZugangFalsch,
        "Die Anmeldung bei der Fritz!Box ist fehlgeschlagen. Prüfe Benutzer und Passwort oder richte den Zugang neu ein.",
    ))
}

fn host_lesen(xml: &str) -> Option<Host> {
    let mac = crate::hersteller::parse_mac(&tag(xml, "NewMACAddress")?)?
        .map(|b| format!("{b:02x}"))
        .join(":");
    Some(Host {
        mac,
        ip: tag(xml, "NewIPAddress").and_then(|i| i.parse().ok()),
        name: tag(xml, "NewHostName"),
        verbindung: match tag(xml, "NewInterfaceType").as_deref() {
            Some("Ethernet") => "LAN",
            Some("802.11") => "WLAN",
            Some("HomePlug") => "Powerline",
            _ => "unbekannt",
        },
        aktiv: tag(xml, "NewActive").is_some_and(|a| a == "1"),
    })
}

/// Pfad der `Hosts:1`-Steuerung aus der Beschreibung; `None`, wenn es keine Fritz!Box ist.
fn steuerpfad(xml: &str) -> Option<String> {
    if !tag(xml, "manufacturer")?.contains("AVM") {
        return None;
    }
    let pfad = xml
        .split("<service>")
        .skip(1)
        .find(|s| tag(s, "serviceType").as_deref() == Some(DIENST))
        .and_then(|s| tag(s, "controlURL"))?;
    // Der Pfad kommt in die Anfragezeile: nur sichtbare Zeichen, keine Leerzeichen.
    (pfad.starts_with('/') && pfad.chars().all(|c| c.is_ascii_graphic())).then_some(pfad)
}

fn holen(ip: Ipv4Addr, port: u16, z: &Zugang) -> Result<Vec<Host>, Stoerung> {
    let get =
        format!("GET /tr64desc.xml HTTP/1.0\r\nHost: {ip}:{port}\r\nConnection: close\r\n\r\n");
    let roh = senden(ip, port, &get).map_err(|e| io_stoerung(ip, &e))?;
    let (status, _, beschreibung) = zerlegen(&roh).ok_or_else(|| {
        stoerung(
            Zustand::KeineFritzbox,
            "Der Router antwortet nicht wie eine Fritz!Box.",
        )
    })?;
    if status == 404 {
        return Err(stoerung(Zustand::Tr064Aus, SATZ_AUS));
    }
    let pfad = (status == 200)
        .then(|| steuerpfad(beschreibung))
        .flatten()
        .ok_or_else(|| {
            stoerung(
                Zustand::KeineFritzbox,
                "Der Router ist keine Fritz!Box (oder meldet TR-064 anders). Netzinfo liest nur die Fritz!Box.",
            )
        })?;
    let mut sitzung = None;
    let anzahl = aufruf(
        ip,
        port,
        &pfad,
        "GetHostNumberOfEntries",
        "",
        z,
        &mut sitzung,
    )?;
    let n: u32 = tag(&anzahl, "NewHostNumberOfEntries")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| {
            stoerung(
                Zustand::NichtErreichbar,
                "Die Fritz!Box nennt keine Zahl der Geräte.",
            )
        })?;
    let mut out = Vec::new();
    // ponytail: höchstens 512 Geräte; MAX_GERAETE anheben, wenn ein Netz mehr hat.
    for i in 0..n.min(MAX_GERAETE) {
        let arg = format!("<NewIndex>{i}</NewIndex>");
        let xml = aufruf(
            ip,
            port,
            &pfad,
            "GetGenericHostEntry",
            &arg,
            z,
            &mut sitzung,
        )?;
        out.extend(host_lesen(&xml));
    }
    Ok(out)
}

// ---- Zusammenführen ----

/// Mischt die Router-Liste über die MAC in die eigene Liste. Unbekannte aktive Geräte kommen
/// neu dazu; unbekannte inaktive nicht (die Fritz!Box merkt sich Geräte von vor Jahren).
/// Gibt die Zahl der neu angelegten Geräte zurück.
fn einmischen(bekannt: &mut Vec<Device>, hosts: Vec<Host>, jetzt: u64) -> usize {
    let mut neu = 0;
    for h in hosts {
        let mut info = RouterInfo {
            verbindung: h.verbindung.into(),
            aktiv: h.aktiv,
            zuletzt: jetzt,
            nur_router: false,
        };
        // Ohne MAC (Scan ohne `--nachbarn`) entscheidet die IP, dann bekommt das Gerät die MAC.
        let gleich = |d: &Device| {
            d.mac.as_deref() == Some(h.mac.as_str())
                || (d.mac.is_none() && h.ip == Some(d.ip) && h.aktiv)
        };
        let name = h.name.clone().map(|n| Name {
            quelle: Quelle::Router,
            name: n,
            zuletzt: jetzt,
        });
        let i = match bekannt.iter().position(gleich) {
            Some(i) => i,
            None => {
                let Some(ip) = h.ip.filter(|_| h.aktiv) else {
                    continue;
                };
                neu += 1;
                info.nur_router = true;
                bekannt.push(Device {
                    ip,
                    first_seen: jetzt,
                    last_seen: jetzt,
                    besuche: 1,
                    ..Default::default()
                });
                bekannt.len() - 1
            }
        };
        let d = &mut bekannt[i];
        if d.mac.is_none() {
            d.mac = Some(h.mac.clone());
            if let Some(i) = crate::hersteller::fuer_text(&h.mac) {
                d.hersteller = i.name;
                d.mac_zufaellig = i.zufaellig;
            }
        }
        // Ein Gerät, das nur der Router kannte, bleibt es, bis ein Scan es findet (`geraete::merge`).
        info.nur_router |= d.router.as_ref().is_some_and(|r| r.nur_router);
        d.router = Some(info);
        if let Some(n) = name {
            namen::eintragen(&mut d.namen, n);
        }
    }
    bekannt.sort_by_key(|d| d.ip);
    neu
}

fn privat(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

/// Abruf mit Pfaden und Port als Parameter (für die Tests). Ohne Zugang wird nichts gesendet.
fn abrufen_in(
    pfad: &Path,
    key: &str,
    gw: Ipv4Addr,
    port: u16,
    zugang: Option<Zugang>,
    jetzt: u64,
) -> Result<Abruf, String> {
    let ende = |zustand, satz: &str| Abruf {
        zustand,
        satz: satz.into(),
        gefunden: 0,
        neu: 0,
    };
    if !privat(gw) {
        return Err(format!(
            "Das Gateway {gw} ist keine private Adresse, Netzinfo fragt dort nichts ab."
        ));
    }
    let Some(z) = zugang else {
        return Ok(ende(
            Zustand::KeinZugang,
            "Für die Geräteliste der Fritz!Box braucht Netzinfo einen eigenen Zugang.",
        ));
    };
    let hosts = match holen(gw, port, &z) {
        Ok(h) => h,
        Err((zustand, satz)) => return Ok(ende(zustand, &satz)),
    };
    let gefunden = hosts.len();
    let mut alle: std::collections::BTreeMap<String, Vec<Device>> = netze::load_json(pfad)?;
    let neu = einmischen(alle.entry(key.into()).or_default(), hosts, jetzt);
    netze::save(pfad, &alle)?;
    Ok(Abruf {
        zustand: Zustand::Ok,
        satz: String::new(),
        gefunden,
        neu,
    })
}

fn gate(meins: bool) -> Result<(), String> {
    if meins {
        return Ok(());
    }
    Err("`netzinfo fritzbox` fragt nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es wurde nichts gesendet.".into())
}

/// Liest die Liste der Fritz!Box dieses Netzes und mischt sie in `geraete.json`.
/// `lesen` holt den Zugang (der Schlüsselbund; die Tests geben eine eigene Funktion).
pub fn abrufen(lesen: impl FnOnce() -> Result<Option<Zugang>, String>) -> Result<Abruf, String> {
    gate(netze::current_is_meins())?;
    let info = crate::hier::collect()?;
    let gw = info.router.ok_or(
        "Kein Gateway gefunden. Verbinde dich mit einem Netz und versuche es noch einmal.",
    )?;
    let key = netze::fingerprint(&info)?.key();
    let pfad = netze::data_path()?.with_file_name("geraete.json");
    abrufen_in(&pfad, &key, gw, PORT, lesen()?, netze::now())
}

// ---- Kommandozeile ----

pub const OPTIONEN: &[&str] = &["--zugang", "--entfernen"];

#[cfg(unix)]
fn passwort_fragen() -> Result<String, String> {
    let (tty, mut t) = (unsafe { libc::isatty(0) } == 1, unsafe {
        std::mem::zeroed::<libc::termios>()
    });
    let aus = tty && unsafe { libc::tcgetattr(0, &mut t) } == 0;
    let alt = t;
    if aus {
        t.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &t) };
    }
    eprint!("Passwort: ");
    let mut zeile = String::new();
    let r = std::io::stdin().read_line(&mut zeile);
    if aus {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &alt) };
        eprintln!();
    }
    r.map_err(|e| {
        format!(
            "Das Passwort ließ sich nicht lesen ({}).",
            crate::io_satz(&e)
        )
    })?;
    Ok(zeile.trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(not(unix))]
fn passwort_fragen() -> Result<String, String> {
    Err("Das Passwort lässt sich hier nicht abfragen.".into())
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "fritzbox")?;
    let e = |t: &str| crate::explain(explain_on, t);
    if args.iter().any(|a| a == "--entfernen") {
        zugang_entfernen()?;
        println!("Der Zugang ist aus dem Schlüsselbund entfernt.");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--zugang") {
        let benutzer = args.get(i + 1).filter(|b| !b.starts_with("--")).ok_or(
            "Nach `--zugang` fehlt der Benutzer, z. B. `netzinfo fritzbox --zugang netzinfo`.",
        )?;
        e(
            "Das Passwort fragt Netzinfo im Terminal ab und legt es im macOS-Schlüsselbund ab. Es steht nie in einer Datei und nie als Argument in der Prozessliste.",
        );
        zugang_speichern(benutzer, &passwort_fragen()?)?;
        crate::satz(&format!(
            "Der Zugang für „{benutzer}“ liegt im Schlüsselbund."
        ));
        return Ok(());
    }
    if let Some(a) = args.iter().find(|a| !a.starts_with("--")) {
        return Err(format!(
            "Unbekannte Eingabe „{a}“. `netzinfo fritzbox` kennt `--zugang <Benutzer>`, `--entfernen` und `--erklaer`."
        ));
    }
    e(
        "Diese Liste kommt vom Router selbst. Der Router weiß auch von Geräten, die auf Netzinfo nicht antworten.",
    );
    e(
        "TR-064 ist der Fernzugriff, den die Fritz!Box im Heimnetz anbietet. Netzinfo meldet sich dort mit HTTP Digest an: Das Passwort geht dabei nicht im Klartext über das Netz, nur ein MD5-Wert aus Passwort und einer Zahl, die die Fritz!Box vorgibt. Gefragt wird nur nach der Geräteliste, nie wird etwas geändert.",
    );
    let r = abrufen(zugang_lesen)?;
    match r.zustand {
        Zustand::Ok => crate::satz(&format!(
            "{} {} vom Router gelesen, {} davon neu in deiner Liste.",
            r.gefunden,
            if r.gefunden == 1 { "Gerät" } else { "Geräte" },
            r.neu
        )),
        _ => crate::satz(&r.satz),
    }
    if r.zustand == Zustand::KeinZugang {
        crate::satz(
            "Lege ihn an mit `netzinfo fritzbox --zugang <Benutzer>`. Der Benutzer braucht in der Fritz!Box das Recht „FRITZ!Box Einstellungen“.",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    const DESC: &str = include_str!("../testdata/fritzbox/tr64desc.xml");
    const ANZAHL: &str = include_str!("../testdata/fritzbox/anzahl.xml");
    const LEER: &str = include_str!("../testdata/fritzbox/leer.xml");
    const HOSTS: [&str; 3] = [
        include_str!("../testdata/fritzbox/host_0.xml"),
        include_str!("../testdata/fritzbox/host_1.xml"),
        include_str!("../testdata/fritzbox/host_2.xml"),
    ];
    const F606: &str = include_str!("../testdata/fritzbox/fehler_606.xml");
    const GEHEIM: &str = "geheim-passwort-4711";

    fn zugang(p: &str) -> Zugang {
        Zugang {
            benutzer: "netzinfo".into(),
            passwort: p.into(),
        }
    }

    /// Ein Server, der sich wie die Fritz!Box verhält: Digest mit `GEHEIM`, Fixtures als Antwort.
    /// `modus`: "normal", "leer", "606", "nur401" .
    fn server(modus: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let (mut kopf, mut zeile) = (String::new(), String::new());
                while r.read_line(&mut zeile).is_ok_and(|n| n > 2) {
                    kopf += &zeile;
                    zeile.clear();
                }
                let len = kopf
                    .lines()
                    .find_map(|z| z.strip_prefix("Content-Length: "))
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let body = String::from_utf8_lossy(&body).into_owned();
                let antwort = |status: &str, extra: &str, text: &str| {
                    let mut s = s.try_clone().unwrap();
                    let _ = write!(
                        s,
                        "HTTP/1.0 {status}\r\n{extra}Content-Length: {}\r\n\r\n{text}",
                        text.len()
                    );
                };
                if kopf.starts_with("GET /tr64desc.xml") {
                    let d = if modus == "keinavm" {
                        DESC.replace("AVM", "Andere")
                    } else {
                        DESC.into()
                    };
                    antwort("200 OK", "", &d);
                    continue;
                }
                let h = Herausforderung {
                    realm: "F!Box SOAP-Auth".into(),
                    nonce: "abc123".into(),
                    opaque: None,
                    qop: true,
                };
                let ok = kopf
                    .lines()
                    .find_map(|z| z.strip_prefix("Authorization: "))
                    .is_some_and(|a| {
                        let (nc, cn) = (
                            parameter_roh(a, "nc"),
                            parameter(a, "cnonce").unwrap_or_default(),
                        );
                        let erwartet = digest_antwort(
                            &zugang(GEHEIM),
                            "POST",
                            "/upnp/control/hosts",
                            &h,
                            &nc,
                            &cn,
                        );
                        parameter(a, "response") == Some(erwartet)
                            && parameter(a, "username").as_deref() == Some("netzinfo")
                    });
                if !ok {
                    let c = "WWW-Authenticate: Digest realm=\"F!Box SOAP-Auth\", nonce=\"abc123\", algorithm=MD5, qop=\"auth\"\r\n";
                    antwort("401 Unauthorized", c, "");
                } else if modus == "606" {
                    antwort("500 Internal Server Error", "", F606);
                } else if body.contains("GetHostNumberOfEntries") {
                    antwort("200 OK", "", if modus == "leer" { LEER } else { ANZAHL });
                } else {
                    let i: usize = tag(&body, "NewIndex").unwrap().parse().unwrap();
                    antwort("200 OK", "", HOSTS[i]);
                }
            }
        });
        port
    }

    /// `nc=00000001` hat keine Anführungszeichen.
    fn parameter_roh(kopf: &str, name: &str) -> String {
        let s = format!("{name}=");
        let i = kopf.find(&s).unwrap() + s.len();
        kopf[i..].split(',').next().unwrap().trim().to_string()
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("netz-fritz-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const LO: Ipv4Addr = Ipv4Addr::LOCALHOST;

    #[test]
    fn digest_gegen_rechenbeispiel_aus_rfc_2617() {
        let z = Zugang {
            benutzer: "Mufasa".into(),
            passwort: "Circle Of Life".into(),
        };
        let h = Herausforderung {
            realm: "testrealm@host.com".into(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".into(),
            opaque: None,
            qop: true,
        };
        assert_eq!(
            digest_antwort(&z, "GET", "/dir/index.html", &h, "00000001", "0a4f113b"),
            "6629fae49393a05397450978507c4ef1"
        );
    }

    #[test]
    fn herausforderung_wird_gelesen_und_cnonce_nicht_verwechselt() {
        let kopf = "HTTP/1.0 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"F!Box SOAP-Auth\", nonce=\"N1\", algorithm=MD5, qop=\"auth\"\r\n";
        let h = herausforderung(kopf).unwrap();
        assert_eq!(
            (h.realm.as_str(), h.nonce.as_str(), h.qop),
            ("F!Box SOAP-Auth", "N1", true)
        );
        assert_eq!(
            parameter("cnonce=\"x\", nonce=\"y\"", "nonce").as_deref(),
            Some("y")
        );
        assert!(
            herausforderung("HTTP/1.0 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n")
                .is_none()
        );
    }

    #[test]
    fn nur_leseaktionen_werden_gebaut() {
        for a in AKTIONEN {
            assert!(anfrage(LO, PORT, "/upnp/control/hosts", a, "", None).is_ok());
        }
        for a in [
            "X_AVM-DE_SetFriendlyNameByMAC",
            "SetHostNameByMACAddress",
            "X_AVM-DE_WakeOnLANByMACAddress",
            "GetHostListPath",
            "",
        ] {
            assert!(
                anfrage(LO, PORT, "/upnp/control/hosts", a, "", None)
                    .unwrap_err()
                    .contains("nie etwas geändert")
            );
        }
    }

    #[test]
    fn host_aus_fixture_und_inaktiv_ohne_ip() {
        let h = host_lesen(HOSTS[0]).unwrap();
        assert_eq!(
            (h.mac.as_str(), h.verbindung, h.aktiv, h.ip),
            (
                "00:00:5e:00:53:20",
                "LAN",
                true,
                Some("192.0.2.20".parse().unwrap())
            )
        );
        assert_eq!(host_lesen(HOSTS[1]).unwrap().verbindung, "WLAN");
        let aus = host_lesen(HOSTS[2]).unwrap();
        assert!(!aus.aktiv && aus.ip.is_none());
        assert!(host_lesen("<NewMACAddress>kaputt</NewMACAddress>").is_none());
    }

    #[test]
    fn beschreibung_nur_von_avm() {
        assert_eq!(steuerpfad(DESC).as_deref(), Some("/upnp/control/hosts"));
        assert!(steuerpfad(&DESC.replace("AVM", "Andere")).is_none());
        assert!(steuerpfad(&DESC.replace("/upnp/control/hosts", "/a b\r\nX: y")).is_none());
    }

    #[test]
    fn liste_vom_server_und_einmischen() {
        let port = server("normal");
        let r = holen(LO, port, &zugang(GEHEIM)).unwrap();
        assert_eq!(r.len(), 3);
        let mut bekannt = vec![Device {
            ip: "192.0.2.20".parse().unwrap(),
            mac: Some("00:00:5e:00:53:20".into()),
            ..Default::default()
        }];
        let neu = einmischen(&mut bekannt, r, 100);
        // macbook bekannt (nur ergänzt), Steckdose aktiv und neu, Tablet inaktiv und unbekannt: nicht übernommen
        assert_eq!((neu, bekannt.len()), (1, 2));
        let mac = &bekannt[0];
        assert_eq!(mac.router.as_ref().unwrap().verbindung, "LAN");
        assert_eq!(mac.namen[0].quelle, Quelle::Router);
        let dose = &bekannt[1];
        assert_eq!(
            (
                dose.ip.to_string().as_str(),
                dose.router.as_ref().unwrap().verbindung.as_str()
            ),
            ("192.0.2.50", "WLAN")
        );
        assert_eq!(namen::bester(dose).unwrap().name, "steckdose-flur");
        assert!(
            dose.router.as_ref().unwrap().nur_router && !mac.router.as_ref().unwrap().nur_router
        );
        // Ein Scan, der die Steckdose findet, nimmt „nur Router“ weg; findet er sie nicht, ist sie nicht „verschwunden“.
        let lo: Ipv4Addr = "192.0.2.1".parse().unwrap();
        let hi: Ipv4Addr = "192.0.2.254".parse().unwrap();
        let (dip, dmac) = (dose.ip, dose.mac.clone());
        let (_, weg) = crate::geraete::merge(&mut bekannt, vec![], lo, hi);
        assert_eq!(weg.len(), 1, "nur das Gerät, das ein Scan schon kannte");
        let gefunden = Device {
            ip: dip,
            mac: dmac,
            ..Default::default()
        };
        crate::geraete::merge(&mut bekannt, vec![gefunden], lo, hi);
        assert!(!bekannt[1].router.as_ref().unwrap().nur_router);
    }

    #[test]
    fn leere_liste_falsches_passwort_und_fehlendes_recht() {
        let p = server("leer");
        assert!(holen(LO, p, &zugang(GEHEIM)).unwrap().is_empty());
        let p = server("normal");
        let e = holen(LO, p, &zugang("falsch")).unwrap_err();
        assert_eq!(e.0, Zustand::ZugangFalsch);
        let p = server("606");
        let e = holen(LO, p, &zugang(GEHEIM)).unwrap_err();
        assert!(e.0 == Zustand::ZugangFalsch && e.1.contains("FRITZ!Box Einstellungen"));
        let p = server("keinavm");
        assert_eq!(
            holen(LO, p, &zugang(GEHEIM)).unwrap_err().0,
            Zustand::KeineFritzbox
        );
    }

    #[test]
    fn nichts_lauscht_heisst_tr064_aus() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let e = holen(LO, port, &zugang(GEHEIM)).unwrap_err();
        assert_eq!(e.0, Zustand::Tr064Aus);
        assert!(e.1.contains("Zugriff für Anwendungen zulassen"));
    }

    #[test]
    fn ohne_zugang_wird_nichts_gesendet_und_fremde_adresse_abgelehnt() {
        let d = tmp("ohne");
        // Port 1 wäre ein Fehler, wenn gesendet würde: Der Zustand beweist, dass es nicht geschah.
        let r = abrufen_in(&d.join("g.json"), "k", LO, 1, None, 1).unwrap();
        assert_eq!(r.zustand, Zustand::KeinZugang);
        assert!(!d.join("g.json").exists());
        assert!(
            abrufen_in(
                &d.join("g.json"),
                "k",
                "8.8.8.8".parse().unwrap(),
                1,
                None,
                1
            )
            .is_err()
        );
        assert!(gate(false).unwrap_err().contains("als „meins“ markiert"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn zugangsdaten_landen_in_keiner_datei() {
        let d = tmp("dateien");
        let port = server("normal");
        let r = abrufen_in(
            &d.join("geraete.json"),
            "k",
            LO,
            port,
            Some(zugang(GEHEIM)),
            5,
        )
        .unwrap();
        assert_eq!((r.zustand, r.gefunden, r.neu), (Zustand::Ok, 3, 2));
        let mut gelesen = 0;
        for f in std::fs::read_dir(&d).unwrap().flatten() {
            let t = std::fs::read_to_string(f.path()).unwrap();
            assert!(!t.contains(GEHEIM), "Zugang in {:?}", f.path());
            gelesen += 1;
        }
        assert!(gelesen > 0);
        assert!(!format!("{:?}", zugang(GEHEIM)).contains(GEHEIM));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn zugang_wird_geprueft_bevor_er_gespeichert_wird() {
        assert!(zugang_speichern("", "x").is_err());
        assert!(zugang_speichern("a", "").is_err());
        assert!(
            zugang_speichern("a\nb", "x")
                .unwrap_err()
                .contains("Zeilenumbruch")
        );
        assert!(zugang_speichern("a", "x\"y").is_err());
    }
}
