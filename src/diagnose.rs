//! `netzinfo diagnose` (issue #31): four ping series at once (router, first public hop of the
//! provider, two independent internet targets), one sentence on where it probably hangs.
//! Measuring is `pfad` (ICMP sockets, path run); the verdict compares with the stored values of
//! the network (`diagnose.json` next to `netze.json`, same key as `qualitaet.json`).
//! ponytail: no signal strength and noise (CoreWLAN) here, that is #37 after #34.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::pfad::{self, Icmp, Prober};
use crate::{explain, hier, netze};

const EINS: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
const NEUN: Ipv4Addr = Ipv4Addr::new(9, 9, 9, 9);
const PROBES: u32 = 20;
const GAP: Duration = Duration::from_millis(200);
/// Wait for stragglers after the last probe.
const LATE: Duration = Duration::from_millis(500);
/// Loss in percent from which a target counts as bad.
const LOSS_PCT: f64 = 5.0;
/// P95 above this multiple of the stored P95 counts as bad.
const FACTOR: f64 = 3.0;
const MAX_BASIS: usize = 10;
const NAMES: [&str; 4] = [
    "Router",
    "Erster Rechner des Anbieters",
    "1.1.1.1",
    "9.9.9.9",
];
/// Fixed limits for the median without stored values: router, provider hop, internet targets.
const FIXED_MS: [f64; 4] = [20.0, 150.0, 150.0, 150.0];

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Schlecht,
    Stumm,
    /// Not measured (no public hop found: hotspot, company network).
    Fehlt,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Schicht {
    WlanOderRouter,
    Anbieter,
    WeitWeg,
    InOrdnung,
    KeinInternet,
}

impl Schicht {
    pub fn satz(self) -> &'static str {
        match self {
            Schicht::WlanOderRouter => "Das Problem liegt wahrscheinlich im WLAN oder am Router.",
            Schicht::Anbieter => "Das Problem liegt wahrscheinlich beim Anbieter.",
            Schicht::WeitWeg => {
                "Weit weg, nicht dein Anschluss: nur ein Ziel im Internet ist schlecht."
            }
            Schicht::InOrdnung => "Keine Auffälligkeit gemessen.",
            Schicht::KeinInternet => "Kein Internet oder ICMP gesperrt.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ziel {
    pub name: String,
    pub adresse: Option<Ipv4Addr>,
    pub gesendet: u32,
    pub beantwortet: u32,
    pub verlust_prozent: f64,
    pub median_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub status: Status,
    /// Why the target is bad; empty when it is fine.
    pub gruende: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagnoseErgebnis {
    pub ziele: Vec<Ziel>,
    pub urteil: Schicht,
    /// One sentence, same as `urteil.satz()`.
    pub satz: String,
    pub begruendung: Vec<String>,
    /// TCP check on 1.1.1.1:443, only when all targets were silent.
    pub tcp_443: Option<bool>,
    /// Compared with stored values of this network (else fixed limits).
    pub mit_basis: bool,
    /// Unix seconds.
    pub zeitpunkt: u64,
}

// ---- stored values -------------------------------------------------------------------------

/// Median and P95 per target of one run rated „in Ordnung“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Lauf {
    zeit: u64,
    median: [Option<f64>; 4],
    p95: [Option<f64>; 4],
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct NetDiag {
    /// TTL and address of the remembered first public hop.
    hop: Option<(u8, Ipv4Addr)>,
    basis: Vec<Lauf>,
}

type Store = std::collections::BTreeMap<String, NetDiag>;

fn store_path() -> Result<PathBuf, String> {
    Ok(netze::data_path()?.with_file_name("diagnose.json"))
}

#[derive(Debug, Default)]
struct Basis {
    median: [Option<f64>; 4],
    p95: [Option<f64>; 4],
    laeufe: usize,
}

fn mitte(v: impl Iterator<Item = f64>) -> Option<f64> {
    let mut v: Vec<f64> = v.collect();
    v.sort_by(f64::total_cmp);
    (!v.is_empty()).then(|| pfad::median(&v))
}

fn basis_aus(laeufe: &[Lauf]) -> Basis {
    let mut b = Basis {
        laeufe: laeufe.len(),
        ..Basis::default()
    };
    for i in 0..4 {
        b.median[i] = mitte(laeufe.iter().filter_map(|l| l.median[i]));
        b.p95[i] = mitte(laeufe.iter().filter_map(|l| l.p95[i]));
    }
    b
}

/// Remembers the hop and, for a run rated „in Ordnung“, its values (the last `MAX_BASIS`).
fn speichern(
    path: &Path,
    key: &str,
    hop: Option<(u8, Ipv4Addr)>,
    lauf: Option<Lauf>,
) -> Result<(), String> {
    let mut store: Store = netze::load_json(path)?;
    let d = store.entry(key.to_string()).or_default();
    d.hop = hop;
    if let Some(l) = lauf {
        d.basis.push(l);
        let weg = d.basis.len().saturating_sub(MAX_BASIS);
        d.basis.drain(..weg);
    }
    netze::save(path, &store)
}

// ---- measuring -----------------------------------------------------------------------------

/// Books replies from `addr` until `until`; `recv` returning `None` ends the wait early.
fn warte(
    p: &mut dyn Prober,
    addr: Ipv4Addr,
    until: Instant,
    sent: &[Instant],
    got: &mut [bool],
    rtts: &mut Vec<f64>,
) {
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Some((from, r)) = p.recv(left) else { break };
        let i = usize::from(r.seq);
        if from == addr
            && let (Some(t0), Some(g)) = (sent.get(i), got.get_mut(i))
            && !*g
        {
            *g = true;
            rtts.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if let Some(left) = until.checked_duration_since(Instant::now()) {
        std::thread::sleep(left);
    }
}

/// Sends `n` echo requests with `ttl` one `gap` apart; round trip times in ms in order of arrival.
/// A reply counts if it comes from `addr`: the echo reply of the target or the „Time Exceeded“
/// of the hop that sits at `ttl`.
fn serie(
    p: &mut dyn Prober,
    addr: Ipv4Addr,
    ttl: u8,
    n: u32,
    gap: Duration,
    late: Duration,
) -> Vec<f64> {
    let (mut sent, mut got, mut rtts) = (Vec::new(), vec![false; n as usize], Vec::new());
    for seq in 0..n {
        // a failed send is a lost probe
        let _ = p.send(ttl, seq as u16);
        sent.push(Instant::now());
        warte(p, addr, Instant::now() + gap, &sent, &mut got, &mut rtts);
    }
    warte(p, addr, Instant::now() + late, &sent, &mut got, &mut rtts);
    rtts
}

struct Messung {
    adresse: Option<Ipv4Addr>,
    gesendet: u32,
    rtts: Vec<f64>,
}

type Plan = (Ipv4Addr, u8, Box<dyn Prober + Send>);

/// All series at the same time, one thread each. `None` = target not measured.
fn serien(plans: Vec<Option<Plan>>, n: u32, gap: Duration, late: Duration) -> Vec<Messung> {
    std::thread::scope(|s| {
        let jobs: Vec<_> = plans
            .into_iter()
            .map(|pl| {
                pl.map(|(addr, ttl, mut p)| {
                    (
                        addr,
                        s.spawn(move || serie(&mut *p, addr, ttl, n, gap, late)),
                    )
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| match j {
                Some((a, h)) => Messung {
                    adresse: Some(a),
                    gesendet: n,
                    rtts: h.join().unwrap_or_default(),
                },
                None => Messung {
                    adresse: None,
                    gesendet: 0,
                    rtts: vec![],
                },
            })
            .collect()
    })
}

// ---- rules ---------------------------------------------------------------------------------

fn zahl(x: f64) -> String {
    format!("{x:.1}").replace('.', ",")
}

/// Rating of one target. The rule table:
/// - nothing answered: `Stumm`
/// - loss from 5 %: bad
/// - with stored values: P95 above 3 times the stored P95: bad
/// - without: median above the fixed limit (router 20 ms, others 150 ms): bad
fn bewerte(i: usize, m: &Messung, basis: &Basis) -> Ziel {
    let name = NAMES[i];
    let mut s = m.rtts.clone();
    s.sort_by(f64::total_cmp);
    let has = !s.is_empty();
    let beantwortet = s.len() as u32;
    let verlust = if m.gesendet == 0 {
        0.0
    } else {
        100.0 * f64::from(m.gesendet - beantwortet) / f64::from(m.gesendet)
    };
    let median = has.then(|| pfad::median(&s));
    let p95 = has.then(|| pfad::p95(&s));
    let mut gruende = Vec::new();
    let status = if m.gesendet == 0 {
        Status::Fehlt
    } else if !has {
        gruende.push(format!("{name} antwortet nicht."));
        Status::Stumm
    } else {
        if verlust >= LOSS_PCT {
            gruende.push(format!("{name} verliert {} % der Pakete.", zahl(verlust)));
        }
        match (basis.p95[i], median, p95) {
            (Some(b), Some(mm), Some(pp)) if pp > FACTOR * b => {
                let sonst = basis.median[i].map_or(String::new(), |bm| {
                    format!(
                        " Im Mittel (Median) antwortet {name} hier sonst in {} ms, jetzt in {} ms.",
                        zahl(bm),
                        zahl(mm)
                    )
                });
                gruende.push(format!(
                    "{name}: Im schlechten Moment (95. Perzentil) {} ms statt sonst {} ms, mehr als das Dreifache.{sonst}",
                    zahl(pp),
                    zahl(b)
                ));
            }
            (None, Some(mm), _) if mm > FIXED_MS[i] => gruende.push(format!(
                "{name}: Median {} ms, über der festen Grenze von {} ms.",
                zahl(mm),
                zahl(FIXED_MS[i])
            )),
            _ => {}
        }
        if gruende.is_empty() {
            Status::Ok
        } else {
            Status::Schlecht
        }
    };
    Ziel {
        name: name.to_string(),
        adresse: m.adresse,
        gesendet: m.gesendet,
        beantwortet,
        verlust_prozent: verlust,
        median_ms: median,
        p95_ms: p95,
        jitter_ms: (m.rtts.len() > 1).then(|| {
            m.rtts.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (m.rtts.len() - 1) as f64
        }),
        status,
        gruende,
    }
}

/// Verdict from the four ratings. `tcp` is the check on 1.1.1.1:443, only asked when all are silent.
fn urteil(z: &[Ziel], tcp: Option<bool>) -> (Schicht, Vec<String>) {
    let st = |i: usize| z[i].status;
    let gruende = |r: std::ops::Range<usize>| -> Vec<String> {
        z[r].iter().flat_map(|x| x.gruende.clone()).collect()
    };
    if z.iter()
        .filter(|x| x.status != Status::Fehlt)
        .all(|x| x.status == Status::Stumm)
    {
        let gegenprobe = match tcp {
            Some(true) => {
                "Eine TCP-Verbindung zu 1.1.1.1:443 klappt aber: Das Internet geht, Ping (ICMP) wird wohl gesperrt."
            }
            Some(false) => {
                "Auch eine TCP-Verbindung zu 1.1.1.1:443 klappt nicht: Wahrscheinlich ist keine Verbindung ins Internet da."
            }
            None => "Eine TCP-Gegenprobe zu 1.1.1.1:443 wurde nicht gemacht.",
        };
        return (
            Schicht::KeinInternet,
            vec![
                "Keine der gemessenen Stellen antwortet auf Ping.".into(),
                gegenprobe.into(),
            ],
        );
    }
    if st(0) != Status::Ok {
        return (Schicht::WlanOderRouter, gruende(0..1));
    }
    let schicht = match (2..4).filter(|&i| st(i) != Status::Ok).count() {
        2 => Schicht::Anbieter,
        1 => Schicht::WeitWeg,
        _ => Schicht::InOrdnung,
    };
    let mut b = vec!["Der Router antwortet in Ordnung.".to_string()];
    if schicht == Schicht::InOrdnung {
        b = vec!["Router und beide Internet-Ziele antworten ohne Auffälligkeit.".into()];
        if matches!(st(1), Status::Schlecht | Status::Stumm) {
            b.push("Nur der erste Rechner des Anbieters fällt auf. Das ist meist gedrosseltes Ping und kein Problem, weil die Ziele dahinter gut sind.".into());
        }
    } else {
        b.extend(gruende(1..4));
    }
    (schicht, b)
}

/// Ratings and verdict of four measurements.
fn auswerten(m: &[Messung], basis: &Basis, tcp: Option<bool>) -> (Vec<Ziel>, Schicht, Vec<String>) {
    let ziele: Vec<Ziel> = m
        .iter()
        .enumerate()
        .map(|(i, x)| bewerte(i, x, basis))
        .collect();
    let (s, b) = urteil(&ziele, tcp);
    (ziele, s, b)
}

// ---- running -------------------------------------------------------------------------------

/// TTL and address of the first public hop on the way to 1.1.1.1 (short path run).
fn finde_hop() -> Option<(u8, Ipv4Addr)> {
    let p = pfad::pfad_daten("1.1.1.1", 6, 3, false).ok()?;
    let h = p.hops.iter().find(|h| h.erster_oeffentlicher)?;
    match h.adressen.first()? {
        std::net::IpAddr::V4(a) => Some((h.ttl, *a)),
        _ => None,
    }
}

/// Series whose replies count when they come from `von`. For a hop the packets go to 1.1.1.1
/// with TTL = hop number: many providers answer „Time Exceeded“ but not a direct ping.
fn plan(von: Ipv4Addr, nach: Ipv4Addr, ttl: u8) -> Result<Option<Plan>, String> {
    Ok(Some((von, ttl, Box::new(Icmp::open(nach)?))))
}

fn tcp_443() -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([1, 1, 1, 1], 443)),
        Duration::from_secs(2),
    )
    .is_ok()
}

/// Measures the four targets now. The stored values and the remembered hop come from the
/// network of the current connection; without a fingerprint (VPN) fixed limits apply.
pub fn diagnose_daten() -> Result<DiagnoseErgebnis, String> {
    let info = hier::collect()?;
    let router = info.router.ok_or(
        "Kein Router gefunden. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.",
    )?;
    let ort = netze::fingerprint(&info)
        .ok()
        .and_then(|fp| Some((store_path().ok()?, fp.key())));
    let alt: NetDiag = match &ort {
        Some((p, k)) => netze::load_json::<Store>(p)?.remove(k).unwrap_or_default(),
        None => NetDiag::default(),
    };
    let basis = basis_aus(&alt.basis);
    let gemerkt = alt.hop.is_some();
    let mut hop = alt.hop.or_else(finde_hop);
    let plans = vec![
        plan(router, router, 64)?,
        match hop {
            Some((ttl, a)) => plan(a, EINS, ttl)?,
            None => None,
        },
        plan(EINS, EINS, 64)?,
        plan(NEUN, NEUN, 64)?,
    ];
    let mut m = serien(plans, PROBES, GAP, LATE);
    // A remembered hop that is now silent: ask again once, the path may have changed.
    // ponytail: a truly silent hop costs a second path run (a few seconds) on every run.
    if gemerkt
        && m[1].rtts.is_empty()
        && let Some(neu) = finde_hop()
        && Some(neu) != hop
    {
        hop = Some(neu);
        m[1] = serien(vec![plan(neu.1, EINS, neu.0)?], PROBES, GAP, LATE).remove(0);
    }
    let tcp = m
        .iter()
        .filter(|x| x.gesendet > 0)
        .all(|x| x.rtts.is_empty())
        .then(tcp_443);
    let (ziele, schicht, begruendung) = auswerten(&m, &basis, tcp);
    if let Some((p, k)) = &ort {
        let ok = ziele
            .iter()
            .all(|z| matches!(z.status, Status::Ok | Status::Fehlt));
        let lauf = (schicht == Schicht::InOrdnung && ok).then(|| Lauf {
            zeit: netze::now(),
            median: std::array::from_fn(|i| ziele[i].median_ms),
            p95: std::array::from_fn(|i| ziele[i].p95_ms),
        });
        speichern(p, k, hop, lauf)?;
    }
    Ok(DiagnoseErgebnis {
        ziele,
        urteil: schicht,
        satz: schicht.satz().to_string(),
        begruendung,
        tcp_443: tcp,
        mit_basis: basis.laeufe > 0,
        zeitpunkt: netze::now(),
    })
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{} ms", zahl(x)))
}

fn status_wort(s: Status) -> &'static str {
    match s {
        Status::Ok => "in Ordnung",
        Status::Schlecht => "auffällig",
        Status::Stumm => "stumm",
        Status::Fehlt => "nicht gemessen",
    }
}

fn schicht_erklaerung(s: Schicht) -> &'static str {
    match s {
        Schicht::WlanOderRouter => {
            "Der Router ist die erste Station hinter deinem WLAN oder Kabel. Antwortet er langsam oder verliert Pakete, liegt es am Funk, am Kabel oder am Router selbst. Ich vergleiche mit den gespeicherten Werten dieses Netzes, etwa „normalerweise antwortet dein Router hier in 3 ms, jetzt in 40 ms“. Das spricht für WLAN oder einen überlasteten Router."
        }
        Schicht::Anbieter => {
            "Dein Router antwortet schnell und ohne Verlust, aber schon der erste Rechner deines Anbieters oder beide Ziele im Internet haben Probleme. Das Problem liegt außerhalb deiner Wohnung."
        }
        Schicht::WeitWeg => {
            "Router und Anbieter sind in Ordnung, nur eines der beiden Ziele im Internet ist schlecht. Dann liegt es am Weg dorthin oder am Ziel selbst, nicht an deinem Anschluss."
        }
        Schicht::InOrdnung => {
            "An keiner der vier Stellen fällt etwas auf. Tritt das Problem nur manchmal auf, hilft `netzinfo messen` über mehrere Stunden: Kurze Störungen unter fünf Sekunden verpasst dieser Test."
        }
        Schicht::KeinInternet => {
            "Router und Internet antworten alle nicht auf Ping. Entweder ist das Netz wirklich ohne Internet, oder Ping (ICMP) ist gesperrt. Darum probiere ich zur Gegenprobe eine TCP-Verbindung zu 1.1.1.1 auf Port 443, wie sie jeder Browser aufbaut."
        }
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    if let Some(a) = args.first() {
        return Err(format!(
            "„{a}“ passt hier nicht. Aufruf: `netzinfo diagnose` (ohne Optionen, außer `--erklaer`)."
        ));
    }
    explain(
        explain_on,
        "Ich schicke an vier Stellen gleichzeitig je 20 Ping-Pakete (ICMP Echo) im Abstand von 200 Millisekunden: an deinen Router, an den ersten Rechner deines Anbieters (den finde ich mit einem kurzen Pfadlauf, bei dem ein Paket mit kleiner Lebensdauer, dem TTL, schon dort umkehrt) und an die Server 1.1.1.1 und 9.9.9.9. Das dauert etwa 5 Sekunden und sind zusammen unter 10 kB.",
    );
    let r = diagnose_daten()?;
    println!(
        "{}",
        if r.mit_basis {
            "Vergleich mit den gespeicherten Werten dieses Netzes."
        } else {
            "Noch keine gespeicherten Werte für dieses Netz, es gelten feste Grenzen."
        }
    );
    for z in &r.ziele {
        println!(
            "  {:<30} {:>2}/{:<2} Verlust {:>5} %  Median {:>9}  P95 {:>9}  Jitter {:>9}  {}",
            z.name,
            z.beantwortet,
            z.gesendet,
            zahl(z.verlust_prozent),
            ms(z.median_ms),
            ms(z.p95_ms),
            ms(z.jitter_ms),
            status_wort(z.status)
        );
    }
    println!("\nUrteil: {}", r.satz);
    for b in &r.begruendung {
        println!("  - {b}");
    }
    explain(explain_on, schicht_erklaerung(r.urteil));
    explain(
        explain_on,
        "Das Urteil ist eine Vermutung aus vier Messreihen, kein Beweis. Router und Anbieter drosseln Ping manchmal, deshalb zählt nur, wie Router und beide Internet-Ziele zusammen aussehen.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(rtts: &[f64], gesendet: u32) -> Messung {
        Messung {
            adresse: Some(Ipv4Addr::new(192, 0, 2, 1)),
            gesendet,
            rtts: rtts.to_vec(),
        }
    }

    /// 20 probes, `ok` answered with the same time.
    fn gleich(ok: u32, t: f64) -> Messung {
        msg(&vec![t; ok as usize], 20)
    }

    fn mit_basis(p95: f64) -> Basis {
        Basis {
            median: [Some(3.0); 4],
            p95: [Some(p95); 4],
            laeufe: 3,
        }
    }

    #[test]
    fn loss_threshold_edges() {
        let b = Basis::default();
        assert_eq!(bewerte(0, &gleich(20, 3.0), &b).status, Status::Ok);
        // 19 of 20 = 5 % loss: bad
        assert_eq!(bewerte(0, &gleich(19, 3.0), &b).status, Status::Schlecht);
        assert_eq!(bewerte(2, &gleich(19, 3.0), &b).status, Status::Schlecht);
        assert_eq!(bewerte(0, &gleich(0, 3.0), &b).status, Status::Stumm);
        assert_eq!(bewerte(1, &msg(&[], 0), &b).status, Status::Fehlt);
    }

    #[test]
    fn fixed_limits_without_basis() {
        let b = Basis::default();
        assert_eq!(bewerte(0, &gleich(20, 20.0), &b).status, Status::Ok);
        assert_eq!(bewerte(0, &gleich(20, 20.5), &b).status, Status::Schlecht);
        assert_eq!(bewerte(2, &gleich(20, 150.0), &b).status, Status::Ok);
        assert_eq!(bewerte(3, &gleich(20, 150.5), &b).status, Status::Schlecht);
        assert_eq!(bewerte(1, &gleich(20, 150.5), &b).status, Status::Schlecht);
    }

    #[test]
    fn relative_limit_with_basis() {
        // 25 ms is bad against the fixed 20 ms, but fine against a stored P95 of 10 ms
        assert_eq!(
            bewerte(0, &gleich(20, 25.0), &Basis::default()).status,
            Status::Schlecht
        );
        assert_eq!(
            bewerte(0, &gleich(20, 25.0), &mit_basis(10.0)).status,
            Status::Ok
        );
        assert_eq!(
            bewerte(0, &gleich(20, 30.0), &mit_basis(10.0)).status,
            Status::Ok
        );
        let z = bewerte(0, &gleich(20, 30.5), &mit_basis(10.0));
        assert_eq!(z.status, Status::Schlecht);
        assert!(
            z.gruende[0].contains("statt sonst 10,0 ms"),
            "{:?}",
            z.gruende
        );
        assert!(z.gruende[0].contains(
            "Im Mittel (Median) antwortet Router hier sonst in 3,0 ms, jetzt in 30,5 ms"
        ));
        // 150 ms is fine for the fixed limit but 3 times above a stored 10 ms
        assert_eq!(
            bewerte(2, &gleich(20, 150.0), &Basis::default()).status,
            Status::Ok
        );
        assert_eq!(
            bewerte(2, &gleich(20, 150.0), &mit_basis(10.0)).status,
            Status::Schlecht
        );
    }

    #[test]
    fn basis_is_median_of_runs() {
        let l = |m: f64, p: Option<f64>| Lauf {
            zeit: 0,
            median: [Some(m), None, Some(m), Some(m)],
            p95: [p, None, Some(1.0), Some(1.0)],
        };
        let b = basis_aus(&[l(2.0, Some(4.0)), l(4.0, Some(6.0)), l(100.0, None)]);
        assert_eq!(b.median[0], Some(4.0));
        assert_eq!(b.p95[0], Some(5.0));
        assert_eq!(b.median[1], None);
        assert_eq!(b.laeufe, 3);
        assert!(basis_aus(&[]).median[0].is_none());
    }

    fn ziele(r: Messung, h: Messung, a: Messung, b: Messung) -> (Schicht, Vec<String>) {
        let (_, s, g) = auswerten(&[r, h, a, b], &Basis::default(), Some(false));
        (s, g)
    }

    #[test]
    fn all_five_verdicts() {
        let gut = || gleich(20, 3.0);
        let still = || gleich(0, 0.0);
        let schlecht = || gleich(15, 3.0);
        assert_eq!(ziele(gut(), gut(), gut(), gut()).0, Schicht::InOrdnung);
        assert_eq!(
            ziele(schlecht(), gut(), gut(), gut()).0,
            Schicht::WlanOderRouter
        );
        assert_eq!(
            ziele(still(), gut(), gut(), gut()).0,
            Schicht::WlanOderRouter
        );
        assert_eq!(
            ziele(gut(), schlecht(), schlecht(), schlecht()).0,
            Schicht::Anbieter
        );
        assert_eq!(
            ziele(gut(), still(), still(), schlecht()).0,
            Schicht::Anbieter
        );
        assert_eq!(ziele(gut(), gut(), gut(), schlecht()).0, Schicht::WeitWeg);
        assert_eq!(ziele(gut(), gut(), still(), gut()).0, Schicht::WeitWeg);
        assert_eq!(
            ziele(still(), still(), still(), still()).0,
            Schicht::KeinInternet
        );
        // no public hop found: the other three decide
        let fehlt = || msg(&[], 0);
        assert_eq!(
            ziele(still(), fehlt(), still(), still()).0,
            Schicht::KeinInternet
        );
        assert_eq!(ziele(gut(), fehlt(), gut(), gut()).0, Schicht::InOrdnung);
        // only the hop is bad, the targets behind it are fine: throttled ping
        let (s, g) = ziele(gut(), still(), gut(), gut());
        assert_eq!(s, Schicht::InOrdnung);
        assert!(g.iter().any(|x| x.contains("gedrosseltes Ping")));
        // reasons name the bad target
        let (_, g) = ziele(gut(), gut(), gut(), schlecht());
        assert!(
            g.iter()
                .any(|x| x.contains("9.9.9.9 verliert 25,0 % der Pakete"))
        );
    }

    #[test]
    fn tcp_check_changes_the_reason_only() {
        let z: Vec<Ziel> = (0..4)
            .map(|i| bewerte(i, &gleich(0, 0.0), &Basis::default()))
            .collect();
        let (s, b) = urteil(&z, Some(true));
        assert_eq!(s, Schicht::KeinInternet);
        assert!(b[1].contains("klappt aber"));
        assert!(urteil(&z, Some(false)).1[1].contains("klappt nicht"));
    }

    /// Fake socket: answers probe `seq` if `antwort(seq)`, after 1 ms.
    struct Fake {
        von: Ipv4Addr,
        echo: bool,
        antwort: fn(u16) -> bool,
        offen: Option<u16>,
    }

    impl Prober for Fake {
        fn send(&mut self, _ttl: u8, seq: u16) -> Result<(), String> {
            self.offen = (self.antwort)(seq).then_some(seq);
            Ok(())
        }
        fn recv(&mut self, _wait: Duration) -> Option<(Ipv4Addr, pfad::Reply)> {
            let seq = self.offen.take()?;
            std::thread::sleep(Duration::from_millis(1));
            Some((
                self.von,
                pfad::Reply {
                    seq,
                    echo: self.echo,
                },
            ))
        }
    }

    fn fake(i: u8, echo: bool, antwort: fn(u16) -> bool) -> Option<Plan> {
        let a = Ipv4Addr::new(192, 0, 2, i);
        Some((
            a,
            64,
            Box::new(Fake {
                von: a,
                echo,
                antwort,
                offen: None,
            }),
        ))
    }

    fn lauf(plans: Vec<Option<Plan>>) -> (Schicht, Vec<Ziel>) {
        let m = serien(
            plans,
            20,
            Duration::from_millis(3),
            Duration::from_millis(5),
        );
        let (z, s, _) = auswerten(&m, &Basis::default(), Some(false));
        (s, z)
    }

    #[test]
    fn five_verdicts_with_fake_sockets() {
        let immer: fn(u16) -> bool = |_| true;
        let nie: fn(u16) -> bool = |_| false;
        let drittel: fn(u16) -> bool = |s| s % 3 != 0;
        let (s, z) = lauf(vec![
            fake(1, true, immer),
            fake(2, false, immer),
            fake(3, true, immer),
            fake(4, true, immer),
        ]);
        assert_eq!(s, Schicht::InOrdnung);
        assert!(
            z.iter()
                .all(|x| x.beantwortet == 20 && x.status == Status::Ok)
        );
        let wlan = lauf(vec![
            fake(1, true, drittel),
            fake(2, false, immer),
            fake(3, true, immer),
            fake(4, true, immer),
        ]);
        assert_eq!(wlan.0, Schicht::WlanOderRouter);
        let anbieter = lauf(vec![
            fake(1, true, immer),
            fake(2, false, nie),
            fake(3, true, drittel),
            fake(4, true, drittel),
        ]);
        assert_eq!(anbieter.0, Schicht::Anbieter);
        let weit = lauf(vec![
            fake(1, true, immer),
            fake(2, false, immer),
            fake(3, true, immer),
            fake(4, true, nie),
        ]);
        assert_eq!(weit.0, Schicht::WeitWeg);
        let tot = lauf(vec![
            fake(1, true, nie),
            fake(2, false, nie),
            fake(3, true, nie),
            fake(4, true, nie),
        ]);
        assert_eq!(tot.0, Schicht::KeinInternet);
        // no hop: three series, the hop shows as not measured
        let (s, z) = lauf(vec![
            fake(1, true, immer),
            None,
            fake(3, true, immer),
            fake(4, true, immer),
        ]);
        assert_eq!((s, z[1].status), (Schicht::InOrdnung, Status::Fehlt));
    }

    #[test]
    fn foreign_replies_do_not_count() {
        let mut f = Fake {
            von: Ipv4Addr::new(192, 0, 2, 9),
            echo: true,
            antwort: |_| true,
            offen: None,
        };
        let ms2 = Duration::from_millis(2);
        assert!(serie(&mut f, Ipv4Addr::new(192, 0, 2, 1), 64, 3, ms2, ms2).is_empty());
    }

    #[test]
    fn only_ok_runs_enter_the_store() {
        let dir = std::env::temp_dir().join(format!("netz-diagnose-{}", std::process::id()));
        let path = dir.join("diagnose.json");
        let hop = Some((2, Ipv4Addr::new(203, 0, 113, 1)));
        let l = |t| Lauf {
            zeit: t,
            median: [Some(3.0); 4],
            p95: [Some(5.0); 4],
        };
        for t in 0..12 {
            speichern(&path, "k", hop, Some(l(t))).unwrap();
        }
        speichern(&path, "k", hop, None).unwrap();
        speichern(&path, "anderes", None, None).unwrap();
        let s: Store = netze::load_json(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(s["k"].basis.len(), MAX_BASIS);
        assert_eq!(s["k"].basis[0].zeit, 2);
        assert_eq!(s["k"].hop, hop);
        assert!(s["anderes"].basis.is_empty());
    }

    #[test]
    fn wrong_option_and_texts() {
        assert!(
            run(&["--x".into()], false)
                .unwrap_err()
                .contains("passt hier nicht")
        );
        for s in [
            Schicht::WlanOderRouter,
            Schicht::Anbieter,
            Schicht::WeitWeg,
            Schicht::InOrdnung,
            Schicht::KeinInternet,
        ] {
            assert!(!schicht_erklaerung(s).is_empty() && !s.satz().is_empty());
        }
    }
}
