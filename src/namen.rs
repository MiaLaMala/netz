//! Gerätenamen aus mehreren Quellen mit fester Rangfolge. Issue #43.
//!
//! Rangfolge (die Reihenfolge der Varianten in `Quelle` ist sie, nichts sonst):
//! Name von Mia (gilt immer), mDNS-Hostname, UPnP, NetBIOS, PTR (Router-DNS), zuletzt
//! Hersteller plus Ende der MAC.
//!
//! Gespeichert wird nur, was sonst nirgends liegt (`Device::namen`: NetBIOS, später UPnP);
//! die anderen Namen stehen schon in `von_mia`, `mdns_name` und `name` und werden nur gelesen.

use crate::geraete::Device;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quelle {
    Mia,
    Mdns,
    /// Noch ohne Erzeuger: #42 liefert den `friendlyName`, Jana verbindet beides.
    Upnp,
    NetBios,
    Ptr,
    Hersteller,
}

impl Quelle {
    pub fn bezeichnung(self) -> &'static str {
        match self {
            Quelle::Mia => "von dir",
            Quelle::Mdns => "Bonjour",
            Quelle::Upnp => "UPnP",
            Quelle::NetBios => "NetBIOS",
            Quelle::Ptr => "DNS (PTR)",
            Quelle::Hersteller => "Hersteller",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Name {
    pub quelle: Quelle,
    pub name: String,
    /// Unix-Sekunden, wann die Quelle den Namen zuletzt geliefert hat.
    pub zuletzt: u64,
}

/// Legt `neu` ab; eine Quelle hat höchstens einen Namen, der neuere ersetzt den älteren.
pub fn eintragen(liste: &mut Vec<Name>, neu: Name) {
    match liste.iter_mut().find(|n| n.quelle == neu.quelle) {
        Some(alt) => *alt = neu,
        None => liste.push(neu),
    }
}

/// Alle Namen des Geräts, bester zuerst.
pub fn kandidaten(d: &Device) -> Vec<Name> {
    let mut v: Vec<Name> = d.namen.clone();
    let mut dazu = |quelle, name: Option<&str>| {
        if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
            v.push(Name {
                quelle,
                name: n.into(),
                zuletzt: d.last_seen,
            });
        }
    };
    dazu(
        Quelle::Mia,
        d.von_mia.as_ref().and_then(|m| m.name.as_deref()),
    );
    dazu(Quelle::Mdns, d.mdns_name.as_deref());
    dazu(Quelle::Ptr, d.name.as_deref());
    if let Some(mac) = d.mac.as_deref().and_then(crate::hersteller::parse_mac) {
        let maker = crate::hersteller::hersteller(mac).name;
        dazu(
            Quelle::Hersteller,
            Some(&format!(
                "{} {:02x}:{:02x}:{:02x}",
                maker.as_deref().unwrap_or("Gerät"),
                mac[3],
                mac[4],
                mac[5]
            )),
        );
    }
    v.sort_by_key(|n| n.quelle);
    v
}

pub fn bester(d: &Device) -> Option<Name> {
    kandidaten(d).into_iter().next()
}

// ---- NetBIOS-Knotenstatus (UDP 137) ----

const NB_TIMEOUT: Duration = Duration::from_secs(1);
const NB_ID: u16 = 0x4e42;

/// "Node Status Request" für den Platzhalter-Namen `*`: Kopf, 32 Byte kodierter Name, NBSTAT, IN.
pub fn nbstat_anfrage() -> Vec<u8> {
    let mut p = vec![
        (NB_ID >> 8) as u8,
        NB_ID as u8,
        0,
        0,
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0x20,
    ];
    // `*` und 15 Nullbytes, je Halbbyte ein Buchstabe ab 'A': 0x2A -> "CK", 0x00 -> "AA"
    p.extend(b"CK");
    p.extend(std::iter::repeat_n(b'A', 30));
    p.extend([0, 0, 0x21, 0, 1]);
    p
}

/// Der erste Workstation-Name (Suffix 0x00, kein Gruppenname) aus einer NBSTAT-Antwort.
/// Abgeschnittene oder fremde Pakete geben einen Fehler, nie einen halben Namen.
pub fn nbstat_parsen(b: &[u8]) -> Result<Option<String>, String> {
    let kaputt = || "Eine NetBIOS-Antwort ist abgeschnitten oder kaputt und wird ignoriert.";
    if b.len() < 13 || b[..2] != NB_ID.to_be_bytes() || b[2] & 0x80 == 0 {
        return Err(kaputt().into());
    }
    // Antwortname: voll (0x20 + 32 + 0) oder als Zeiger; danach Typ, Klasse, TTL, Länge
    let nach_name = match b[12] {
        0x20 => 12 + 34,
        l if l & 0xC0 == 0xC0 => 12 + 2,
        _ => return Err(kaputt().into()),
    };
    if b.get(nach_name..nach_name + 2) != Some(&[0, 0x21]) {
        return Err(kaputt().into());
    }
    let rd = nach_name + 10;
    let anzahl = *b.get(rd).ok_or_else(kaputt)? as usize;
    let tabelle = b.get(rd + 1..rd + 1 + anzahl * 18).ok_or_else(kaputt)?;
    Ok(tabelle.chunks(18).find_map(|e| {
        let gruppe = e[16] & 0x80 != 0;
        let name: String = String::from_utf8_lossy(&e[..15])
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        (e[15] == 0 && !gruppe)
            .then(|| name.trim().to_string())
            .filter(|n| !n.is_empty())
    }))
}

fn nbstat_fragen_eine(ip: Ipv4Addr) -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(NB_TIMEOUT)).ok()?;
    s.send_to(&nbstat_anfrage(), SocketAddr::from((ip, 137)))
        .ok()?;
    let mut buf = [0u8; 1024];
    let (n, von) = s.recv_from(&mut buf).ok()?;
    if von.ip() != ip {
        return None;
    }
    nbstat_parsen(&buf[..n]).ok().flatten()
}

/// Fragt `ips` parallel nach ihrem NetBIOS-Namen. Ohne „meins“ wird nichts gesendet.
/// Die Adressen sollen aus der Nachbartabelle stammen (das prüft der Aufrufer in `geraete`).
pub fn netbios_fragen(meins: bool, ips: &[Ipv4Addr]) -> Vec<Option<String>> {
    fragen_mit(meins, ips, nbstat_fragen_eine)
}

fn fragen_mit(
    meins: bool,
    ips: &[Ipv4Addr],
    f: impl Fn(Ipv4Addr) -> Option<String> + Sync,
) -> Vec<Option<String>> {
    if !meins {
        return vec![None; ips.len()];
    }
    crate::geraete::par(ips.len(), |i| f(ips[i]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geraetekarte::VonMia;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Antwort eines Windows-Rechners, von Hand nach RFC 1002 gebaut: Namenstabelle wie
    /// übergeben, danach 46 Byte Statistik (MAC zuerst). Kein Mitschnitt aus einem echten Netz.
    fn antwort(eintraege: &[(&str, u8, u16)]) -> Vec<u8> {
        let mut p = vec![0x4e, 0x42, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0x20];
        p.extend(b"CKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        p.extend([0, 0, 0x21, 0, 1, 0, 0, 0, 0]);
        let rdlen = 1 + eintraege.len() * 18 + 46;
        p.extend((rdlen as u16).to_be_bytes());
        p.push(eintraege.len() as u8);
        for (n, suffix, flags) in eintraege {
            p.extend(format!("{n:<15}").bytes());
            p.push(*suffix);
            p.extend(flags.to_be_bytes());
        }
        p.extend([0, 0, 0x5e, 0, 0x53, 0x07]);
        p.extend([0u8; 40]);
        p
    }

    #[test]
    fn anfrage_ist_der_feste_knotenstatus() {
        let a = nbstat_anfrage();
        assert_eq!(a.len(), 50);
        assert_eq!(&a[12..46], b"\x20CKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\x00");
        assert_eq!(&a[45..], [0, 0, 0x21, 0, 1]);
    }

    #[test]
    fn nimmt_nur_den_workstation_namen() {
        let p = antwort(&[
            ("WORKGROUP", 0, 0x8400),
            ("RECHNER-A", 0x20, 0x0400),
            ("RECHNER-A", 0, 0x0400),
        ]);
        assert_eq!(nbstat_parsen(&p), Ok(Some("RECHNER-A".into())));
        // nur Gruppe und Dienst: kein Name
        let p = antwort(&[("WORKGROUP", 0, 0x8400), ("RECHNER-A", 0x20, 0x0400)]);
        assert_eq!(nbstat_parsen(&p), Ok(None));
    }

    #[test]
    fn abgeschnittene_und_fremde_pakete_sind_fehler() {
        let p = antwort(&[("RECHNER-A", 0, 0x0400)]);
        let ende = 12 + 34 + 10 + 1 + 18; // Ende der einen Tabellenzeile
        for n in 0..ende {
            assert!(nbstat_parsen(&p[..n]).is_err(), "Länge {n}");
        }
        assert!(nbstat_parsen(&p[..ende]).is_ok());
        let mut fremd = p.clone();
        fremd[0] = 0; // andere Transaktionsnummer
        assert!(nbstat_parsen(&fremd).is_err());
        let mut anfrage = p;
        anfrage[2] = 0; // Antwort-Bit fehlt
        assert!(nbstat_parsen(&anfrage).is_err());
    }

    #[test]
    fn ohne_meins_geht_kein_paket_raus() {
        let gesendet = AtomicUsize::new(0);
        let f = |_| {
            gesendet.fetch_add(1, Ordering::Relaxed);
            Some("x".to_string())
        };
        let ips = ["192.0.2.7".parse().unwrap(), "192.0.2.8".parse().unwrap()];
        assert_eq!(fragen_mit(false, &ips, f), vec![None, None]);
        assert_eq!(gesendet.load(Ordering::Relaxed), 0);
        assert_eq!(fragen_mit(true, &ips, f).len(), 2);
        assert_eq!(gesendet.load(Ordering::Relaxed), 2);
    }

    fn geraet(maske: u32) -> Device {
        let n = |q, s: &str| Name {
            quelle: q,
            name: s.into(),
            zuletzt: 5,
        };
        let mut d = Device {
            mac: Some("00:00:5e:00:53:07".into()),
            ..Default::default()
        };
        if maske & 1 != 0 {
            d.von_mia = Some(VonMia {
                name: Some("Mias Name".into()),
                typ: None,
            });
        }
        if maske & 2 != 0 {
            d.mdns_name = Some("mdns-name".into());
        }
        if maske & 4 != 0 {
            d.namen.push(n(Quelle::Upnp, "upnp-name"));
        }
        if maske & 8 != 0 {
            d.namen.push(n(Quelle::NetBios, "NB-NAME"));
        }
        if maske & 16 != 0 {
            d.name = Some("ptr.example".into());
        }
        d
    }

    #[test]
    fn rangfolge_bei_allen_kombinationen_fehlender_quellen() {
        let reihe = [
            (Quelle::Mia, "Mias Name"),
            (Quelle::Mdns, "mdns-name"),
            (Quelle::Upnp, "upnp-name"),
            (Quelle::NetBios, "NB-NAME"),
            (Quelle::Ptr, "ptr.example"),
        ];
        for maske in 0..32u32 {
            let b = bester(&geraet(maske)).unwrap();
            let erwartet = match reihe
                .iter()
                .enumerate()
                .find(|(i, _)| maske & (1 << i) != 0)
            {
                Some((_, (q, n))) => (*q, n.to_string()),
                None => (Quelle::Hersteller, "ICANN, IANA Department 00:53:07".into()),
            };
            assert_eq!((b.quelle, b.name), erwartet, "Maske {maske:05b}");
        }
        // ohne MAC und ohne Namen: nichts
        assert_eq!(bester(&Device::default()), None);
    }

    #[test]
    fn eine_quelle_ein_name() {
        let mut l = vec![];
        let n = |s: &str, t| Name {
            quelle: Quelle::NetBios,
            name: s.into(),
            zuletzt: t,
        };
        eintragen(&mut l, n("alt", 1));
        eintragen(&mut l, n("neu", 2));
        assert_eq!(l, vec![n("neu", 2)]);
    }
}
