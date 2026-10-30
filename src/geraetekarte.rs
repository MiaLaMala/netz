//! Gerätekarte: ein Eintrag je Gerät aus allen Quellen, mit Typ und Hinweisen. Issue #29.
//!
//! The sources (ports, Bonjour, MAC maker, neighbour table) are already merged per device in
//! `geraete.json` (`geraete::Device`, key: MAC, else IP). This module only reads that store:
//! it guesses a type from a rule table and lists facts as hints. It sends nothing.
//!
//! ponytail: the rules are a `const` table here, not `data/typen.toml`: no toml dependency, the
//! compiler checks it. Upgrade path: a file the user can edit, if people want their own rules.
//! UPnP is not a source in Netzinfo yet, so there is no UPnP condition.

use crate::geraete::Device;
use crate::netze;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::Path;

/// A correction by the user. Never overwritten by a scan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VonMia {
    pub name: Option<String>,
    pub typ: Option<String>,
}

/// Name, symbol, and the phrase for „hält dieses Gerät für …“.
pub const TYPEN: &[(&str, &str, &str)] = &[
    ("Router", "🌐", "einen Router"),
    ("Drucker", "🖨️", "einen Drucker"),
    (
        "Medien",
        "📺",
        "ein Medien-Gerät (Fernseher oder Lautsprecher)",
    ),
    ("Smart Home", "🏠", "ein Smart-Home-Gerät"),
    ("Computer", "💻", "einen Computer"),
    ("Handy", "📱", "ein Handy oder Tablet"),
];
const UNBEKANNT: (&str, &str) = ("Unbekannt", "❔");

enum Wenn {
    /// Bonjour service type, e.g. `_ipp._tcp`.
    Dienst(&'static str),
    /// Lower-case part of the maker name.
    Hersteller(&'static str),
    Port(u16),
}
use Wenn::{Dienst, Hersteller, Port};

struct Regel {
    wenn: Wenn,
    typ: &'static str,
    punkte: u8,
    /// Completes „… weil es …“.
    grund: &'static str,
}

const fn r(wenn: Wenn, typ: &'static str, punkte: u8, grund: &'static str) -> Regel {
    Regel {
        wenn,
        typ,
        punkte,
        grund,
    }
}

/// Points add up per type; the highest sum wins, a tie is „Unbekannt“.
const REGELN: &[Regel] = &[
    r(
        Dienst("_ipp._tcp"),
        "Drucker",
        3,
        "den Bonjour-Dienst für Drucken (_ipp._tcp) anbietet",
    ),
    r(
        Dienst("_ipps._tcp"),
        "Drucker",
        3,
        "den Bonjour-Dienst für Drucken (_ipps._tcp) anbietet",
    ),
    r(
        Dienst("_printer._tcp"),
        "Drucker",
        3,
        "den Bonjour-Dienst für Drucken (_printer._tcp) anbietet",
    ),
    r(
        Dienst("_pdl-datastream._tcp"),
        "Drucker",
        3,
        "den Bonjour-Dienst für Druckdaten (_pdl-datastream._tcp) anbietet",
    ),
    r(Port(631), "Drucker", 2, "den Druckport 631 (IPP) offen hat"),
    r(Port(9100), "Drucker", 2, "den Druckport 9100 offen hat"),
    r(
        Dienst("_airplay._tcp"),
        "Medien",
        3,
        "den Bonjour-Dienst AirPlay (_airplay._tcp) anbietet",
    ),
    r(
        Dienst("_raop._tcp"),
        "Medien",
        3,
        "den Bonjour-Dienst für AirPlay-Ton (_raop._tcp) anbietet",
    ),
    r(
        Dienst("_googlecast._tcp"),
        "Medien",
        3,
        "den Bonjour-Dienst Cast (_googlecast._tcp) anbietet",
    ),
    r(Port(8009), "Medien", 2, "den Cast-Port 8009 offen hat"),
    r(Port(7000), "Medien", 1, "den AirPlay-Port 7000 offen hat"),
    r(
        Dienst("_hap._tcp"),
        "Smart Home",
        3,
        "den Bonjour-Dienst HomeKit (_hap._tcp) anbietet",
    ),
    r(
        Dienst("_homekit._tcp"),
        "Smart Home",
        3,
        "den Bonjour-Dienst HomeKit (_homekit._tcp) anbietet",
    ),
    r(
        Dienst("_matter._tcp"),
        "Smart Home",
        3,
        "den Bonjour-Dienst Matter (_matter._tcp) anbietet",
    ),
    r(
        Dienst("_hue._tcp"),
        "Smart Home",
        3,
        "den Bonjour-Dienst Hue (_hue._tcp) anbietet",
    ),
    r(
        Hersteller("espressif"),
        "Smart Home",
        2,
        "von einem Chiphersteller stammt, dessen Chips meist in Steckdosen und Lampen stecken",
    ),
    r(
        Hersteller("signify"),
        "Smart Home",
        2,
        "vom Hersteller der Hue-Lampen stammt",
    ),
    r(
        Hersteller("tuya"),
        "Smart Home",
        2,
        "von einem Hersteller für Smart-Home-Geräte stammt",
    ),
    r(
        Hersteller("sonos"),
        "Medien",
        2,
        "vom Lautsprecherhersteller Sonos stammt",
    ),
    r(
        Hersteller("avm"),
        "Router",
        2,
        "vom Routerhersteller AVM stammt",
    ),
    r(
        Hersteller("ubiquiti"),
        "Router",
        2,
        "von einem Hersteller für Netzwerktechnik stammt",
    ),
    r(
        Hersteller("mikrotik"),
        "Router",
        2,
        "von einem Hersteller für Netzwerktechnik stammt",
    ),
    r(
        Hersteller("tp-link"),
        "Router",
        1,
        "von einem Hersteller für Netzwerktechnik stammt",
    ),
    r(Port(53), "Router", 1, "einen DNS-Server (Port 53) anbietet"),
    r(
        Port(62078),
        "Handy",
        3,
        "den iPhone-Sync-Port 62078 offen hat",
    ),
    r(
        Dienst("_workstation._tcp"),
        "Computer",
        2,
        "sich per Bonjour als Arbeitsplatzrechner meldet",
    ),
    r(
        Dienst("_ssh._tcp"),
        "Computer",
        1,
        "SSH per Bonjour anbietet",
    ),
    r(
        Dienst("_smb._tcp"),
        "Computer",
        1,
        "Dateifreigabe (_smb._tcp) per Bonjour anbietet",
    ),
    r(Port(22), "Computer", 1, "SSH (Port 22) anbietet"),
    r(
        Port(445),
        "Computer",
        1,
        "Dateifreigabe (Port 445) anbietet",
    ),
];

/// The default gateway is a router, whatever else it offers.
const GATEWAY: u8 = 5;

#[derive(Debug, Serialize)]
pub struct Karte {
    #[serde(flatten)]
    pub geraet: Device,
    /// Best name by the order in `namen` (user, mDNS, UPnP, NetBIOS, PTR, maker), else IP.
    pub anzeigename: String,
    /// Where `anzeigename` comes from, e.g. „Bonjour“; empty if it is the IP.
    pub namequelle: String,
    pub typ: String,
    pub symbol: String,
    /// One German sentence: why this type (or why none).
    pub erklaerung: String,
    pub hinweise: Vec<String>,
    /// Das Standardgateway des Netzes: nur dort bietet die App die Router-Seite an (#67).
    pub gateway: bool,
}

fn typ_info(name: &str) -> (&'static str, &'static str, &'static str) {
    TYPEN.iter().find(|t| t.0 == name).copied().unwrap_or((
        UNBEKANNT.0,
        UNBEKANNT.1,
        "ein unbekanntes Gerät",
    ))
}

/// Type, symbol and sentence for one device.
pub fn typ_bestimmen(d: &Device, router: Option<Ipv4Addr>) -> (String, String, String) {
    if let Some(t) = d.von_mia.as_ref().and_then(|m| m.typ.as_deref()) {
        let (n, s, _) = typ_info(t);
        return (
            n.into(),
            s.into(),
            format!(
                "Du hast festgelegt, dass dies ein Gerät vom Typ „{n}“ ist. Neue Läufe ändern das nicht."
            ),
        );
    }
    // (type, points, best single rule, reason of that rule)
    let mut summe: Vec<(&str, u8, u8, &str)> = Vec::new();
    let mut zaehle =
        |typ: &'static str, p: u8, grund: &'static str| match summe.iter_mut().find(|s| s.0 == typ)
        {
            Some(s) => {
                s.1 += p;
                if p > s.2 {
                    (s.2, s.3) = (p, grund);
                }
            }
            None => summe.push((typ, p, p, grund)),
        };
    if router == Some(d.ip) {
        zaehle("Router", GATEWAY, "das Standardgateway deines Netzes ist");
    }
    let hersteller = d.hersteller.as_deref().unwrap_or("").to_lowercase();
    for x in REGELN {
        let trifft = match x.wenn {
            Dienst(t) => d.dienste.iter().any(|s| s.typ == t),
            Hersteller(h) => hersteller.contains(h),
            Port(p) => d.ports.contains(&p),
        };
        if trifft {
            zaehle(x.typ, x.punkte, x.grund);
        }
    }
    let beste = summe.iter().map(|s| s.1).max().unwrap_or(0);
    let vorn: Vec<_> = summe.iter().filter(|s| s.1 == beste).collect();
    match vorn.as_slice() {
        [s] => {
            let (n, sym, akk) = typ_info(s.0);
            (
                n.into(),
                sym.into(),
                format!("Netzinfo hält dieses Gerät für {akk}, weil es {}.", s.3),
            )
        }
        [] => (
            UNBEKANNT.0.into(),
            UNBEKANNT.1.into(),
            "Dieses Gerät kennt Netzinfo noch nicht. Du kannst ihm einen Namen geben.".into(),
        ),
        gleich => (
            UNBEKANNT.0.into(),
            UNBEKANNT.1.into(),
            format!(
                "Die Hinweise passen zu gleich vielen Typen ({}). Netzinfo legt sich nicht fest. Du kannst dem Gerät einen Typ geben.",
                gleich.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")
            ),
        ),
    }
}

/// Short facts, no rating and no number.
pub fn hinweise(d: &Device, jetzt: u64) -> Vec<String> {
    let mut h = Vec::new();
    if d.ports.contains(&23) {
        h.push("bietet Telnet an (Port 23)".to_string());
    }
    if d.mac_zufaellig {
        h.push("nutzt eine zufällige MAC-Adresse".into());
    }
    if d.mac.is_some() && !d.mac_zufaellig && d.hersteller.is_none() {
        h.push("Hersteller steht nicht in der eingebauten Liste".into());
    }
    if !d.ip_verlauf.is_empty() {
        h.push(format!(
            "hatte früher {} andere IP-Adresse(n)",
            d.ip_verlauf.len()
        ));
    }
    if d.besuche <= 1 {
        h.push("wurde erst einmal gesehen".into());
    }
    let tage = jetzt.saturating_sub(d.last_seen) / 86400;
    if tage >= 7 {
        h.push(format!("zuletzt vor {tage} Tagen gesehen"));
    }
    h
}

pub fn karte(d: &Device, router: Option<Ipv4Addr>, jetzt: u64) -> Karte {
    let (typ, symbol, erklaerung) = typ_bestimmen(d, router);
    let (anzeigename, namequelle) = match crate::namen::bester(d) {
        Some(n) => (n.name, n.quelle.bezeichnung().to_string()),
        None => (d.ip.to_string(), String::new()),
    };
    let mut geraet = d.clone();
    for s in &mut geraet.dienste {
        s.txt.clear(); // serial numbers and names stay in the file, not in the view
    }
    geraet.besuche = geraet.besuche.max(1);
    Karte {
        hinweise: hinweise(&geraet, jetzt),
        gateway: router == Some(d.ip),
        geraet,
        anzeigename,
        namequelle,
        typ,
        symbol,
        erklaerung,
    }
}

type Store = BTreeMap<String, Vec<Device>>;

/// All cards of network `key`, last seen first.
pub fn karten(
    path: &Path,
    key: &str,
    router: Option<Ipv4Addr>,
    jetzt: u64,
) -> Result<Vec<Karte>, String> {
    let mut store: Store = netze::load_json(path)?;
    let mut list: Vec<Karte> = store
        .remove(key)
        .unwrap_or_default()
        .iter()
        .map(|d| karte(d, router, jetzt))
        .collect();
    list.sort_by(|a, b| (b.geraet.last_seen, a.geraet.ip).cmp(&(a.geraet.last_seen, b.geraet.ip)));
    Ok(list)
}

/// Sets the user's name and/or type of one device (`schluessel` = MAC or IP). Both empty
/// removes the correction.
pub fn setzen(
    path: &Path,
    key: &str,
    schluessel: &str,
    name: Option<&str>,
    typ: Option<&str>,
) -> Result<(), String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let typ = typ.filter(|t| !t.is_empty());
    if name.is_some_and(|n| n.chars().count() > 60 || n.chars().any(char::is_control)) {
        return Err(
            "Der Name ist zu lang oder enthält Steuerzeichen. Nimm höchstens 60 Zeichen.".into(),
        );
    }
    if let Some(t) = typ
        && !TYPEN.iter().any(|x| x.0 == t)
    {
        let alle: Vec<_> = TYPEN.iter().map(|x| x.0).collect();
        return Err(format!(
            "Den Typ „{t}“ gibt es nicht. Erlaubt: {}.",
            alle.join(", ")
        ));
    }
    let _sperre = crate::sperre(path)?;
    let mut store: Store = netze::load_json(path)?;
    let d = store
        .get_mut(key)
        .and_then(|l| {
            l.iter_mut().find(|d| {
                d.mac.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(schluessel))
                    || d.ip.to_string() == schluessel
            })
        })
        .ok_or("Dieses Gerät ist in diesem Netz nicht gespeichert. Suche zuerst mit `netzinfo geraete --nachbarn`.")?;
    d.von_mia = (name.is_some() || typ.is_some()).then(|| VonMia {
        name: name.map(String::from),
        typ: typ.map(String::from),
    });
    netze::save(path, &store)
}

fn kontext() -> Result<(std::path::PathBuf, netze::Fingerprint), String> {
    let fp = netze::fingerprint(&crate::hier::collect()?)?;
    Ok((netze::data_path()?.with_file_name("geraete.json"), fp))
}

/// Cards of the network we are in now.
pub fn karten_jetzt() -> Result<Vec<Karte>, String> {
    let (path, fp) = kontext()?;
    karten(&path, &fp.key(), Some(fp.router), netze::now())
}

pub fn setzen_jetzt(schluessel: &str, name: Option<&str>, typ: Option<&str>) -> Result<(), String> {
    let (path, fp) = kontext()?;
    setzen(&path, &fp.key(), schluessel, name, typ)
}

/// `netzinfo geraete --karten`: shows what is stored, sends nothing.
pub fn zeigen(explain_on: bool) -> Result<(), String> {
    let e = |t: &str| crate::explain(explain_on, t);
    e(
        "Ich schicke nichts. Ich lese nur, was frühere Läufe von `netzinfo geraete` und `netzinfo bonjour` in diesem Netz gemerkt haben, und lege je Gerät eine Karte an.",
    );
    let liste = karten_jetzt()?;
    if liste.is_empty() {
        crate::satz("Noch keine Geräte gemerkt. Suche zuerst mit `netzinfo geraete --nachbarn`.");
        return Ok(());
    }
    println!("Gerätekarten: {}", liste.len());
    for k in &liste {
        let g = &k.geraet;
        println!(
            "{} {:<10} {}  ({}, {})",
            k.symbol,
            k.typ,
            k.anzeigename,
            g.ip,
            crate::hersteller::anzeige(g.mac.as_deref())
        );
        if !g.dienste.is_empty() {
            let t: Vec<_> = g.dienste.iter().map(|s| s.typ.as_str()).collect();
            println!("     Dienste: {}", t.join(", "));
        }
        if !k.hinweise.is_empty() {
            println!("     Hinweise: {}", k.hinweise.join("; "));
        }
        println!(
            "     {} Besuche, zuletzt gesehen {}",
            g.besuche,
            netze::format_utc(g.last_seen)
        );
        e(&k.erklaerung);
    }
    e(
        "Der Typ ist eine Vermutung aus öffentlichen Ankündigungen des Geräts. Wer will, kann sie fälschen. Geräte, die nichts ankündigen, bleiben „Unbekannt“. In der App kannst du Name und Typ selbst setzen; das überschreibt kein neuer Lauf.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bonjour::Dienst;

    fn dev(ip: &str) -> Device {
        Device {
            ip: ip.parse().unwrap(),
            first_seen: 1,
            last_seen: 1,
            ..Default::default()
        }
    }
    fn mit_dienst(t: &str) -> Device {
        Device {
            dienste: vec![Dienst {
                typ: t.into(),
                ..Default::default()
            }],
            ..dev("192.0.2.9")
        }
    }
    fn typ(d: &Device) -> String {
        typ_bestimmen(d, None).0
    }

    #[test]
    fn each_type_has_an_example() {
        assert_eq!(typ(&mit_dienst("_ipp._tcp")), "Drucker");
        assert_eq!(typ(&mit_dienst("_airplay._tcp")), "Medien");
        assert_eq!(typ(&mit_dienst("_hap._tcp")), "Smart Home");
        assert_eq!(typ(&mit_dienst("_workstation._tcp")), "Computer");
        let handy = Device {
            ports: vec![62078],
            ..dev("192.0.2.10")
        };
        assert_eq!(typ(&handy), "Handy");
        let router = dev("192.0.2.1");
        assert_eq!(typ_bestimmen(&router, Some(router.ip)).0, "Router");
        let avm = Device {
            hersteller: Some("AVM GmbH".into()),
            ..dev("192.0.2.2")
        };
        assert_eq!(typ(&avm), "Router");
    }

    #[test]
    fn tie_and_nothing_are_unknown_with_a_sentence() {
        assert_eq!(typ(&dev("192.0.2.5")), "Unbekannt");
        assert!(
            typ_bestimmen(&dev("192.0.2.5"), None)
                .2
                .contains("kennt Netzinfo noch nicht")
        );
        // Cast (Medien, 2) against a printer port (Drucker, 2): equal, no guess
        let d = Device {
            ports: vec![8009, 631],
            ..dev("192.0.2.6")
        };
        let (t, _, s) = typ_bestimmen(&d, None);
        assert_eq!(t, "Unbekannt");
        assert!(s.contains("Drucker") && s.contains("Medien"));
    }

    #[test]
    fn points_add_up_and_the_reason_names_the_best_rule() {
        let d = Device {
            ports: vec![631],
            ..mit_dienst("_ipp._tcp")
        };
        let (t, _, s) = typ_bestimmen(&d, None);
        assert_eq!(t, "Drucker");
        assert!(s.starts_with(
            "Netzinfo hält dieses Gerät für einen Drucker, weil es den Bonjour-Dienst"
        ));
        // a Mac with SSH, SMB and AirPlay port stays a computer
        let mac = Device {
            ports: vec![22, 445, 7000],
            ..dev("192.0.2.20")
        };
        assert_eq!(typ(&mac), "Computer");
    }

    #[test]
    fn hints_are_facts_only() {
        let d = Device {
            ports: vec![23],
            mac: Some("02:00:5e:00:53:01".into()),
            mac_zufaellig: true,
            besuche: 3,
            last_seen: 1,
            ..dev("192.0.2.7")
        };
        let h = hinweise(&d, 1 + 12 * 86400);
        assert!(h.contains(&"bietet Telnet an (Port 23)".to_string()));
        assert!(h.contains(&"zuletzt vor 12 Tagen gesehen".to_string()));
        let alle = h.join(" ").to_lowercase();
        for wort in [
            "gefahr", "warnung", "angriff", "unsicher", "risiko", "gehackt",
        ] {
            assert!(!alle.contains(wort), "{wort}");
        }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("netz-karte-{}-{name}", std::process::id()))
    }

    #[test]
    fn mias_correction_survives_a_new_scan_and_wins() {
        let dir = tmp("mia");
        let path = dir.join("geraete.json");
        let mut a = dev("192.0.2.9");
        a.mac = Some("00:00:5e:00:53:09".into());
        a.dienste = mit_dienst("_ipp._tcp").dienste;
        let mut store = Store::new();
        store.insert("k".into(), vec![a.clone()]);
        netze::save(&path, &store).unwrap();
        // by MAC (any case) or by IP
        setzen(
            &path,
            "k",
            "00:00:5E:00:53:09",
            Some("  Flur-Drucker "),
            Some("Computer"),
        )
        .unwrap();
        // a new scan finds the same device (other IP, fresh data) and merges it into the store
        let mut store: Store = netze::load_json(&path).unwrap();
        let mut neu = a.clone();
        neu.ip = "192.0.2.19".parse().unwrap();
        neu.last_seen = 99;
        crate::geraete::merge(
            store.get_mut("k").unwrap(),
            vec![neu],
            "192.0.2.0".parse().unwrap(),
            "192.0.2.255".parse().unwrap(),
        );
        netze::save(&path, &store).unwrap();
        let k = karten(&path, "k", None, 100).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(k.len(), 1);
        assert_eq!(
            (k[0].anzeigename.as_str(), k[0].typ.as_str()),
            ("Flur-Drucker", "Computer")
        );
        assert_eq!(k[0].geraet.besuche, 2);
        assert!(k[0].erklaerung.starts_with("Du hast festgelegt"));
    }

    #[test]
    fn setzen_checks_input_and_can_clear() {
        let dir = tmp("setzen");
        let path = dir.join("geraete.json");
        let mut store = Store::new();
        store.insert("k".into(), vec![dev("192.0.2.9")]);
        netze::save(&path, &store).unwrap();
        let fehler = |n, t| setzen(&path, "k", "192.0.2.9", n, t).unwrap_err();
        assert!(fehler(None, Some("Toaster")).contains("Erlaubt"));
        assert!(fehler(Some("a\nb"), None).contains("Steuerzeichen"));
        assert!(
            setzen(&path, "k", "192.0.2.99", Some("x"), None)
                .unwrap_err()
                .contains("nicht gespeichert")
        );
        assert!(setzen(&path, "andere", "192.0.2.9", Some("x"), None).is_err());
        setzen(&path, "k", "192.0.2.9", Some("Nas"), None).unwrap();
        let s: Store = netze::load_json(&path).unwrap();
        assert_eq!(
            s["k"][0].von_mia.as_ref().unwrap().name.as_deref(),
            Some("Nas")
        );
        setzen(&path, "k", "192.0.2.9", Some(" "), Some("")).unwrap();
        let s: Store = netze::load_json(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(s["k"][0].von_mia, None);
    }

    #[test]
    fn old_file_loads_and_survives_a_save_without_loss() {
        let dir = tmp("alt");
        let path = dir.join("geraete.json");
        std::fs::create_dir_all(&dir).unwrap();
        let alt = r#"{"k":[{"ip":"192.0.2.2","name":"drucker.example","ports":[631],"first_seen":1,"last_seen":2,"mac":"00:00:5e:00:53:02","ip_verlauf":["192.0.2.3"],"ipv6":["2001:db8::2"],"hersteller":null,"mac_zufaellig":false,"dienste":[],"mdns_name":null}]}"#;
        std::fs::write(&path, alt).unwrap();
        let vorher: Store = netze::load_json(&path).unwrap();
        assert_eq!(
            (vorher["k"][0].besuche, vorher["k"][0].von_mia.clone()),
            (0, None)
        );
        netze::save(&path, &vorher).unwrap();
        let nachher: Store = netze::load_json(&path).unwrap();
        let datei = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(vorher, nachher);
        // every old field is still in the file with its value
        let j: serde_json::Value = serde_json::from_str(&datei).unwrap();
        let a: serde_json::Value = serde_json::from_str(alt).unwrap();
        for (feld, wert) in a["k"][0].as_object().unwrap() {
            assert_eq!(&j["k"][0][feld], wert, "{feld}");
        }
        // an old card counts as seen once
        assert_eq!(karte(&nachher["k"][0], None, 2).geraet.besuche, 1);
    }

    #[test]
    fn card_view_drops_txt_values() {
        let mut d = mit_dienst("_ipp._tcp");
        d.dienste[0].txt.insert("sn".into(), "SERIAL".into());
        let k = karte(&d, None, 1);
        assert!(!serde_json::to_string(&k).unwrap().contains("SERIAL"));
        assert_eq!(k.anzeigename, "192.0.2.9");
    }
}
