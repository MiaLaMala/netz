//! Pfad-Gedächtnis: Wegänderungen pro Netz erkennen. Issue #66.
//! Per target a short path run; the public hops are cut to their /24 (IPv4) or /48 (IPv6),
//! hashed, and compared with the last visits. Only the hash and the cut networks are stored.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ring::digest;
use serde::{Deserialize, Serialize};

use crate::netze;
use crate::pfad::{self, Hop};
use crate::pruefen::Klasse;

const ZIELE: [&str; 2] = ["1.1.1.1", "9.9.9.9"];
const RUNDEN: u8 = 3;
const MAX_HOPS: u8 = 12;
/// A hop counts when at least this many of the rounds were answered.
const MIN_ANTWORTEN: u32 = 2;
/// Entries kept per target and network.
const BEHALTEN: usize = 5;

/// Set by `netzinfo pruefen --pfad`: measure although the network is not „meins“.
static ERZWUNGEN: AtomicBool = AtomicBool::new(false);

pub fn erzwingen() {
    ERZWUNGEN.store(true, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Eintrag {
    ziel: String,
    /// SHA-256 (hex) of the cut public hop networks.
    fingerabdruck: String,
    hops_zahl: u32,
    /// Cut networks of the public hops, in order, for the explanation. Never whole addresses.
    netze: Vec<String>,
    /// Answering private hops in front of the first public hop.
    privat: u32,
    zeitpunkt: u64,
}

type Store = BTreeMap<String, Vec<Eintrag>>;

fn store_path(daten: &Path) -> PathBuf {
    daten.with_file_name("pfadspur.json")
}

fn netz_von(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            format!("{}/48", Ipv6Addr::new(s[0], s[1], s[2], 0, 0, 0, 0, 0))
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Boils a measured path down to what is stored.
fn eintrag(ziel: &str, hops: &[Hop], zeitpunkt: u64) -> Eintrag {
    let mut hops: Vec<&Hop> = hops
        .iter()
        .filter(|h| !h.adressen.is_empty() && h.beantwortet >= MIN_ANTWORTEN)
        .collect();
    hops.sort_by_key(|h| h.ttl);
    let privat = hops.iter().take_while(|h| h.lokal).count() as u32;
    let mut netze = Vec::new();
    for h in hops.iter().filter(|h| !h.lokal) {
        // ponytail: per hop only the smallest network counts, so load balancing over several
        // addresses (and the order of replies) does not change the hash; upgrade: compare sets.
        if let Some(n) = h.adressen.iter().map(|a| netz_von(*a)).min() {
            netze.push(n);
        }
    }
    Eintrag {
        ziel: ziel.to_string(),
        fingerabdruck: hex(digest::digest(&digest::SHA256, netze.join(">").as_bytes()).as_ref()),
        hops_zahl: hops.len() as u32,
        netze,
        privat,
        zeitpunkt,
    }
}

/// Judges the newest entry (last of `hist`, all one target) against the visits before.
fn bewerten(hist: &[Eintrag]) -> Option<(Klasse, String)> {
    let (neu, rest) = hist.split_last()?;
    let ziel = &neu.ziel;
    if neu.netze.len() < 2 {
        return Some((
            Klasse::Harmlos,
            format!(
                "Der Weg zu {ziel} ist nicht beurteilbar: Es antworteten weniger als 2 öffentliche Stationen."
            ),
        ));
    }
    let alt = rest.last().filter(|a| a.netze.len() >= 2)?;
    // an extra private hop counts only when it shows up twice in a row
    if let [.., vor, a] = rest
        && neu.privat > vor.privat
        && a.privat > vor.privat
    {
        return Some((
            Klasse::Alarm,
            format!(
                "Zwischen Router und Anbieter steht auf dem Weg zu {ziel} jetzt ein zusätzliches Gerät. Das kann ein neuer Tunnel oder ein zweiter Router sein, oder jemand leitet deinen Verkehr um."
            ),
        ));
    }
    if neu.fingerabdruck == alt.fingerabdruck {
        return Some((
            Klasse::Harmlos,
            format!(
                "Der Weg zu {ziel} führt über dieselben Stationen wie bei deinem letzten Besuch."
            ),
        ));
    }
    if neu.netze[0] != alt.netze[0] || neu.hops_zahl.abs_diff(alt.hops_zahl) >= 3 {
        return Some((
            Klasse::Auffaellig,
            format!(
                "Der Weg zu {ziel} hat sich geändert: {} statt {} Stationen, erste öffentliche Station jetzt {} statt {}. Ein anderer Anschluss oder ein Umweg kann das erklären.",
                neu.hops_zahl, alt.hops_zahl, neu.netze[0], alt.netze[0]
            ),
        ));
    }
    Some((
        Klasse::Harmlos,
        format!(
            "Der Weg zu {ziel} weicht leicht vom letzten Besuch ab. Lastverteilung oder Wartung beim Anbieter erklären das meist."
        ),
    ))
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Sends path probes (3 rounds, 12 hops, two targets) only in a „meins“ network or with `--pfad`.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    if !meins && !ERZWUNGEN.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let jetzt = netze::now();
    let neu: Vec<Eintrag> = std::thread::scope(|s| {
        let jobs: Vec<_> = ZIELE
            .iter()
            .map(|z| s.spawn(move || pfad::pfad_daten(z, MAX_HOPS, RUNDEN, false)))
            .collect();
        ZIELE
            .iter()
            .zip(jobs)
            .filter_map(|(z, j)| Some(eintrag(z, &j.join().ok()?.ok()?.hops, jetzt)))
            .collect()
    });
    let _sperre = netze::sperre(&store_path(daten));
    let Ok(mut store) = netze::load_json::<Store>(&store_path(daten)) else {
        return Vec::new();
    };
    let liste = store.entry(key.to_string()).or_default();
    let mut befunde = Vec::new();
    for e in neu {
        liste.push(e.clone());
        let mut hist: Vec<Eintrag> = liste.iter().filter(|x| x.ziel == e.ziel).cloned().collect();
        befunde.extend(bewerten(&hist));
        hist.drain(..hist.len().saturating_sub(BEHALTEN));
        liste.retain(|x| x.ziel != e.ziel);
        liste.extend(hist);
    }
    // Ein Speicherfehler lässt die Befunde stehen; nur der Verlauf für den nächsten Besuch fehlt.
    let _ = netze::save(&store_path(daten), &store);
    befunde
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hop(ttl: u8, adressen: &[&str], beantwortet: u32) -> Hop {
        let adressen: Vec<IpAddr> = adressen.iter().map(|a| a.parse().unwrap()).collect();
        let lokal = adressen.first().is_some_and(|a| match a {
            IpAddr::V4(v) => v.is_private(),
            IpAddr::V6(_) => false,
        });
        Hop {
            ttl,
            adressen,
            name: None,
            gesendet: 3,
            beantwortet,
            verlust_prozent: 0.0,
            min_ms: None,
            median_ms: None,
            p95_ms: None,
            jitter_ms: None,
            lokal,
            erster_oeffentlicher: false,
            ziel: false,
        }
    }

    fn weg(adr: &[&str]) -> Vec<Hop> {
        adr.iter()
            .enumerate()
            .map(|(i, a)| hop(i as u8 + 1, &[a], 3))
            .collect()
    }

    const BASIS: [&str; 4] = ["192.168.1.1", "203.0.113.1", "198.51.100.7", "192.0.2.9"];

    #[test]
    fn cuts_to_24_and_48() {
        assert_eq!(netz_von("203.0.113.77".parse().unwrap()), "203.0.113.0/24");
        assert_eq!(
            netz_von("2001:db8:1:2:3::9".parse().unwrap()),
            "2001:db8:1::/48"
        );
    }

    #[test]
    fn hash_ignores_reply_order_and_host_part() {
        let a = vec![
            hop(1, &["203.0.113.1", "198.51.100.2"], 3),
            hop(2, &["192.0.2.9"], 3),
        ];
        let b = vec![
            hop(2, &["192.0.2.1"], 3),
            hop(1, &["198.51.100.99", "203.0.113.5"], 3),
        ];
        assert_eq!(
            eintrag("z", &a, 0).fingerabdruck,
            eintrag("z", &b, 0).fingerabdruck
        );
        let c = weg(&["203.0.113.1", "198.51.100.2"]);
        assert_ne!(
            eintrag("z", &a, 0).fingerabdruck,
            eintrag("z", &c, 0).fingerabdruck
        );
    }

    #[test]
    fn store_has_no_whole_addresses() {
        let e = eintrag("1.1.1.1", &weg(&BASIS), 0);
        let json = serde_json::to_string(&e).unwrap();
        for a in &BASIS[1..] {
            assert!(!json.contains(a), "{a} im Speicher");
        }
        assert_eq!(e.privat, 1);
    }

    #[test]
    fn too_few_public_hops_not_judgeable() {
        let e = eintrag("z", &weg(&["192.168.1.1", "203.0.113.1"]), 0);
        let (k, t) = bewerten(&[e]).unwrap();
        assert_eq!(k, Klasse::Harmlos);
        assert!(t.contains("nicht beurteilbar"));
    }

    #[test]
    fn hops_with_one_answer_do_not_count() {
        let mut h = weg(&BASIS);
        h[2] = hop(3, &["198.51.100.7"], 1);
        assert_eq!(eintrag("z", &h, 0).netze.len(), 2);
    }

    #[test]
    fn comparison_levels() {
        let basis = eintrag("z", &weg(&BASIS), 0);
        // unchanged
        let (k, t) = bewerten(&[basis.clone(), basis.clone()]).unwrap();
        assert_eq!(k, Klasse::Harmlos);
        assert!(t.contains("dieselben Stationen"));
        // other later hop, same length and same first public hop: harmless
        let mut a = BASIS;
        a[3] = "203.0.113.9";
        let leicht = eintrag("z", &weg(&a), 0);
        let (k, t) = bewerten(&[basis.clone(), leicht]).unwrap();
        assert_eq!(k, Klasse::Harmlos);
        assert!(t.contains("weicht leicht"));
        // first public hop in another /24
        let mut a = BASIS;
        a[1] = "198.51.100.3";
        let erster = eintrag("z", &weg(&a), 0);
        assert_eq!(
            bewerten(&[basis.clone(), erster]).unwrap().0,
            Klasse::Auffaellig
        );
        // 3 hops more
        let mut v = BASIS.to_vec();
        v.extend(["203.0.113.50", "198.51.100.50", "192.0.2.50"]);
        let lang = eintrag("z", &weg(&v), 0);
        assert_eq!(bewerten(&[basis, lang]).unwrap().0, Klasse::Auffaellig);
    }

    #[test]
    fn extra_private_hop_alarms_only_second_time() {
        let basis = eintrag("z", &weg(&BASIS), 0);
        let mut v = BASIS.to_vec();
        v.insert(1, "10.0.0.1");
        let extra = eintrag("z", &weg(&v), 0);
        assert_eq!(extra.privat, 2);
        let klasse = |h: &[Eintrag]| bewerten(h).map(|r| r.0);
        // first time: no alarm
        let h = [basis.clone(), basis.clone(), extra.clone()];
        assert_ne!(klasse(&h), Some(Klasse::Alarm));
        // second time in a row: alarm
        let h = [basis.clone(), basis, extra.clone(), extra.clone()];
        assert_eq!(klasse(&h), Some(Klasse::Alarm));
        // third time: the new state is the normal one
        let h = [extra.clone(), extra.clone(), extra];
        assert_ne!(klasse(&h), Some(Klasse::Alarm));
    }

    #[test]
    fn nothing_sent_unless_meins_or_forced() {
        let d = std::env::temp_dir().join("pfadspur-test-nicht/netze.json");
        assert!(fuer_pruefen(&d, "a", false).is_empty());
        assert!(!store_path(&d).exists());
    }
}
