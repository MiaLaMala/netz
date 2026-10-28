//! `netz pruefen`: measure the current network and compare it with the last visit (issue #6).
//!
//! A check that failed (no internet, timeout) is `None`. `None` is shown, never compared, and
//! never overwrites an older good value in the store.

use crate::explain;
use crate::netze::{self, Fingerprint};
use crate::tls::{self, Verdict};
use crate::{dns, hier, portal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{IpAddr, Ipv4Addr};

const TLS_HOSTS: [&str; 2] = ["github.com", "apple.com"];
const PUBLIC_DNS: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Tv {
    Normal,
    Aufgebrochen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TlsM {
    pub host: String,
    /// `None`: the handshake failed.
    pub verdict: Option<Tv>,
    /// SHA-256 of the certificate that signed the leaf.
    pub issuer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Messung {
    /// Unix seconds.
    pub time: u64,
    pub router: Ipv4Addr,
    pub dhcp_server: Ipv4Addr,
    pub dhcp_dns: Vec<Ipv4Addr>,
    /// github.com (A) blocked at the system resolver / at 1.1.1.1.
    pub blocked_local: Option<bool>,
    pub blocked_public: Option<bool>,
    /// Local resolver invents an answer for a name that does not exist.
    pub hijack: Option<bool>,
    pub tls: Vec<TlsM>,
    pub public_ip: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Klasse {
    Harmlos,
    Auffaellig,
    Alarm,
}

type Store = BTreeMap<String, Messung>;

fn random_label() -> String {
    format!("netz-{:016x}", RandomState::new().build_hasher().finish())
}

fn list(v: &[Ipv4Addr]) -> String {
    v.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn measure(fp: &Fingerprint, dhcp_dns: Vec<Ipv4Addr>) -> Messung {
    let local = dns::system_resolver();
    let ask = |server: Result<IpAddr, String>, name: &str| {
        server
            .and_then(|s| dns::query(s, name, dns::A, dns::IN))
            .ok()
    };
    let l = ask(local.clone(), "github.com");
    let p = ask(Ok(PUBLIC_DNS), "github.com");
    let fake = format!("{}.example.com", random_label());
    let hijack = match (ask(local, &fake), ask(Ok(PUBLIC_DNS), &fake)) {
        (Some(l), Some(p)) => {
            Some(l.rcode == 0 && l.records.iter().any(|r| r.rtype == dns::A) && p.rcode == 3)
        }
        _ => None,
    };
    let tls = TLS_HOSTS
        .iter()
        .map(|h| {
            let r = tls::check_host(h).ok();
            let verdict = r.as_ref().and_then(|r| match r.verdict {
                Verdict::Normal => Some(Tv::Normal),
                Verdict::Aufgebrochen => Some(Tv::Aufgebrochen),
                Verdict::Fehler(_) => None,
            });
            let issuer = r
                .and_then(|r| r.chain)
                .and_then(|c| c.issuer_sha256)
                .filter(|_| verdict.is_some());
            TlsM {
                host: h.to_string(),
                verdict,
                issuer,
            }
        })
        .collect();
    let public_ip = dns::query(PUBLIC_DNS, "whoami.cloudflare", dns::TXT, dns::CH)
        .ok()
        .and_then(|a| a.records.into_iter().find(|r| r.rtype == dns::TXT))
        .map(|r| r.data.trim_matches('"').to_string());
    Messung {
        time: netze::now(),
        router: fp.router,
        dhcp_server: fp.dhcp_server,
        dhcp_dns,
        blocked_local: l.map(|a| dns::blocked(&a, dns::A)),
        blocked_public: p.map(|a| dns::blocked(&a, dns::A)),
        hijack,
        tls,
        public_ip,
    }
}

/// Differences between the last and the current measurement, worst first.
/// Failed checks (`None`) on either side are skipped.
pub fn compare(old: &Messung, new: &Messung) -> Vec<(Klasse, String)> {
    compare_in(old, new, false)
}

/// Like `compare`; behind a captive portal (`portal`) TLS and NXDOMAIN findings are only
/// `Auffaellig`, because the portal causes them.
fn compare_in(old: &Messung, new: &Messung, portal: bool) -> Vec<(Klasse, String)> {
    use Klasse::*;
    let mut out = Vec::new();
    let intercept = if portal { Auffaellig } else { Alarm };
    if old.dhcp_server == old.router && new.dhcp_server != new.router {
        out.push((Alarm, format!("Der DHCP-Server ({}) ist nicht mehr der Router ({}). Vorher waren beide dasselbe Gerät: Vielleicht verteilt jetzt ein fremdes Gerät Adressen.", new.dhcp_server, new.router)));
    }
    let sorted = |v: &[Ipv4Addr]| {
        let mut v = v.to_vec();
        v.sort();
        v
    };
    if !old.dhcp_dns.is_empty()
        && !new.dhcp_dns.is_empty()
        && sorted(&old.dhcp_dns) != sorted(&new.dhcp_dns)
    {
        out.push((
            Auffaellig,
            format!(
                "Das Netz nennt andere DNS-Server als beim letzten Besuch: jetzt {}, vorher {}.",
                list(&new.dhcp_dns),
                list(&old.dhcp_dns)
            ),
        ));
    }
    if let (Some(o), Some(n)) = (old.blocked_local, new.blocked_local)
        && o != n
    {
        out.push((
            Auffaellig,
            if n {
                "github.com wird von deinem Netz jetzt blockiert, vorher nicht.".into()
            } else {
                "github.com wird von deinem Netz nicht mehr blockiert, vorher schon.".into()
            },
        ));
    }
    if old.hijack == Some(false) && new.hijack == Some(true) {
        out.push((intercept, "Dein Netz erfindet jetzt Antworten für Namen, die es nicht gibt (NXDOMAIN-Umleitung), vorher nicht. Das kann ein Captive Portal oder DNS-Hijacking sein.".into()));
    }
    for n in &new.tls {
        let Some(o) = old.tls.iter().find(|o| o.host == n.host) else {
            continue;
        };
        let (Some(ov), Some(nv)) = (o.verdict, n.verdict) else {
            continue;
        };
        if ov == Tv::Normal && nv == Tv::Aufgebrochen {
            out.push((intercept, format!("Das Zertifikat von {} stammt jetzt von einer unbekannten Stelle, vorher war es normal. Jemand im Netz liest vielleicht verschlüsselten Verkehr mit.", n.host)));
        } else if ov == Tv::Normal
            && nv == Tv::Normal
            && o.issuer.is_some()
            && n.issuer.is_some()
            && o.issuer != n.issuer
        {
            out.push((Harmlos, format!("Die Stelle, die das Zertifikat von {} ausstellt, hat gewechselt, das Urteil bleibt normal. Zertifikate werden regelmäßig erneuert.", n.host)));
        }
    }
    if let (Some(o), Some(n)) = (&old.public_ip, &new.public_ip)
        && o != n
    {
        out.push((
            Harmlos,
            format!(
                "Neue öffentliche IP: {n}, vorher {o}. Provider vergeben sie immer wieder neu."
            ),
        ));
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.0));
    out
}

/// Keeps older good values where the new measurement failed.
fn fill(mut new: Messung, old: &Messung) -> Messung {
    new.blocked_local = new.blocked_local.or(old.blocked_local);
    new.blocked_public = new.blocked_public.or(old.blocked_public);
    new.hijack = new.hijack.or(old.hijack);
    new.public_ip = new.public_ip.or_else(|| old.public_ip.clone());
    for t in &mut new.tls {
        if t.verdict.is_none()
            && let Some(o) = old.tls.iter().find(|o| o.host == t.host)
        {
            *t = o.clone();
        }
    }
    new
}

fn ja(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "ja",
        Some(false) => "nein",
        None => "nicht messbar",
    }
}

fn failed(m: &Messung) -> usize {
    [m.blocked_local, m.blocked_public, m.hijack]
        .iter()
        .filter(|v| v.is_none())
        .count()
        + m.tls.iter().filter(|t| t.verdict.is_none()).count()
        + usize::from(m.public_ip.is_none())
}

fn show(m: &Messung) {
    println!("Router {}, DHCP-Server {}", m.router, m.dhcp_server);
    println!(
        "DNS-Server vom Netz: {}",
        if m.dhcp_dns.is_empty() {
            "keine".into()
        } else {
            list(&m.dhcp_dns)
        }
    );
    println!(
        "github.com blockiert: im Netz {}, bei 1.1.1.1 {}",
        ja(m.blocked_local),
        ja(m.blocked_public)
    );
    println!("Erfundene Antworten für falsche Namen: {}", ja(m.hijack));
    for t in &m.tls {
        let v = match t.verdict {
            Some(Tv::Normal) => "normal",
            Some(Tv::Aufgebrochen) => "aufgebrochen",
            None => "nicht messbar",
        };
        let id = t.issuer.as_deref().map_or(String::new(), |h| {
            format!(", Aussteller {}", &h[..h.len().min(16)])
        });
        println!("TLS {}: {v}{id}", t.host);
    }
    println!(
        "Öffentliche IP: {}",
        m.public_ip.as_deref().unwrap_or("nicht messbar")
    );
}

/// One difference between two visits.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unterschied {
    pub klasse: Klasse,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct PruefErgebnis {
    /// Name of the stored network.
    pub netz: String,
    pub messung: Messung,
    /// The measurement of the last visit (`time` says when). `None` on the first visit.
    pub vorher: Option<Messung>,
    /// Worst first. Empty on the first visit.
    pub unterschiede: Vec<Unterschied>,
    pub erster_besuch: bool,
    /// Checks that could not be measured; they do not count as changes.
    pub nicht_messbar: usize,
}

/// Measures the current (stored) network, compares with the last visit and stores the result.
pub fn pruefen() -> Result<PruefErgebnis, String> {
    let info = hier::collect()?;
    let fp = netze::fingerprint(&info)?;
    let path = netze::data_path()?;
    let known = netze::load(&path)?;
    let Some(i) = netze::find(&known, &fp) else {
        return Err("Dieses Netz ist nicht gemerkt, ich habe nichts zum Vergleichen. Speichere es mit `netz merken <Name>` und prüfe dann noch mal.".into());
    };
    let store_path = path.with_file_name("pruefungen.json");
    let mut store: Store = netze::load_json(&store_path)?;
    let new = measure(&fp, info.dhcp_dns);
    let portal = portal::check().status == portal::Status::PortalVermutet;
    let mut erg = auswerten(&mut store, fp.key(), new, portal);
    erg.netz = known[i].name.clone();
    netze::save(&store_path, &store)?;
    Ok(erg)
}

/// Compares `new` with the stored measurement of `key` and puts the merged one into the store.
fn auswerten(store: &mut Store, key: String, new: Messung, portal: bool) -> PruefErgebnis {
    let nicht_messbar = failed(&new);
    let vorher = store.get(&key).cloned();
    let mut unterschiede: Vec<Unterschied> = vorher.as_ref().map_or(vec![], |old| {
        compare_in(old, &new, portal)
            .into_iter()
            .map(|(klasse, text)| Unterschied { klasse, text })
            .collect()
    });
    if portal {
        unterschiede.push(Unterschied {
            klasse: Klasse::Harmlos,
            text: "Anmeldung nötig: Dein Netz hängt hinter einem Captive Portal (`netz portal`). Bis zur Anmeldung sind DNS- und Zertifikatsergebnisse nur auffällig, nicht Alarm. Melde dich an und prüfe dann noch mal.".into(),
        });
    }
    let stored = vorher
        .as_ref()
        .map_or(new.clone(), |old| fill(new.clone(), old));
    store.insert(key, stored);
    PruefErgebnis {
        netz: String::new(),
        messung: new,
        erster_besuch: vorher.is_none(),
        vorher,
        unterschiede,
        nicht_messbar,
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    if let Some(a) = args.first() {
        return Err(format!(
            "`netz pruefen` kennt „{a}“ nicht. Es braucht keine Angaben, `--erklaer` erklärt die Schritte."
        ));
    }
    let r = pruefen()?;
    println!("Netz „{}“", r.netz);
    explain(
        explain_on,
        "Ich frage github.com (A) beim DNS-Server deines Netzes und bei 1.1.1.1. Sagt nur dein Netz „gibt es nicht“ oder 0.0.0.0, blockiert es den Namen (Werbefilter, Jugendschutz oder Zensur).",
    );
    explain(
        explain_on,
        "Dann frage ich nach einem Namen, den es sicher nicht gibt (eine Zufallskette unter example.com). Ehrlich ist die Antwort „NXDOMAIN“. Erfindet dein Netz trotzdem eine Adresse, während 1.1.1.1 NXDOMAIN sagt, leitet es falsche Namen um. Das tun Captive Portals und manche Provider, es kann aber auch DNS-Hijacking sein.",
    );
    explain(
        explain_on,
        "Bei github.com und apple.com baue ich eine TLS-Verbindung auf und prüfe, ob das Zertifikat von einer öffentlich bekannten Stelle stammt. Sonst liest vielleicht jemand mit (SSL-Inspection, wie in Firmennetzen oder bei einem Angreifer). Zusätzlich merke ich mir den Fingerabdruck der ausstellenden Stelle.",
    );
    explain(
        explain_on,
        "Meine öffentliche IP erfahre ich von 1.1.1.1 (TXT „whoami.cloudflare“, Klasse CH). Sie gehört dem Provider und wechselt oft, das ist normal.",
    );
    show(&r.messung);
    let Some(old) = &r.vorher else {
        println!("Erste Messung in diesem Netz. Sie ist ab jetzt die Vergleichsbasis.");
        explain(
            explain_on,
            "Beim nächsten Besuch vergleiche ich mit diesen Werten und sage dir, was anders ist.",
        );
        return Ok(());
    };
    let when = netze::format_utc(old.time);
    if r.unterschiede.is_empty() {
        println!("Nichts Auffälliges, alles wie beim letzten Besuch am {when}.");
    } else {
        println!("Seit dem letzten Besuch am {when} ist anders:");
        for u in &r.unterschiede {
            let label = match u.klasse {
                Klasse::Harmlos => "Harmlos",
                Klasse::Auffaellig => "Auffällig",
                Klasse::Alarm => "Alarm",
            };
            println!("{label}: {}", u.text);
        }
    }
    if r.nicht_messbar > 0 {
        println!(
            "{} Prüfung(en) ließen sich nicht messen (Internet? Zeitüberschreitung?). Sie zählen nicht als Änderung.",
            r.nicht_messbar
        );
    }
    explain(
        explain_on,
        "Harmlos heißt: kommt im Alltag vor. Auffällig: schau nach, ob das so gewollt ist. Alarm: nutze das Netz nicht für Passwörter oder Bankgeschäfte, bis du es geklärt hast. Die neue Messung ersetzt jetzt die alte; fehlgeschlagene Prüfungen behalten den alten Wert.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    fn base() -> Messung {
        Messung {
            time: 1793050200,
            router: ip("192.168.178.1"),
            dhcp_server: ip("192.168.178.1"),
            dhcp_dns: vec![ip("192.168.178.1")],
            blocked_local: Some(false),
            blocked_public: Some(false),
            hijack: Some(false),
            tls: ["github.com", "apple.com"]
                .iter()
                .map(|h| TlsM {
                    host: h.to_string(),
                    verdict: Some(Tv::Normal),
                    issuer: Some(format!("aa{h}")),
                })
                .collect(),
            public_ip: Some("203.0.113.7".into()),
        }
    }

    fn classes(old: &Messung, new: &Messung) -> Vec<Klasse> {
        compare(old, new).into_iter().map(|d| d.0).collect()
    }

    #[test]
    fn nothing_changed() {
        assert!(compare(&base(), &base()).is_empty());
    }

    #[test]
    fn harmlos_new_ip_and_rotated_issuer() {
        let mut n = base();
        n.public_ip = Some("203.0.113.99".into());
        n.tls[0].issuer = Some("bb".into());
        assert_eq!(classes(&base(), &n), vec![Klasse::Harmlos, Klasse::Harmlos]);
    }

    #[test]
    fn auffaellig_dns_and_blocking() {
        let mut n = base();
        n.dhcp_dns = vec![ip("192.168.178.53")];
        n.blocked_local = Some(true);
        assert_eq!(classes(&base(), &n), vec![Klasse::Auffaellig; 2]);
        // order of DNS servers does not matter
        let mut a = base();
        a.dhcp_dns = vec![ip("10.0.0.1"), ip("10.0.0.2")];
        let mut b = a.clone();
        b.dhcp_dns.reverse();
        assert!(compare(&a, &b).is_empty());
    }

    #[test]
    fn alarm_tls_hijack_dhcp_and_sorted_first() {
        let mut n = base();
        n.public_ip = Some("203.0.113.99".into());
        n.tls[1].verdict = Some(Tv::Aufgebrochen);
        n.hijack = Some(true);
        n.dhcp_server = ip("192.168.178.77");
        assert_eq!(
            classes(&base(), &n),
            vec![Klasse::Alarm, Klasse::Alarm, Klasse::Alarm, Klasse::Harmlos]
        );
    }

    #[test]
    fn portal_downgrades_tls_and_hijack_to_auffaellig() {
        let mut n = base();
        n.tls[0].verdict = Some(Tv::Aufgebrochen);
        n.hijack = Some(true);
        n.dhcp_server = ip("192.168.178.77");
        let k = |p| -> Vec<Klasse> {
            compare_in(&base(), &n, p)
                .into_iter()
                .map(|d| d.0)
                .collect()
        };
        assert_eq!(k(false), vec![Klasse::Alarm; 3]);
        // the DHCP alarm stays
        assert_eq!(
            k(true),
            vec![Klasse::Alarm, Klasse::Auffaellig, Klasse::Auffaellig]
        );
        let r = auswerten(
            &mut Store::from([("k".into(), base())]),
            "k".into(),
            n,
            true,
        );
        assert!(
            r.unterschiede
                .iter()
                .any(|u| u.text.starts_with("Anmeldung nötig"))
        );
    }

    #[test]
    fn dhcp_server_already_different_is_no_alarm() {
        let mut o = base();
        o.dhcp_server = ip("192.168.178.2");
        let mut n = o.clone();
        n.dhcp_server = ip("192.168.178.3");
        assert!(compare(&o, &n).is_empty());
    }

    #[test]
    fn failed_checks_are_not_changes() {
        let mut n = base();
        n.blocked_local = None;
        n.hijack = None;
        n.public_ip = None;
        n.dhcp_dns = vec![];
        for t in &mut n.tls {
            t.verdict = None;
            t.issuer = None;
        }
        assert!(compare(&base(), &n).is_empty());
        assert!(compare(&n, &base()).is_empty());
        assert_eq!(failed(&n), 5);
        // old good values survive in the store
        assert_eq!(fill(n, &base()).tls, base().tls);
    }

    #[test]
    fn auswerten_first_visit_then_compare_and_fill() {
        let mut store = Store::new();
        let first = auswerten(&mut store, "k".into(), base(), false);
        assert!(first.erster_besuch && first.vorher.is_none() && first.unterschiede.is_empty());
        let mut n = base();
        n.time += 60;
        n.public_ip = None;
        n.dhcp_dns = vec![ip("10.0.0.9")];
        let second = auswerten(&mut store, "k".into(), n, false);
        assert!(!second.erster_besuch);
        assert_eq!(second.vorher.as_ref().map(|m| m.time), Some(base().time));
        assert_eq!(second.nicht_messbar, 1);
        assert_eq!(second.unterschiede.len(), 1);
        assert_eq!(second.unterschiede[0].klasse, Klasse::Auffaellig);
        // shown value stays None, stored value keeps the old IP
        assert_eq!(second.messung.public_ip, None);
        assert_eq!(store["k"].public_ip.as_deref(), Some("203.0.113.7"));
        assert!(
            serde_json::to_string(&second)
                .unwrap()
                .contains("Auffaellig")
        );
    }

    #[test]
    fn store_json_round_trip() {
        let dir = std::env::temp_dir().join(format!("netz-test-{}-pruefen", std::process::id()));
        let path = dir.join("pruefungen.json");
        let mut s = Store::new();
        s.insert("k|1".into(), base());
        let mut gone = base();
        gone.hijack = None;
        s.insert("k|2".into(), gone);
        netze::save(&path, &s).unwrap();
        assert_eq!(netze::load_json::<Store>(&path).unwrap(), s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
