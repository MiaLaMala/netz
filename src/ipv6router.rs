//! IPv6-Router und Gateway abgleichen. Issue #68.
//!
//! Ohne Root und ohne Mitschnitt: `ndp -rn` listet die Router, die das System aus Router-
//! Advertisements gelernt hat, `ndp -an` ordnet ihre Link-Local-Adresse einer MAC zu. Die MAC
//! vergleichen wir mit der des IPv4-Gateways. Gesehen wird nur, was das System behalten hat.
//! Allein aus diesem Signal entsteht kein Alarm: mehrere Router sind in Mesh- und Firmennetzen
//! normal. Alarm braucht ein zweites Zeichen (der DHCP-Server ist nicht der Router).

use crate::hier::NetInfo;
use crate::netze;
use crate::pruefen::Klasse;
use crate::{explain, ort};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::Path;

/// Lifetime of a router that never expires (`expire=Never`).
const UNBEGRENZT: u64 = u64::MAX;

/// One advertised router, raw. Only lives in memory and on the screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Router {
    pub adresse: Ipv6Addr,
    pub schnittstelle: String,
    /// Seconds left; `UNBEGRENZT` for „Never“.
    pub lebensdauer: u64,
    pub mac: Option<[u8; 6]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Befund {
    /// Routers with a lifetime above 0 on the default-route interface.
    pub router: Vec<Router>,
    /// `None`: gateway MAC or router MAC unknown (e.g. no „Lokales Netzwerk“ grant).
    pub passt_zu_gateway: Option<bool>,
}

fn dauer(text: &str) -> u64 {
    if text == "Never" {
        return UNBEGRENZT;
    }
    let (mut summe, mut zahl) = (0u64, 0u64);
    for c in text.chars() {
        let faktor = match c {
            '0'..='9' => {
                zahl = zahl * 10 + u64::from(c as u8 - b'0');
                continue;
            }
            'd' => 86400,
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => return 0,
        };
        summe += zahl * faktor;
        zahl = 0;
    }
    summe
}

/// `fe80::1%en0` -> the address without the scope.
fn adresse(token: &str) -> Option<Ipv6Addr> {
    token.split('%').next()?.parse().ok()
}

/// `ndp -rn`: `<addr>%<if> if=en0, flags=OT, pref=medium, expire=2m13s`, one router per line.
fn parse_router(out: &str) -> Vec<Router> {
    out.lines()
        .filter_map(|l| {
            let adr = adresse(l.split_whitespace().next()?)?;
            let wert = |k: &str| l.split([' ', ',']).find_map(|t| t.strip_prefix(k));
            Some(Router {
                adresse: adr,
                schnittstelle: wert("if=")?.to_string(),
                lebensdauer: dauer(wert("expire=")?),
                mac: None,
            })
        })
        .collect()
}

/// `ndp -an`: `<addr>%<if>  <mac>  <if>  <expire> ...`; header and rows without a MAC are skipped.
fn parse_nachbarn(out: &str) -> Vec<(Ipv6Addr, [u8; 6])> {
    out.lines()
        .filter_map(|l| {
            let mut t = l.split_whitespace();
            Some((adresse(t.next()?)?, ort::bssid_bytes(t.next()?)?))
        })
        .collect()
}

/// Linux `ip -6 route show default`: `default via <addr> dev eth0 proto ra metric 1024 expires 1798sec`.
/// No `expires` means a static route and counts as never expiring.
fn parse_router_linux(out: &str) -> Vec<Router> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<_> = l.split_whitespace().collect();
            let nach = |k: &str| t.iter().position(|x| *x == k).and_then(|i| t.get(i + 1));
            Some(Router {
                adresse: nach("via")?.parse().ok()?,
                schnittstelle: nach("dev")?.to_string(),
                lebensdauer: nach("expires").map_or(UNBEGRENZT, |e| {
                    e.trim_end_matches("sec").parse().unwrap_or(0)
                }),
                mac: None,
            })
        })
        .collect()
}

/// Linux `ip -6 neigh`: `<addr> dev eth0 lladdr <mac> router REACHABLE`; rows without `lladdr` are skipped.
fn parse_nachbarn_linux(out: &str) -> Vec<(Ipv6Addr, [u8; 6])> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<_> = l.split_whitespace().collect();
            let i = t.iter().position(|x| *x == "lladdr")?;
            Some((adresse(t.first()?)?, ort::bssid_bytes(t.get(i + 1)?)?))
        })
        .collect()
}

fn aufruf(programm: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(programm)
        .args(args)
        .output()
        .map_err(|e| format!("`{programm}` ließ sich nicht starten ({e}). Ist es installiert?"))?;
    if !out.status.success() {
        return Err(format!("`{programm}` meldete einen Fehler."));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn router_lesen() -> Result<Vec<Router>, String> {
    Ok(if cfg!(target_os = "macos") {
        parse_router(&aufruf("/usr/sbin/ndp", &["-rn"])?)
    } else {
        parse_router_linux(&aufruf("ip", &["-6", "route", "show", "default"])?)
    })
}

fn nachbarn_lesen() -> Result<Vec<(Ipv6Addr, [u8; 6])>, String> {
    Ok(if cfg!(target_os = "macos") {
        parse_nachbarn(&aufruf("/usr/sbin/ndp", &["-an"])?)
    } else {
        parse_nachbarn_linux(&aufruf("ip", &["-6", "neigh"])?)
    })
}

/// MAC of the IPv4 gateway from the neighbour table (empty without the local-network grant).
fn gateway_mac(info: &NetInfo) -> Option<[u8; 6]> {
    let gw = info.router?;
    let n = crate::nachbarn::lesen(&info.interface).ok()?;
    ort::bssid_bytes(&n.iter().find(|n| n.ipv4.contains(&gw))?.mac)
}

/// Joins routers, neighbour MACs and the gateway MAC. Pure, so the tests carry the logic.
fn abgleichen(
    mut router: Vec<Router>,
    nachbarn: &[(Ipv6Addr, [u8; 6])],
    schnittstelle: &str,
    gw: Option<[u8; 6]>,
) -> Befund {
    router.retain(|r| r.schnittstelle == schnittstelle && r.lebensdauer > 0);
    for r in &mut router {
        r.mac = nachbarn.iter().find(|n| n.0 == r.adresse).map(|n| n.1);
    }
    let macs: Vec<_> = router.iter().filter_map(|r| r.mac).collect();
    let passt = gw
        .filter(|_| !macs.is_empty())
        .map(|g| macs.iter().all(|m| *m == g));
    Befund {
        router,
        passt_zu_gateway: passt,
    }
}

pub fn befund(info: &NetInfo) -> Result<Befund, String> {
    let router = router_lesen()?;
    let nachbarn = if router.is_empty() {
        Vec::new()
    } else {
        nachbarn_lesen()?
    };
    Ok(abgleichen(
        router,
        &nachbarn,
        &info.interface,
        gateway_mac(info),
    ))
}

pub fn satz(b: &Befund) -> String {
    let bezug = match b.passt_zu_gateway {
        Some(true) => ", passt zum Gateway",
        Some(false) => ", passt nicht zum Gateway",
        None => ", Gateway-Abgleich nicht möglich",
    };
    match b.router.len() {
        0 => "kein IPv6-Router, dieses Netz nutzt wohl kein IPv6".into(),
        1 => format!("ein Router{bezug}"),
        n => format!("{n} Router{bezug}"),
    }
}

/// App row: the sentence plus the numbers, so the view can colour it.
#[derive(Debug, Serialize)]
pub struct Anzeige {
    pub anzahl: usize,
    pub passt_zu_gateway: Option<bool>,
    pub text: String,
}

pub fn anzeige() -> Result<Anzeige, String> {
    let b = befund(&crate::hier::collect()?)?;
    Ok(Anzeige {
        anzahl: b.router.len(),
        passt_zu_gateway: b.passt_zu_gateway,
        text: satz(&b),
    })
}

/// Section of `netzinfo hier`; never fails the command.
pub fn zeigen(info: &NetInfo, explain_on: bool) {
    match befund(info) {
        Ok(b) => {
            println!("IPv6-Router:   {}", satz(&b));
            explain(
                explain_on,
                match b.router.len() {
                    0 => {
                        "Ohne IPv6 im Netz meldet sich kein Gerät als IPv6-Router. Das ist ruhig und kein Fehler."
                    }
                    1 => "Nur dein Router meldet sich als IPv6-Router. Das ist der Normalfall.",
                    _ => {
                        "In diesem Netz melden sich mehrere Geräte als IPv6-Router. Zu Hause sollte nur dein Router das tun; in Firmen- und Mesh-Netzen ist es normal. Ich sehe nur, was der Rechner sich gemerkt hat."
                    }
                },
            );
        }
        Err(e) => println!("IPv6-Router:   nicht lesbar ({e})"),
    }
}

/// What the store keeps per router: hashes only, never the address or the MAC.
#[derive(Debug, Serialize, Deserialize)]
pub struct RouterHash {
    pub link_local_hash: String,
    pub mac_hash: Option<String>,
    pub lebensdauer: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Eintrag {
    pub zeit: u64,
    pub router_anzahl: usize,
    pub passt_zu_gateway: Option<bool>,
    pub router: Vec<RouterHash>,
}

type Store = BTreeMap<String, Eintrag>;

fn eintrag(b: &Befund, salz: &str, zeit: u64) -> Eintrag {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, salz.as_bytes());
    let hash = |bytes: &[u8]| -> String {
        ring::hmac::sign(&key, bytes).as_ref()[..16]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    };
    Eintrag {
        zeit,
        router_anzahl: b.router.len(),
        passt_zu_gateway: b.passt_zu_gateway,
        router: b
            .router
            .iter()
            .map(|r| RouterHash {
                link_local_hash: hash(&r.adresse.octets()),
                mac_hash: r.mac.map(|m| ort::bssid_hash(salz, m)),
                lebensdauer: r.lebensdauer,
            })
            .collect(),
    }
}

/// The rules of the issue. `vorher`: router count of the last calm visit.
fn bewerten(
    vorher: Option<usize>,
    b: &Befund,
    meins: bool,
    dhcp_weicht_ab: bool,
) -> Vec<(Klasse, String)> {
    let n = b.router.len();
    if n == 0 {
        return Vec::new();
    }
    let fremd = b.passt_zu_gateway == Some(false);
    let mehr = n >= 2 && vorher == Some(1);
    if meins && (mehr || fremd) {
        // Alarm only with a second sign; the router signal alone stays „auffällig“.
        let (klasse, zusatz) = if dhcp_weicht_ab {
            (
                Klasse::Alarm,
                " Außerdem ist der DHCP-Server nicht der Router.",
            )
        } else {
            (Klasse::Auffaellig, "")
        };
        let was = if fremd {
            "Ein Router passt nicht zum Gateway"
        } else {
            "Es melden sich mehr IPv6-Router als bei deinem letzten Besuch"
        };
        return vec![(
            klasse,
            format!(
                "IPv6: {was}.{zusatz} Zu Hause sollte nur dein Router sich als IPv6-Router melden."
            ),
        )];
    }
    if !meins && fremd {
        return vec![(
            Klasse::Auffaellig,
            "IPv6: Ein Router in diesem Netz gehört nicht zum Gateway.".into(),
        )];
    }
    vec![(Klasse::Harmlos, format!("IPv6: {}.", satz(b)))]
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let Ok(info) = crate::hier::collect() else {
        return Vec::new();
    };
    let (Ok(b), Ok(salz)) = (befund(&info), ort::salz(&daten.with_file_name("salz.json"))) else {
        return Vec::new();
    };
    let pfad = daten.with_file_name("ipv6router.json");
    let Ok(mut store) = netze::load_json::<Store>(&pfad) else {
        return Vec::new();
    };
    let vorher = store.get(key).map(|e| e.router_anzahl);
    let dhcp_weicht_ab = info.dhcp_server.is_some() && info.dhcp_server != info.router;
    let out = bewerten(vorher, &b, meins, dhcp_weicht_ab);
    // The baseline is the last calm state: extra routers do not overwrite it, so the hint stays
    // until they are gone. ponytail: an added mesh node nags on every visit.
    if !b.router.is_empty() && !(b.router.len() >= 2 && vorher == Some(1)) {
        store.insert(key.to_string(), eintrag(&b, &salz, netze::now()));
        let _ = netze::save(&pfad, &store);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Format of macOS `ndp -rn` / `ndp -an`, addresses replaced by fe80::/RFC 7042 values.
    const EIN_ROUTER: &str = "fe80::5eff:fe00:5301%en0 if=en0, flags=OT, pref=medium, expire=29m58s\nfe80::1%utun0 if=utun0, flags=IST, pref=medium, expire=Never\n";
    const ZWEI_ROUTER: &str = "fe80::5eff:fe00:5301%en0 if=en0, flags=OT, pref=medium, expire=29m58s\nfe80::5eff:fe00:5302%en0 if=en0, flags=OT, pref=low, expire=1h2m3s\n";
    const NACHBARN: &str = "Neighbor                                Linklayer Address  Netif Expire    St Flgs Prbs\nfe80::5eff:fe00:5301%en0                0:0:5e:0:53:1      en0   23h59m50s S         R\nfe80::5eff:fe00:5302%en0                0:0:5e:0:53:2      en0   23h59m50s S         R\nfe80::5eff:fe00:5303%en0                (incomplete)       en0   expired   N\n";
    const GW: [u8; 6] = [0, 0, 0x5e, 0, 0x53, 1];

    fn lauf(router: &str, gw: Option<[u8; 6]>) -> Befund {
        abgleichen(parse_router(router), &parse_nachbarn(NACHBARN), "en0", gw)
    }

    #[test]
    fn linux_ausgabe_gleicher_befund() {
        let router = "default via fe80::5eff:fe00:5301 dev en0 proto ra metric 1024 expires 1798sec hoplimit 64 pref medium\ndefault via fe80::5eff:fe00:5302 dev en0 metric 1024 pref medium\n";
        let nachbarn = "fe80::5eff:fe00:5301 dev en0 lladdr 00:00:5e:00:53:01 router REACHABLE\nfe80::5eff:fe00:5303 dev en0  FAILED\n";
        let r = parse_router_linux(router);
        assert_eq!((r[0].lebensdauer, r[1].lebensdauer), (1798, UNBEGRENZT));
        let n = parse_nachbarn_linux(nachbarn);
        assert_eq!(n.len(), 1);
        let b = abgleichen(r[..1].to_vec(), &n, "en0", Some(GW));
        assert_eq!(b.passt_zu_gateway, Some(true));
        assert!(parse_router_linux("").is_empty());
    }

    #[test]
    fn leer_ist_ruhig() {
        let b = lauf("", Some(GW));
        assert!(b.router.is_empty() && b.passt_zu_gateway.is_none());
        assert!(bewerten(Some(1), &b, true, true).is_empty());
        assert!(satz(&b).starts_with("kein IPv6-Router"));
    }

    #[test]
    fn ein_router_passt_zum_gateway() {
        let b = lauf(EIN_ROUTER, Some(GW));
        assert_eq!(b.router.len(), 1, "utun0 gehört nicht zu en0");
        assert_eq!(b.router[0].lebensdauer, 29 * 60 + 58);
        assert_eq!(b.passt_zu_gateway, Some(true));
        assert_eq!(bewerten(Some(1), &b, true, true)[0].0, Klasse::Harmlos);
    }

    #[test]
    fn zwei_router_alarm_nur_mit_zweitem_zeichen() {
        let b = lauf(ZWEI_ROUTER, Some(GW));
        assert_eq!(b.router[1].lebensdauer, 3723);
        assert_eq!(b.passt_zu_gateway, Some(false));
        assert_eq!(bewerten(Some(1), &b, true, false)[0].0, Klasse::Auffaellig);
        assert_eq!(bewerten(Some(1), &b, true, true)[0].0, Klasse::Alarm);
        // public network: conspicuous at most, never an alarm
        assert_eq!(bewerten(Some(1), &b, false, true)[0].0, Klasse::Auffaellig);
        // two routers from the first visit and no gateway MAC to compare: calm
        let mesh = lauf(ZWEI_ROUTER, None);
        assert_eq!(bewerten(None, &mesh, true, true)[0].0, Klasse::Harmlos);
    }

    #[test]
    fn abgelaufen_und_unbekannte_mac() {
        let alt = "fe80::5eff:fe00:5301%en0 if=en0, flags=O, pref=medium, expire=0s\n";
        assert!(lauf(alt, Some(GW)).router.is_empty());
        assert_eq!(lauf(EIN_ROUTER, None).passt_zu_gateway, None);
    }

    #[test]
    fn store_enthaelt_nur_hashes() {
        let b = lauf(ZWEI_ROUTER, Some(GW));
        let json = serde_json::to_string(&eintrag(&b, "salz", 1)).unwrap();
        for roh in ["fe80", "5301", "00:00:5e", "0:0:5e"] {
            assert!(!json.contains(roh), "{roh} steht im Store");
        }
        assert!(json.contains("\"router_anzahl\":2"));
    }
}
