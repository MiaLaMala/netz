//! Manufacturer from the MAC address and detection of random addresses, see issue #27.
//!
//! The IEEE registry (MA-L 24 bit, MA-M 28 bit, MA-S 36 bit) is embedded from `data/oui.tsv`,
//! built by `python3 scripts/oui.py`. No download at run time; the list is as old as the release.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const TABLE: &str = include_str!("../data/oui.tsv");

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HerstellerInfo {
    /// Registered organisation; `None` for random/group addresses and unknown prefixes.
    pub name: Option<String>,
    /// Locally administered bit set: the device made the address up, it says nothing about the maker.
    pub zufaellig: bool,
    /// Group (multicast) bit set: not the address of one device.
    pub gruppe: bool,
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    T.get_or_init(|| TABLE.lines().filter_map(|l| l.split_once('\t')).collect())
}

/// Parses `aa:bb:cc:dd:ee:ff` (also `-` as separator).
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = s.split([':', '-']);
    for b in &mut out {
        let p = parts.next().filter(|p| p.len() == 2)?;
        *b = u8::from_str_radix(p, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

pub fn hersteller(mac: [u8; 6]) -> HerstellerInfo {
    let zufaellig = mac[0] & 0x02 != 0;
    let gruppe = mac[0] & 0x01 != 0;
    let mut name = None;
    if !zufaellig && !gruppe {
        let hex: String = mac.iter().map(|b| format!("{b:02X}")).collect();
        // longest assignment wins: MA-S (9 hex digits), MA-M (7), MA-L (6)
        name = [9, 7, 6]
            .iter()
            .find_map(|n| table().get(&hex[..*n]))
            .map(|s| s.to_string());
    }
    HerstellerInfo {
        name,
        zufaellig,
        gruppe,
    }
}

/// Like `hersteller` for the text form; `None` if `mac` is not a MAC address.
pub fn fuer_text(mac: &str) -> Option<HerstellerInfo> {
    parse_mac(mac).map(hersteller)
}

/// One short label for lists: the maker, „zufällige Adresse“ or „-“.
pub fn anzeige(mac: Option<&str>) -> String {
    match mac.and_then(fuer_text) {
        Some(i) if i.zufaellig => "zufällige Adresse".into(),
        Some(i) => i.name.unwrap_or_else(|| "unbekannt".into()),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_maker_ma_l() {
        // 00:00:5e is the IANA block, the one RFC 7042 uses for documentation
        let i = hersteller([0x00, 0x00, 0x5e, 0x00, 0x53, 0x01]);
        assert_eq!(i.name.as_deref(), Some("ICANN, IANA Department"));
        assert!(!i.zufaellig && !i.gruppe);
        let avm = hersteller([0x38, 0x10, 0xd5, 0, 0, 1]);
        assert!(avm.name.unwrap().starts_with("AVM"));
    }

    #[test]
    fn ma_s_beats_ma_l() {
        // 8C:1F:64:AF:A is a 36 bit block inside the MA-L block 8C:1F:64
        let s = hersteller([0x8c, 0x1f, 0x64, 0xaf, 0xa0, 0x01]);
        assert_eq!(s.name.as_deref(), Some("DATA ELECTRONIC DEVICES, INC"));
        let l = hersteller([0x8c, 0x1f, 0x64, 0x00, 0x00, 0x01]);
        assert_ne!(l.name, s.name);
    }

    #[test]
    fn locally_administered_is_random_without_name() {
        let i = hersteller([0x02, 0x00, 0x5e, 0x00, 0x53, 0x01]);
        assert_eq!((i.name, i.zufaellig, i.gruppe), (None, true, false));
    }

    #[test]
    fn multicast_bit_is_a_group() {
        let i = hersteller([0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb]);
        assert_eq!((i.name, i.zufaellig, i.gruppe), (None, false, true));
    }

    #[test]
    fn unknown_prefix_has_no_name() {
        let i = hersteller([0xfc, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert!(i.name.is_none() && !i.zufaellig);
    }

    #[test]
    fn parses_text_form() {
        assert_eq!(
            parse_mac("00:00:5e:00:53:af"),
            Some([0, 0, 0x5e, 0, 0x53, 0xaf])
        );
        assert_eq!(parse_mac("00:00:5e:00:53"), None);
        assert_eq!(parse_mac("00:00:5e:00:53:af:01"), None);
        assert_eq!(parse_mac("zz:00:5e:00:53:af"), None);
        assert_eq!(anzeige(Some("02:00:5e:00:53:01")), "zufällige Adresse");
        assert_eq!(anzeige(None), "-");
    }
}
