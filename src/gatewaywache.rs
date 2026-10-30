//! Gateway-Wache: MAC des Routers pro Netz merken, Wechsel und doppelte IP melden. Issue #44.
//!
//! Liest nur die Nachbartabelle (`nachbarn::lesen`), sendet nichts. Die Router-IP steht im
//! Netzschlüssel (`Fingerprint::key()`: Router|DHCP-Server|Domain|Subnetz), also sind „gleiche
//! Gateway-IP, gleicher DHCP-Server, gleiche Domain“ beim selben Schlüssel immer gegeben.
//! Gespeichert wird in `gateway.json` neben netze.json, nicht im Fingerabdruck.

use crate::nachbarn::Nachbar;
use crate::pruefen::Klasse;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::Path;

/// Pro Netzschlüssel die zuletzt gesehene Router-MAC (klein geschrieben, `aa:bb:cc:dd:ee:ff`).
type Speicher = BTreeMap<String, Eintrag>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Eintrag {
    mac: String,
}

/// Die ersten drei Bytes einer MAC: der Hersteller-Präfix.
fn oui(mac: &str) -> &str {
    mac.get(..8).unwrap_or(mac)
}

/// Reine Regeln. `gespeichert` ist die MAC vom letzten Besuch. Gibt die Befunde und die MAC
/// zurück, die künftig als Router gilt (`None`: Speicher unverändert lassen).
fn bewerten(
    gespeichert: Option<&str>,
    router: Ipv4Addr,
    tabelle: &[Nachbar],
    meins: bool,
) -> (Vec<(Klasse, String)>, Option<String>) {
    let mut out = Vec::new();
    // Doppelte IP: dieselbe Adresse bei zwei MACs.
    let mut gesehen: BTreeMap<Ipv4Addr, &str> = BTreeMap::new();
    for n in tabelle {
        for ip in &n.ipv4 {
            if let Some(andere) = gesehen.insert(*ip, &n.mac).filter(|m| *m != n.mac) {
                out.push((
                    Klasse::Auffaellig,
                    format!(
                        "Die Adresse {ip} gehört in der Nachbartabelle zu zwei Geräten ({andere} und {}). Das passiert bei ARP-Spoofing oder wenn ein zweiter Router oder ein Repeater dieselbe Adresse nutzt. Prüfe in `netzinfo geraete`, wer die Adresse hat.",
                        n.mac
                    ),
                ));
            }
        }
    }
    // Router nicht in der Tabelle: nicht messbar, kein Befund.
    let Some(neu) = tabelle.iter().find(|n| n.ipv4.contains(&router)) else {
        return (out, None);
    };
    let Some(alt) = gespeichert else {
        return (out, Some(neu.mac.clone()));
    };
    if alt == neu.mac {
        return (out, None);
    }
    // Alarm nur bei neuem Hersteller-Präfix, und nur in einem „meins“-Netz.
    let alarm = meins && oui(alt) != oui(&neu.mac);
    let (klasse, hinweis) = if alarm {
        (
            Klasse::Alarm,
            "Der Hersteller hat gewechselt, das passt selten zu einem Routertausch. Traue diesem Netz nicht, bis du den Router geprüft hast.",
        )
    } else {
        (
            Klasse::Auffaellig,
            "Das ist harmlos, wenn du den Router getauscht hast oder ein Repeater antwortet. Sonst traue dem Netz nicht.",
        )
    };
    out.push((
        klasse,
        format!(
            "Die Adresse deines Routers ({router}) gehört jetzt zu einem anderen Gerät ({}) als beim letzten Besuch ({alt}). {hinweis}",
            neu.mac
        ),
    ));
    // Bei Alarm bleibt die alte MAC Maßstab, damit der nächste Besuch wieder warnt.
    (out, (!alarm).then(|| neu.mac.clone()))
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Ohne lesbare Tabelle oder ohne Router darin gibt es keinen Befund.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let Some(router) = key.split('|').next().and_then(|r| r.parse().ok()) else {
        return Vec::new();
    };
    // ponytail: hier::collect läuft zum zweiten Mal (pruefen kennt die Schnittstelle schon); billig.
    let Ok(info) = crate::hier::collect() else {
        return Vec::new();
    };
    let Ok(tabelle) = crate::nachbarn::lesen(&info.interface) else {
        return Vec::new();
    };
    let pfad = daten.with_file_name("gateway.json");
    let _sperre = crate::netze::sperre(&pfad);
    let Ok(mut speicher) = crate::netze::load_json::<Speicher>(&pfad) else {
        return Vec::new();
    };
    let alt = speicher.get(key).map(|e| e.mac.clone());
    let (befunde, merken) = bewerten(alt.as_deref(), router, &tabelle, meins);
    if let Some(mac) = merken.filter(|m| Some(m) != alt.as_ref()) {
        speicher.insert(key.to_string(), Eintrag { mac });
        // Ein Speicherfehler lässt den Befund trotzdem stehen.
        let _ = crate::netze::save(&pfad, &speicher);
    }
    befunde
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
    const A: &str = "00:00:5e:00:53:01";
    const B_GLEICHER_HERSTELLER: &str = "00:00:5e:00:53:02";
    const C_ANDERER_HERSTELLER: &str = "02:00:5e:00:53:03";

    fn nb(mac: &str, ips: &[[u8; 4]]) -> Nachbar {
        Nachbar {
            mac: mac.into(),
            ipv4: ips.iter().map(|a| Ipv4Addr::from(*a)).collect(),
            ipv6: vec![],
            schnittstelle: "en0".into(),
        }
    }

    #[test]
    fn erster_besuch_merkt_ohne_befund() {
        let (b, m) = bewerten(None, R, &[nb(A, &[[192, 0, 2, 1]])], true);
        assert!(b.is_empty());
        assert_eq!(m.as_deref(), Some(A));
    }

    #[test]
    fn gleiche_mac_ist_still() {
        let (b, m) = bewerten(Some(A), R, &[nb(A, &[[192, 0, 2, 1]])], true);
        assert!(b.is_empty() && m.is_none());
    }

    #[test]
    fn andere_mac_gleicher_hersteller_ist_auffaellig() {
        let t = [nb(B_GLEICHER_HERSTELLER, &[[192, 0, 2, 1]])];
        let (b, m) = bewerten(Some(A), R, &t, true);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].0, Klasse::Auffaellig);
        assert_eq!(m.as_deref(), Some(B_GLEICHER_HERSTELLER));
    }

    #[test]
    fn hersteller_wechsel_ist_alarm_nur_bei_meins() {
        let t = [nb(C_ANDERER_HERSTELLER, &[[192, 0, 2, 1]])];
        let (b, m) = bewerten(Some(A), R, &t, true);
        assert_eq!(b[0].0, Klasse::Alarm);
        assert!(m.is_none(), "bei Alarm bleibt die alte MAC");
        let (b, m) = bewerten(Some(A), R, &t, false);
        assert_eq!(b[0].0, Klasse::Auffaellig);
        assert_eq!(m.as_deref(), Some(C_ANDERER_HERSTELLER));
    }

    #[test]
    fn doppelte_ip_ist_auffaellig() {
        let t = [
            nb(A, &[[192, 0, 2, 1]]),
            nb(C_ANDERER_HERSTELLER, &[[192, 0, 2, 1], [192, 0, 2, 9]]),
        ];
        let (b, _) = bewerten(Some(A), R, &t, true);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].0, Klasse::Auffaellig);
        assert!(b[0].1.contains("192.0.2.1"));
    }

    #[test]
    fn router_fehlt_in_tabelle_ist_kein_befund() {
        let (b, m) = bewerten(Some(A), R, &[nb(A, &[[192, 0, 2, 7]])], true);
        assert!(b.is_empty() && m.is_none());
        let (b, m) = bewerten(Some(A), R, &[], true);
        assert!(b.is_empty() && m.is_none());
    }

    #[test]
    fn alter_speicher_ohne_datei_laedt() {
        let p = std::env::temp_dir().join("gatewaywache-gibt-es-nicht.json");
        let s: Speicher = crate::netze::load_json(&p).unwrap();
        assert!(s.is_empty());
    }
}
