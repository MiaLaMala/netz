//! Ping-Serie mit Jitter und Eignung für Anrufe und Spiele. Issue #46.
//! Eine Sonde ist ein ICMP-Echo über den Datagram-Socket aus `pfad` (ohne sudo). Roh bleiben die
//! letzten 600 Werte im Arbeitsspeicher; gespeichert wird nur eine Kennzahl pro Minute und Ziel.
//! ponytail: nur IPv4 und macOS (wie `pfad`); Rückfall auf TCP-Connect (Linux) und Windows fehlen.

use std::collections::{BTreeMap, VecDeque};
use std::io::IsTerminal;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::pfad::{Icmp, Prober, resolve4};
use crate::statistik::{median, perzentil};
use crate::{explain, hier, netze};

/// Letzte Werte je Ziel im Arbeitsspeicher.
pub const RING: usize = 600;
/// Kennzahlen gelten für die letzten 60 s.
const FENSTER_MS: u64 = 60_000;
/// Die App zeichnet die letzten 5 Minuten.
const ANSICHT_MS: u64 = 300_000;
/// Zeitlimit je Sonde.
const ZEITLIMIT: Duration = Duration::from_secs(2);
/// Kleinster Abstand zwischen zwei Sonden.
pub const MIN_INTERVALL_MS: u64 = 200;
/// Zwei Werte, die weiter auseinanderliegen (Ruhezustand, Pause), zählen nicht für den Jitter.
const LUECKE_MS: u64 = 10_000;
/// Gespeichert wird höchstens ein Fenster pro Minute und Ziel, sieben Tage lang.
const ABLAGE_S: u64 = 60;
const AUFBEWAHREN_S: u64 = 7 * 86_400;

// Eignung: Faustwerte, keine Norm. Unter dem Wert ist es „gut“, bis zum Dreifachen „mit
// Einschränkung“, darüber „schlecht“. Anpassen heißt: hier die Zahlen ändern.
const SCHLECHT_FAKTOR: f64 = 3.0;
const ANRUF_P95_MS: f64 = 100.0;
const ANRUF_JITTER_MS: f64 = 30.0;
const ANRUF_VERLUST: f64 = 1.0;
const SPIEL_MEDIAN_MS: f64 = 50.0;
const SPIEL_JITTER_MS: f64 = 15.0;
const SPIEL_VERLUST: f64 = 0.5;
const STREAMING_VERLUST: f64 = 2.0;

/// Eine Sonde als Ereignis, für Kanäle (Kommandozeile, später Menüleiste). `zeit` in ms seit 1970.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SerienStichprobe {
    pub ziel: String,
    pub laufzeit_ms: Option<f32>,
    pub zeit: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kennzahlen {
    pub gesendet: u32,
    pub verlust_prozent: f64,
    pub min_ms: Option<f64>,
    pub median_ms: Option<f64>,
    pub p5_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mittlere absolute Differenz aufeinanderfolgender Laufzeiten (RFC 3550 vereinfacht).
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stufe {
    Gut,
    Eingeschraenkt,
    Schlecht,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Eignung {
    pub anruf: Stufe,
    pub spiel: Stufe,
    pub streaming: Stufe,
}

/// Wert unter der Grenze: gut. Fehlt der Wert (nie eine Antwort): schlecht. Die schlechteste Stufe zählt.
fn stufe(paare: &[(Option<f64>, f64)]) -> Stufe {
    paare
        .iter()
        .map(|&(w, g)| match w {
            Some(w) if w < g => Stufe::Gut,
            Some(w) if w < g * SCHLECHT_FAKTOR => Stufe::Eingeschraenkt,
            _ => Stufe::Schlecht,
        })
        .max()
        .unwrap_or(Stufe::Schlecht)
}

pub fn einstufen(k: &Kennzahlen) -> Eignung {
    let v = Some(k.verlust_prozent);
    Eignung {
        anruf: stufe(&[
            (k.p95_ms, ANRUF_P95_MS),
            (k.jitter_ms, ANRUF_JITTER_MS),
            (v, ANRUF_VERLUST),
        ]),
        spiel: stufe(&[
            (k.median_ms, SPIEL_MEDIAN_MS),
            (k.jitter_ms, SPIEL_JITTER_MS),
            (v, SPIEL_VERLUST),
        ]),
        streaming: stufe(&[(v, STREAMING_VERLUST)]),
    }
}

/// Die letzten `RING` Sonden eines Ziels: (Zeit in ms, Laufzeit in ms; `None` = keine Antwort).
#[derive(Debug, Default)]
pub struct Serie {
    proben: VecDeque<(u64, Option<f64>)>,
    ip: Option<Ipv4Addr>,
    seq: u16,
    letzte_ablage_ms: u64,
}

impl Serie {
    pub fn push(&mut self, zeit: u64, ms: Option<f64>) {
        self.proben.push_back((zeit, ms));
        if self.proben.len() > RING {
            self.proben.pop_front();
        }
    }

    /// Kennzahlen der letzten 60 s vor `jetzt` (ms). Nur gesendete Sonden zählen: Wo nichts
    /// gesendet wurde (Ruhezustand), gibt es keinen Verlust.
    pub fn kennzahlen(&self, jetzt: u64) -> Option<Kennzahlen> {
        let von = jetzt.saturating_sub(FENSTER_MS);
        let w: Vec<_> = self.proben.iter().filter(|p| p.0 >= von).collect();
        if w.is_empty() {
            return None;
        }
        let ms: Vec<f64> = w.iter().filter_map(|p| p.1).collect();
        let diffs: Vec<f64> = w
            .windows(2)
            .filter_map(|p| match (p[0].1, p[1].1) {
                (Some(a), Some(b)) if p[1].0.saturating_sub(p[0].0) <= LUECKE_MS => {
                    Some((a - b).abs())
                }
                _ => None,
            })
            .collect();
        // ohne ein Paar (eine einzige Antwort) gibt es kein Zittern: 0
        let jitter =
            (!ms.is_empty()).then(|| diffs.iter().sum::<f64>() / diffs.len().max(1) as f64);
        Some(Kennzahlen {
            gesendet: w.len() as u32,
            verlust_prozent: 100.0 * (w.len() - ms.len()) as f64 / w.len() as f64,
            min_ms: ms.iter().copied().reduce(f64::min),
            median_ms: median(&ms),
            p5_ms: perzentil(&ms, 5),
            p95_ms: perzentil(&ms, 95),
            max_ms: ms.iter().copied().reduce(f64::max),
            jitter_ms: jitter,
        })
    }
}

// ---- Sonde -------------------------------------------------------------------------------

/// Schickt ein Echo mit `seq` und wartet bis zum Zeitlimit auf genau diese Antwort des Ziels.
pub(crate) fn sonde_einmal(
    p: &mut dyn Prober,
    ziel: Ipv4Addr,
    seq: u16,
    limit: Duration,
) -> Option<f64> {
    let t0 = Instant::now();
    p.send(64, seq).ok()?;
    loop {
        let rest = limit.checked_sub(t0.elapsed())?;
        let (von, r) = p.recv(rest)?;
        if r.echo && r.seq == seq && von == ziel {
            return Some(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

fn jetzt_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// ---- Speicher ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fenster {
    pub zeit: u64,
    #[serde(flatten)]
    pub kennzahlen: Kennzahlen,
}

/// Netzschlüssel -> Ziel -> Fenster.
type Store = BTreeMap<String, BTreeMap<String, Vec<Fenster>>>;

/// Hängt `f` an, wenn das letzte Fenster mindestens eine Minute alt ist, und wirft alte weg.
fn anhaengen(liste: &mut Vec<Fenster>, f: Fenster) {
    liste.retain(|x| x.zeit + AUFBEWAHREN_S >= f.zeit);
    if liste.last().is_none_or(|l| f.zeit >= l.zeit + ABLAGE_S) {
        liste.push(f);
    }
}

/// Speicherdatei und Schlüssel des aktuellen Netzes, wenn es gemerkt ist.
fn speicher() -> Option<(PathBuf, String)> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let pfad = netze::data_path().ok()?;
    netze::find(&netze::load(&pfad).ok()?, &fp)?;
    Some((pfad.with_file_name("live.json"), fp.key()))
}

fn ablegen(ziel: &str, k: &Kennzahlen) -> Result<(), String> {
    let Some((pfad, key)) = speicher() else {
        return Ok(()); // unbekanntes Netz: nichts speichern
    };
    let mut store: Store = netze::load_json(&pfad)?;
    let liste = store
        .entry(key)
        .or_default()
        .entry(ziel.to_string())
        .or_default();
    anhaengen(
        liste,
        Fenster {
            zeit: netze::now(),
            kennzahlen: k.clone(),
        },
    );
    netze::save(&pfad, &store)
}

// ---- App ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Punkt {
    pub zeit: u64,
    pub ms: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Ansicht {
    pub ziel: String,
    pub ziel_ip: Ipv4Addr,
    /// Die letzten 5 Minuten.
    pub punkte: Vec<Punkt>,
    pub kennzahlen: Option<Kennzahlen>,
    pub eignung: Option<Eignung>,
}

static SERIEN: Mutex<BTreeMap<String, Serie>> = Mutex::new(BTreeMap::new());

/// Eine Sonde für die App. Die Ansicht ruft das einmal pro Sekunde auf, solange sie sichtbar ist;
/// `neu` beginnt die Serie des Ziels von vorn (Knopf „Start“).
pub fn sonde(ziel: &str, neu: bool) -> Result<Ansicht, String> {
    let ziel = ziel.trim();
    let bekannt = SERIEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(ziel)
        .and_then(|s| s.ip)
        .filter(|_| !neu);
    let ip = match bekannt {
        Some(ip) => ip,
        None => resolve4(ziel)?,
    };
    let seq = {
        let mut m = SERIEN.lock().unwrap_or_else(PoisonError::into_inner);
        let s = m.entry(ziel.to_string()).or_default();
        if neu {
            *s = Serie::default();
        }
        s.ip = Some(ip);
        s.seq = s.seq.wrapping_add(1);
        s.seq
    };
    let mut icmp = Icmp::open(ip)?;
    let zeit = jetzt_ms();
    let ms = sonde_einmal(&mut icmp, ip, seq, ZEITLIMIT);
    let mut m = SERIEN.lock().unwrap_or_else(PoisonError::into_inner);
    let s = m.entry(ziel.to_string()).or_default();
    s.push(zeit, ms);
    let jetzt = jetzt_ms();
    let kennzahlen = s.kennzahlen(jetzt);
    if let Some(k) = kennzahlen
        .as_ref()
        .filter(|_| jetzt >= s.letzte_ablage_ms + ABLAGE_S * 1000)
    {
        s.letzte_ablage_ms = jetzt;
        // Die Messung läuft weiter, auch wenn der Speicher klemmt.
        let _ = ablegen(ziel, k);
    }
    Ok(Ansicht {
        ziel: ziel.to_string(),
        ziel_ip: ip,
        punkte: s
            .proben
            .iter()
            .filter(|p| p.0 + ANSICHT_MS >= jetzt)
            .map(|&(zeit, ms)| Punkt { zeit, ms })
            .collect(),
        eignung: kennzahlen.as_ref().map(einstufen),
        kennzahlen,
    })
}

// ---- Kommandozeile -----------------------------------------------------------------------

const BLOECKE: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Die letzten `breite` Werte als Blöcke (Höhe relativ zum größten Wert); „×“ = keine Antwort.
fn kurve(proben: &VecDeque<(u64, Option<f64>)>, breite: usize) -> String {
    let skip = proben.len().saturating_sub(breite);
    let max = proben
        .iter()
        .skip(skip)
        .filter_map(|p| p.1)
        .fold(1.0, f64::max);
    proben
        .iter()
        .skip(skip)
        .map(|p| {
            p.1.map_or('×', |v| BLOECKE[((v / max) * 7.0).round() as usize])
        })
        .collect()
}

fn wort(s: Stufe) -> &'static str {
    match s {
        Stufe::Gut => "gut",
        Stufe::Eingeschraenkt => "mit Einschränkung",
        Stufe::Schlecht => "schlecht",
    }
}

fn zeilen(ziel: &str, s: &Serie) -> [String; 2] {
    let Some(k) = s.kennzahlen(jetzt_ms()) else {
        return [
            format!("{ziel:<16} wartet auf die erste Antwort …"),
            String::new(),
        ];
    };
    let ms = |v: Option<f64>| v.map_or("-".into(), |x| format!("{x:.1} ms"));
    let e = einstufen(&k);
    [
        format!(
            "{ziel:<16} {}  Median {}  P95 {}  Jitter {}  Verlust {:.1} %",
            kurve(&s.proben, 40),
            ms(k.median_ms),
            ms(k.p95_ms),
            ms(k.jitter_ms),
            k.verlust_prozent
        ),
        format!(
            "{:<16} Videoanruf: {}  ·  Spiele: {}  ·  Streaming: {}",
            "",
            wort(e.anruf),
            wort(e.spiel),
            wort(e.streaming)
        ),
    ]
}

/// `netzinfo live <ziel>... [--sekunden N] [--intervall MS]`
pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let (mut ziele, mut sekunden, mut intervall) = (Vec::new(), 30u64, 1000u64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut zahl = |flag: &str, von: u64, bis: u64| {
            it.next()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| (von..=bis).contains(n))
                .ok_or(format!(
                    "Nach `{flag}` gehört eine ganze Zahl von {von} bis {bis}."
                ))
        };
        match a.as_str() {
            "--sekunden" => sekunden = zahl("--sekunden", 1, 3600)?,
            "--intervall" => intervall = zahl("--intervall", MIN_INTERVALL_MS, 60_000)?,
            f if f.starts_with("--") => return Err(format!("Unbekannte Option „{f}“.")),
            _ => ziele.push(a.as_str()),
        }
    }
    if !(1..=4).contains(&ziele.len()) {
        return Err("Gib ein bis vier Ziele an: `netzinfo live 1.1.1.1 9.9.9.9`.".into());
    }
    let mut sender = Vec::new();
    for z in &ziele {
        let ip = resolve4(z)?;
        sender.push((*z, ip, Icmp::open(ip)?));
    }
    println!(
        "Messe {sekunden} s lang alle {intervall} ms. × heißt: keine Antwort. Ende mit Strg-C."
    );
    explain(
        explain_on,
        "Ich schicke dem Ziel regelmäßig ein kleines Ping-Paket (ICMP Echo) und messe, wie lange die Antwort braucht. Das braucht kein sudo. Jitter ist das Zittern dieser Zeit: Bei Videoanrufen führt er zu abgehackter Stimme, auch wenn der Durchschnitt gut aussieht. Verlust sind Pakete ohne Antwort nach 2 s.",
    );
    explain(
        explain_on,
        "Die Eignung ist eine Faustregel: Videoanruf gut bei P95 unter 100 ms, Jitter unter 30 ms, Verlust unter 1 %. Spiele gut bei Median unter 50 ms, Jitter unter 15 ms, Verlust unter 0,5 %. Streaming gut bei Verlust unter 2 %. Ein Ping wird von Routern manchmal langsamer beantwortet als echter Verkehr.",
    );
    let mut serien: Vec<Serie> = ziele.iter().map(|_| Serie::default()).collect();
    let tty = std::io::stdout().is_terminal();
    let ende = Instant::now() + Duration::from_secs(sekunden);
    let takt = Duration::from_millis(intervall);
    let (tx, rx) = mpsc::channel::<(usize, SerienStichprobe)>();
    std::thread::scope(|sc| {
        for (i, (z, ip, mut icmp)) in sender.into_iter().enumerate() {
            let tx = tx.clone();
            sc.spawn(move || {
                for seq in 0u16.. {
                    let t0 = Instant::now();
                    if t0 >= ende {
                        break;
                    }
                    let zeit = jetzt_ms();
                    let ms = sonde_einmal(&mut icmp, ip, seq, ZEITLIMIT);
                    let s = SerienStichprobe {
                        ziel: z.to_string(),
                        laufzeit_ms: ms.map(|m| m as f32),
                        zeit,
                    };
                    if tx.send((i, s)).is_err() {
                        break;
                    }
                    std::thread::sleep(takt.saturating_sub(t0.elapsed()));
                }
            });
        }
        drop(tx);
        let mut gezeichnet = false;
        for (i, s) in rx {
            serien[i].push(s.zeit, s.laufzeit_ms.map(f64::from));
            if tty {
                if gezeichnet {
                    print!("\x1b[{}A", ziele.len() * 2);
                }
                for (z, s) in ziele.iter().zip(&serien) {
                    for l in zeilen(z, s) {
                        println!("\x1b[2K{l}");
                    }
                }
                gezeichnet = true;
            }
        }
    });
    if !tty {
        for (z, s) in ziele.iter().zip(&serien) {
            zeilen(z, s).iter().for_each(|l| println!("{l}"));
        }
    }
    for (z, s) in ziele.iter().zip(&serien) {
        if let Some(k) = s.kennzahlen(jetzt_ms()) {
            // Der Lauf ist vorbei; ein klemmender Speicher ändert das Ergebnis nicht.
            let _ = ablegen(z, &k);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pfad::Reply;

    fn serie(werte: &[Option<f64>], abstand: u64) -> Serie {
        let mut s = Serie::default();
        for (i, w) in werte.iter().enumerate() {
            s.push(i as u64 * abstand, *w);
        }
        s
    }

    #[test]
    fn kennzahlen_aus_fester_reihe() {
        let s = serie(
            &[Some(10.0), Some(20.0), Some(10.0), None, Some(30.0)],
            1000,
        );
        let k = s.kennzahlen(4000).unwrap();
        assert_eq!(k.gesendet, 5);
        assert_eq!(k.verlust_prozent, 20.0);
        assert_eq!(k.min_ms, Some(10.0));
        assert_eq!(k.median_ms, Some(15.0));
        assert_eq!(k.p5_ms, Some(10.0));
        assert_eq!(k.p95_ms, Some(30.0));
        assert_eq!(k.max_ms, Some(30.0));
        // (10,20) und (20,10) zählen, die Paare mit dem Verlust nicht
        assert_eq!(k.jitter_ms, Some(10.0));
    }

    #[test]
    fn alles_verloren_ist_schlecht() {
        let k = serie(&[None, None], 1000).kennzahlen(1000).unwrap();
        assert_eq!(
            (k.verlust_prozent, k.median_ms, k.jitter_ms),
            (100.0, None, None)
        );
        assert_eq!(einstufen(&k).streaming, Stufe::Schlecht);
        assert_eq!(einstufen(&k).anruf, Stufe::Schlecht);
    }

    #[test]
    fn ringpuffer_laeuft_bei_600_ueber() {
        let mut s = Serie::default();
        for i in 0..700u64 {
            s.push(i, Some(i as f64));
        }
        assert_eq!(s.proben.len(), RING);
        assert_eq!(s.proben.front().unwrap().0, 100);
        assert_eq!(s.proben.back().unwrap().0, 699);
        assert_eq!(s.kennzahlen(699).unwrap().gesendet, 600);
        // Das Fenster schneidet nach Zeit: Abstand 1 s, jetzt 699 s -> Werte ab 639 s
        let s = serie(&vec![Some(5.0); 700], 1000);
        assert_eq!(s.kennzahlen(699_000).unwrap().gesendet, 61);
    }

    #[test]
    fn ruhezustand_ist_kein_verlust() {
        // Zwei Antworten, dazwischen eine Stunde ohne gesendete Sonde.
        let mut s = Serie::default();
        s.push(0, Some(10.0));
        s.push(3_600_000, Some(90.0));
        let k = s.kennzahlen(3_600_000).unwrap();
        assert_eq!((k.gesendet, k.verlust_prozent), (1, 0.0)); // der alte Wert liegt außerhalb
        // Liegen beide im Fenster, aber weiter auseinander als LUECKE_MS: kein Jitter aus der Lücke.
        let mut s = Serie::default();
        s.push(40_000, Some(10.0));
        s.push(55_000, Some(90.0));
        let k = s.kennzahlen(60_000).unwrap();
        assert_eq!((k.verlust_prozent, k.jitter_ms), (0.0, Some(0.0)));
    }

    #[test]
    fn einstufung_an_den_schwellen() {
        let k = |p95, med, jit, verl| Kennzahlen {
            gesendet: 100,
            verlust_prozent: verl,
            min_ms: Some(1.0),
            median_ms: Some(med),
            p5_ms: Some(1.0),
            p95_ms: Some(p95),
            max_ms: Some(p95),
            jitter_ms: Some(jit),
        };
        use Stufe::*;
        let st = |e: Eignung| (e.anruf, e.spiel, e.streaming);
        assert_eq!(st(einstufen(&k(99.9, 49.9, 14.9, 0.4))), (Gut, Gut, Gut));
        // genau auf der Grenze ist nicht mehr „gut“
        assert_eq!(
            st(einstufen(&k(100.0, 40.0, 10.0, 0.5))),
            (Eingeschraenkt, Eingeschraenkt, Gut)
        );
        // Jitter 20 ms: Anruf gut, Spiel mit Einschränkung
        assert_eq!(
            st(einstufen(&k(50.0, 40.0, 20.0, 0.0))),
            (Gut, Eingeschraenkt, Gut)
        );
        // 4 % Verlust: alles schlecht außer Streaming (Grenze 2 %, schlecht ab 6 %)
        assert_eq!(
            st(einstufen(&k(50.0, 40.0, 5.0, 4.0))),
            (Schlecht, Schlecht, Eingeschraenkt)
        );
    }

    struct Fake(Vec<(Ipv4Addr, Reply)>);
    impl Prober for Fake {
        fn send(&mut self, _: u8, _: u16) -> Result<(), String> {
            Ok(())
        }
        fn recv(&mut self, _: Duration) -> Option<(Ipv4Addr, Reply)> {
            (!self.0.is_empty()).then(|| self.0.remove(0))
        }
    }

    #[test]
    fn sonde_nimmt_nur_die_passende_antwort() {
        let ziel = Ipv4Addr::new(192, 0, 2, 1);
        let fremd = Ipv4Addr::new(192, 0, 2, 99);
        let r = |seq, echo| Reply { seq, echo };
        let mut p = Fake(vec![
            (ziel, r(6, true)),
            (fremd, r(7, true)),
            (ziel, r(7, false)),
            (ziel, r(7, true)),
        ]);
        assert!(sonde_einmal(&mut p, ziel, 7, Duration::from_secs(1)).is_some());
        assert!(p.0.is_empty());
        let mut leer = Fake(vec![(ziel, r(1, true))]);
        assert_eq!(
            sonde_einmal(&mut leer, ziel, 7, Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn speicher_hoechstens_eine_minute_und_sieben_tage() {
        let f = |zeit| Fenster {
            zeit,
            kennzahlen: serie(&[Some(1.0)], 1).kennzahlen(0).unwrap(),
        };
        let mut l = Vec::new();
        let tag = 86_400;
        for s in (0..tag).step_by(10) {
            anhaengen(&mut l, f(1_000_000 + s));
        }
        assert_eq!(l.len(), 1440);
        anhaengen(&mut l, f(1_000_000 + 8 * tag));
        assert_eq!(l.len(), 1); // alles vom ersten Tag ist älter als sieben Tage
    }

    #[test]
    fn kurve_zeigt_verlust_und_hoehe() {
        let s = serie(&[Some(1.0), Some(8.0), None, Some(4.0)], 1);
        assert_eq!(kurve(&s.proben, 40), "▂█×▅");
        assert_eq!(kurve(&s.proben, 2), "×█");
    }
}
