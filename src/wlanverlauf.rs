//! WLAN-Verlauf: Proben, Roaming und Abbrüche pro Netz. Issue #36.
//!
//! Alle 5 Sekunden eine Probe, im Speicher ein Ringpuffer (eine Stunde). Auf Platte kommen nur
//! Zusammenfassungen je Besuch und die letzten Ereignisse, nie Einzelproben, und nur für gemerkte
//! Netze (`wlanverlauf.json` neben `netze.json`). Ohne Standortfreigabe fehlt der BSSID-Hash:
//! Roaming ist dann nicht erkennbar, Abbrüche schon.

use crate::{explain, hier, netze, ort, wlan};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const TAKT_S: u64 = 5;
const PUFFER: usize = 720;
const MAX_BESUCHE: usize = 50;
const MAX_EREIGNISSE: usize = 200;
/// Länger ohne Probe: die Ansicht war weg, die nächste Probe beginnt einen neuen Besuch.
const LUECKE_S: u64 = 60;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Probe {
    /// Unix-Sekunden.
    pub zeit: u64,
    /// `false`: keine Funkverbindung (Kabel, WLAN aus, getrennt).
    pub verbunden: bool,
    pub rssi: Option<i32>,
    pub rauschen: Option<i32>,
    pub rate: Option<f64>,
    pub band: Option<String>,
    /// Gesalzener Hash der BSSID, nur mit Standortfreigabe.
    pub bssid_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Art {
    Roaming,
    Getrennt,
    Verbunden,
    Bandwechsel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ereignis {
    pub zeit: u64,
    pub art: Art,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spanne {
    pub min: f64,
    pub median: f64,
    pub max: f64,
}

/// Zusammenfassung eines Besuchs. `von` (Unix-Sekunden) ist zugleich seine Kennung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Besuch {
    pub von: u64,
    pub dauer_s: u64,
    pub signal: Option<Spanne>,
    pub rate: Option<Spanne>,
    pub wechsel: u32,
    pub abbrueche: u32,
    /// `false`: kein BSSID-Hash gesehen, Roaming war nicht erkennbar.
    pub roaming_erkennbar: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetzVerlauf {
    pub besuche: Vec<Besuch>,
    pub ereignisse: Vec<Ereignis>,
}

type Store = BTreeMap<String, NetzVerlauf>;

/// Was zwischen zwei Proben passiert ist. Roaming nur ohne `Getrennt` dazwischen.
fn zwischen(a: &Probe, b: &Probe) -> Vec<Art> {
    match (a.verbunden, b.verbunden) {
        (true, false) => vec![Art::Getrennt],
        (false, true) => vec![Art::Verbunden],
        (false, false) => vec![],
        (true, true) => {
            let mut v = vec![];
            if a.bssid_hash.is_some() && b.bssid_hash.is_some() && a.bssid_hash != b.bssid_hash {
                v.push(Art::Roaming);
            }
            if a.band.is_some() && b.band.is_some() && a.band != b.band {
                v.push(Art::Bandwechsel);
            }
            v
        }
    }
}

/// Ereignisse einer Probenfolge, reine Funktion.
pub fn erkennen(proben: &[Probe]) -> Vec<Ereignis> {
    proben
        .windows(2)
        .flat_map(|w| {
            zwischen(&w[0], &w[1]).into_iter().map(|art| Ereignis {
                zeit: w[1].zeit,
                art,
            })
        })
        .collect()
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

fn spanne(v: Vec<f64>) -> Option<Spanne> {
    let min = v.iter().copied().min_by(f64::total_cmp)?;
    let max = v.iter().copied().max_by(f64::total_cmp)?;
    Some(Spanne {
        min,
        median: median(v)?,
        max,
    })
}

/// Ein laufender Besuch: Ringpuffer mit den letzten 720 Proben plus Zähler für den ganzen Besuch.
pub struct Verlauf {
    von: u64,
    proben: VecDeque<Probe>,
    ereignisse: Vec<Ereignis>,
    wechsel: u32,
    abbrueche: u32,
    hash_gesehen: bool,
}

impl Verlauf {
    pub fn neu(von: u64) -> Self {
        Self {
            von,
            proben: VecDeque::new(),
            ereignisse: vec![],
            wechsel: 0,
            abbrueche: 0,
            hash_gesehen: false,
        }
    }

    pub fn push(&mut self, p: Probe) -> Vec<Ereignis> {
        let neu: Vec<Ereignis> = self
            .proben
            .back()
            .map(|a| zwischen(a, &p))
            .unwrap_or_default()
            .into_iter()
            .map(|art| Ereignis { zeit: p.zeit, art })
            .collect();
        for e in &neu {
            match e.art {
                Art::Roaming => self.wechsel += 1,
                Art::Getrennt => self.abbrueche += 1,
                _ => {}
            }
        }
        self.hash_gesehen |= p.bssid_hash.is_some();
        self.ereignisse.extend(neu.iter().cloned());
        if self.ereignisse.len() > MAX_EREIGNISSE {
            self.ereignisse
                .drain(..self.ereignisse.len() - MAX_EREIGNISSE);
        }
        self.proben.push_back(p);
        if self.proben.len() > PUFFER {
            self.proben.pop_front();
        }
        neu
    }

    fn letzte_zeit(&self) -> u64 {
        self.proben.back().map_or(self.von, |p| p.zeit)
    }

    /// `None`, solange keine Probe da ist.
    // ponytail: Min/Median/Max rechnen über den Puffer, bei Besuchen über eine Stunde also über
    // die letzte Stunde. Dauer und Zähler gelten für den ganzen Besuch.
    pub fn besuch(&self) -> Option<Besuch> {
        let letzte = self.proben.back()?;
        let werte = |f: fn(&Probe) -> Option<f64>| self.proben.iter().filter_map(f).collect();
        Some(Besuch {
            von: self.von,
            dauer_s: letzte.zeit.saturating_sub(self.von),
            signal: spanne(werte(|p| p.rssi.map(f64::from))),
            rate: spanne(werte(|p| p.rate)),
            wechsel: self.wechsel,
            abbrueche: self.abbrueche,
            roaming_erkennbar: self.hash_gesehen,
        })
    }
}

/// Besuch und Ereignisse ins Netz schreiben. Wiederholbar: derselbe Besuch (`von`) ersetzt sich.
fn speichern_in(store: &mut Store, key: &str, b: Besuch, ereignisse: &[Ereignis]) {
    let nv = store.entry(key.to_string()).or_default();
    let von = b.von;
    match nv.besuche.iter_mut().find(|x| x.von == von) {
        Some(x) => *x = b,
        None => nv.besuche.push(b),
    }
    if nv.besuche.len() > MAX_BESUCHE {
        nv.besuche.drain(..nv.besuche.len() - MAX_BESUCHE);
    }
    nv.ereignisse.retain(|e| e.zeit < von);
    nv.ereignisse.extend(ereignisse.iter().cloned());
    if nv.ereignisse.len() > MAX_EREIGNISSE {
        nv.ereignisse.drain(..nv.ereignisse.len() - MAX_EREIGNISSE);
    }
}

/// „Dieses Mal im Median 6 dB schwächer als sonst hier.“ `None` ohne frühere Besuche mit Signal.
pub fn vergleich(besuche: &[Besuch], jetzt: &Besuch) -> Option<String> {
    let mein = jetzt.signal?.median;
    let sonst: Vec<f64> = besuche
        .iter()
        .filter(|b| b.von != jetzt.von)
        .filter_map(|b| Some(b.signal?.median))
        .collect();
    let n = sonst.len();
    let d = (mein - median(sonst)?).round() as i32;
    Some(match d {
        0 => format!("Dieses Mal im Median so stark wie sonst hier ({n} frühere Besuche)."),
        d if d < 0 => format!(
            "Dieses Mal im Median {} dB schwächer als sonst hier ({n} frühere Besuche).",
            -d
        ),
        d => format!("Dieses Mal im Median {d} dB stärker als sonst hier ({n} frühere Besuche)."),
    })
}

fn dauer(s: u64) -> String {
    if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

fn zahlen(s: Option<Spanne>, einheit: &str) -> String {
    s.map_or("keine Werte".into(), |s| {
        format!(
            "Median {:.0} {einheit} (Minimum {:.0}, Maximum {:.0})",
            s.median, s.min, s.max
        )
    })
}

fn besuch_zeile(b: &Besuch) -> String {
    let roaming = if b.roaming_erkennbar {
        format!("{} Wechsel", b.wechsel)
    } else {
        "Wechsel nicht erkennbar".into()
    };
    format!(
        "Dauer {}, Signal {}, Rate {}, {roaming}, {} Abbrüche",
        dauer(b.dauer_s),
        zahlen(b.signal, "dBm"),
        zahlen(b.rate, "MBit/s"),
        b.abbrueche
    )
}

/// Die Probe zur aktuellen Funkverbindung. Ohne `salz` kein Hash.
fn probe_aus(f: Option<wlan::Funkverbindung>, zeit: u64, salz: Option<&str>) -> Probe {
    let Some(f) = f else {
        return Probe {
            zeit,
            ..Probe::default()
        };
    };
    let hash = f
        .bssid
        .as_deref()
        .and_then(ort::bssid_bytes)
        .zip(salz)
        .map(|(b, s)| ort::bssid_hash(s, b));
    Probe {
        zeit,
        verbunden: true,
        rssi: f.rssi_dbm,
        rauschen: f.rauschen_dbm,
        rate: f.rate_mbit,
        band: f.band,
        bssid_hash: hash,
    }
}

fn probe_lesen(zeit: u64) -> Result<Probe, String> {
    let salz = netze::data_path()
        .ok()
        .and_then(|p| ort::salz(&p.with_file_name("salz.json")).ok());
    Ok(probe_aus(wlan::lesen()?, zeit, salz.as_deref()))
}

type Ziel = Option<(PathBuf, String)>;

/// Speicherdatei und Schlüssel des aktuellen Netzes, wenn es gemerkt ist.
fn ziel_suchen() -> Ziel {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let pfad = netze::data_path().ok()?;
    netze::find(&netze::load(&pfad).ok()?, &fp)?;
    Some((pfad.with_file_name("wlanverlauf.json"), fp.key()))
}

/// Fasst den Besuch zusammen und legt ihn ab, wenn das Netz gemerkt ist.
fn ablegen(ziel: &Ziel, v: &Verlauf) -> Result<Option<Besuch>, String> {
    let Some(b) = v.besuch() else {
        return Ok(None);
    };
    if let Some((pfad, key)) = ziel {
        let mut store: Store = netze::load_json(pfad)?;
        speichern_in(&mut store, key, b.clone(), &v.ereignisse);
        netze::save(pfad, &store)?;
    }
    Ok(Some(b))
}

/// Gespeicherte Besuche und Ereignisse des aktuellen Netzes. Ungemerkt: leer.
pub fn besuche_jetzt() -> Result<NetzVerlauf, String> {
    let Some((pfad, key)) = ziel_suchen() else {
        return Ok(NetzVerlauf::default());
    };
    let mut store: Store = netze::load_json(&pfad)?;
    Ok(store.remove(&key).unwrap_or_default())
}

struct Stand {
    verlauf: Verlauf,
    ziel: Ziel,
}

// ponytail: ein Verlauf für die ganze App. Wechselt das Netz mitten im Besuch, läuft er bis zur
// nächsten Lücke von 60 s weiter; dann ist `ziel_suchen` neu dran.
static STAND: Mutex<Option<Stand>> = Mutex::new(None);

/// Was die Karte in „Qualität“ zeichnet.
#[derive(Debug, Serialize)]
pub struct Ansicht {
    pub verbunden: bool,
    pub proben: Vec<Probe>,
    pub ereignisse: Vec<Ereignis>,
    pub besuch: Option<Besuch>,
    pub vergleich: Option<String>,
    /// Das Netz ist gemerkt, der Besuch wird gespeichert.
    pub gemerkt: bool,
    pub roaming_erkennbar: bool,
    /// Ein Problem mit der Speicherdatei, nie fatal.
    pub hinweis: Option<String>,
}

/// Eine Probe lesen und in den laufenden Besuch einordnen (App, alle 5 Sekunden).
pub fn abtasten() -> Result<Ansicht, String> {
    let zeit = netze::now();
    let probe = probe_lesen(zeit)?;
    let mut g = STAND.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref()
        .is_some_and(|s| zeit.saturating_sub(s.verlauf.letzte_zeit()) > LUECKE_S)
    {
        *g = None;
    }
    let s = g.get_or_insert_with(|| Stand {
        verlauf: Verlauf::neu(zeit),
        ziel: ziel_suchen(),
    });
    s.verlauf.push(probe);
    let (besuch, mut hinweis) = match ablegen(&s.ziel, &s.verlauf) {
        Ok(b) => (b, None),
        Err(e) => (s.verlauf.besuch(), Some(e)),
    };
    let alle = s.ziel.as_ref().map_or(vec![], |(pfad, key)| {
        netze::load_json::<Store>(pfad)
            .map(|mut st| st.remove(key).unwrap_or_default().besuche)
            .unwrap_or_else(|e| {
                hinweis.get_or_insert(e);
                vec![]
            })
    });
    Ok(Ansicht {
        verbunden: s.verlauf.proben.back().is_some_and(|p| p.verbunden),
        proben: s.verlauf.proben.iter().cloned().collect(),
        ereignisse: s.verlauf.ereignisse.clone(),
        vergleich: besuch.as_ref().and_then(|b| vergleich(&alle, b)),
        besuch,
        gemerkt: s.ziel.is_some(),
        roaming_erkennbar: s.verlauf.hash_gesehen,
        hinweis,
    })
}

/// Abschnitt „WLAN“ für `netzinfo bericht`: nur Zahlen, keine Namen, keine Kennungen.
pub fn abschnitt(netzname: &str) -> Option<String> {
    let pfad = netze::data_path().ok()?;
    let netz = netze::load(&pfad)
        .ok()?
        .into_iter()
        .find(|n| n.name == netzname)?;
    let mut store: Store = netze::load_json(&pfad.with_file_name("wlanverlauf.json")).ok()?;
    abschnitt_aus(&store.remove(&netz.fingerprint.key())?)
}

fn abschnitt_aus(nv: &NetzVerlauf) -> Option<String> {
    let letzter = nv.besuche.last()?;
    let mut out = format!("- Besuche gespeichert: {}\n", nv.besuche.len());
    for b in nv.besuche.iter().rev().take(5) {
        out += &format!("- {}: {}\n", netze::format_utc(b.von), besuch_zeile(b));
    }
    if let Some(v) = vergleich(&nv.besuche, letzter) {
        out += &format!("- {v}\n");
    }
    if nv.besuche.iter().any(|b| !b.roaming_erkennbar) {
        out +=
            "- Bei Besuchen ohne Standortfreigabe war Roaming nicht erkennbar, Abbrüche schon.\n";
    }
    Some(out)
}

fn balken(rssi: Option<i32>) -> String {
    let n = rssi.map_or(0, |r| ((r + 90).clamp(0, 60) / 6) as usize);
    format!("{}{}", "█".repeat(n), "░".repeat(10 - n))
}

fn art_name(a: Art) -> &'static str {
    match a {
        Art::Roaming => "Zugangspunkt gewechselt (Roaming)",
        Art::Getrennt => "Verbindung weg",
        Art::Verbunden => "wieder verbunden",
        Art::Bandwechsel => "Band gewechselt",
    }
}

fn minuten_lesen(args: &[String]) -> Result<u64, String> {
    let fehler = || {
        "Aufruf: `netzinfo wlan --verlauf [--minuten N]`, N ist eine ganze Zahl von 1 bis 1440."
            .to_string()
    };
    match args {
        [] => Ok(10),
        [f, n] if f == "--minuten" => n
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=1440).contains(n))
            .ok_or_else(fehler),
        _ => Err(fehler()),
    }
}

/// `netzinfo wlan --verlauf [--minuten N]`
pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let minuten = minuten_lesen(args)?;
    explain(
        explain_on,
        "Ich frage alle 5 Sekunden das WLAN-System des Mac (CoreWLAN) nach Signal, Rauschen und Senderate. Dabei sende ich nichts ins Netz, ich lese nur, was die WLAN-Karte schon weiß. Wechselt die Kennung des Zugangspunkts, zähle ich ein Roaming, fehlt die Verbindung, einen Abbruch.",
    );
    let ziel = ziel_suchen();
    let gesamt = minuten * 60 / TAKT_S;
    println!(
        "Verfolge das WLAN {minuten} min lang, alle {TAKT_S} s. {}",
        if ziel.is_some() {
            "Das Ende speichert den Besuch in diesem gemerkten Netz."
        } else {
            "Dieses Netz ist nicht gemerkt, es wird nichts gespeichert (`netzinfo merken <Name>`)."
        }
    );
    let start = Instant::now();
    let mut v = Verlauf::neu(netze::now());
    for i in 0..=gesamt {
        let p = probe_lesen(netze::now())?;
        let zeile = if p.verbunden {
            format!(
                "{}  {:>4} dBm  {:>5.0} MBit/s",
                balken(p.rssi),
                p.rssi.map_or("?".into(), |r| r.to_string()),
                p.rate.unwrap_or(0.0)
            )
        } else {
            "keine Funkverbindung".to_string()
        };
        let marke: Vec<_> = v.push(p).iter().map(|e| art_name(e.art)).collect();
        let sek = i * TAKT_S;
        println!(
            "+{:02}:{:02}  {zeile}{}",
            sek / 60,
            sek % 60,
            if marke.is_empty() {
                String::new()
            } else {
                format!("  <- {}", marke.join(", "))
            }
        );
        if i < gesamt
            && let Some(rest) = Duration::from_secs((i + 1) * TAKT_S).checked_sub(start.elapsed())
        {
            std::thread::sleep(rest);
        }
    }
    println!("\nEreignisse");
    if v.ereignisse.is_empty() {
        println!("  keine");
    }
    for e in &v.ereignisse {
        let s = e.zeit.saturating_sub(v.von);
        println!("  +{:02}:{:02}  {}", s / 60, s % 60, art_name(e.art));
    }
    let Some(b) = ablegen(&ziel, &v)? else {
        return Ok(());
    };
    println!("\nZusammenfassung\n  {}", besuch_zeile(&b));
    if !b.roaming_erkennbar {
        println!(
            "  Roaming ist ohne Standortfreigabe nicht erkennbar (kein Hash der Kennung), Abbrüche schon. Die Netzinfo-App fragt danach."
        );
    }
    if let Some((pfad, key)) = &ziel {
        let mut st: Store = netze::load_json(pfad)?;
        let alle = st.remove(key).unwrap_or_default().besuche;
        if let Some(t) = vergleich(&alle, &b) {
            println!("  {t}");
            explain(
                explain_on,
                "Ich vergleiche den Median dieses Besuchs mit dem Median der Mediane früherer Besuche in diesem Netz. Ist das Signal hier im Schnitt schwächer als bei deinen letzten Besuchen, lohnt sich ein Blick auf den Standort des Routers.",
            );
        }
    }
    if b.roaming_erkennbar {
        explain(
            explain_on,
            &format!(
                "Der Rechner hat in {minuten} min {} Mal den Zugangspunkt gewechselt. Das ist normal, wenn man sich bewegt, und ein Problem, wenn man am Schreibtisch sitzt.",
                b.wechsel
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(zeit: u64, rssi: i32, hash: &str) -> Probe {
        Probe {
            zeit,
            verbunden: true,
            rssi: Some(rssi),
            rate: Some(600.0),
            band: Some("5 GHz".into()),
            bssid_hash: Some(hash.into()),
            ..Probe::default()
        }
    }

    fn weg(zeit: u64) -> Probe {
        Probe {
            zeit,
            ..Probe::default()
        }
    }

    fn arten(proben: &[Probe]) -> Vec<Art> {
        erkennen(proben).iter().map(|e| e.art).collect()
    }

    #[test]
    fn roaming_wenn_der_hash_wechselt() {
        let e = erkennen(&[p(0, -50, "a"), p(5, -52, "a"), p(10, -60, "b")]);
        assert_eq!(
            e,
            vec![Ereignis {
                zeit: 10,
                art: Art::Roaming
            }]
        );
    }

    #[test]
    fn abbruch_ist_kein_roaming() {
        let e = arten(&[p(0, -50, "a"), weg(5), p(10, -50, "b")]);
        assert_eq!(e, vec![Art::Getrennt, Art::Verbunden]);
    }

    #[test]
    fn ohne_hash_kein_roaming_aber_abbruch() {
        let ohne = |z, r| Probe {
            bssid_hash: None,
            ..p(z, r, "")
        };
        assert_eq!(
            arten(&[ohne(0, -50), ohne(5, -80), weg(10)]),
            vec![Art::Getrennt]
        );
    }

    #[test]
    fn bandwechsel() {
        let mut b = p(5, -50, "a");
        b.band = Some("2,4 GHz".into());
        assert_eq!(arten(&[p(0, -50, "a"), b]), vec![Art::Bandwechsel]);
    }

    #[test]
    fn ringpuffer_laeuft_ueber() {
        let mut v = Verlauf::neu(0);
        for i in 0..(PUFFER as u64 + 50) {
            v.push(p(i * 5, -50, "a"));
        }
        assert_eq!(v.proben.len(), PUFFER);
        assert_eq!(v.proben.front().map(|p| p.zeit), Some(250));
    }

    #[test]
    fn zusammenfassung_stimmt() {
        let mut v = Verlauf::neu(100);
        for (i, r) in [-60, -50, -70, -40].into_iter().enumerate() {
            v.push(p(100 + i as u64 * 5, r, if i < 2 { "a" } else { "b" }));
        }
        v.push(weg(120));
        v.push(p(125, -55, "b"));
        let b = v.besuch().unwrap();
        let s = b.signal.unwrap();
        assert_eq!((s.min, s.median, s.max), (-70.0, -55.0, -40.0));
        assert_eq!((b.wechsel, b.abbrueche, b.dauer_s), (1, 1, 25));
        assert!(b.roaming_erkennbar);
        assert_eq!(b.rate.unwrap().median, 600.0);
    }

    #[test]
    fn zehntausend_proben_schreiben_wenig() {
        let mut v = Verlauf::neu(0);
        for i in 0..10_000u64 {
            v.push(p(i * 5, -50, if i % 2 == 0 { "a" } else { "b" }));
        }
        let mut st = Store::new();
        speichern_in(&mut st, "k", v.besuch().unwrap(), &v.ereignisse);
        assert!(st["k"].ereignisse.len() <= MAX_EREIGNISSE);
        assert_eq!(st["k"].besuche.len(), 1);
        assert!(serde_json::to_string(&st).unwrap().len() < 20_000);
        // derselbe Besuch noch einmal: ersetzt sich, keine Doppelten
        speichern_in(&mut st, "k", v.besuch().unwrap(), &v.ereignisse);
        assert_eq!(st["k"].besuche.len(), 1);
        assert!(st["k"].ereignisse.len() <= MAX_EREIGNISSE);
    }

    #[test]
    fn hoechstens_50_besuche() {
        let mut st = Store::new();
        for i in 0..60 {
            let mut v = Verlauf::neu(i);
            v.push(p(i, -50, "a"));
            speichern_in(&mut st, "k", v.besuch().unwrap(), &v.ereignisse);
        }
        assert_eq!(st["k"].besuche.len(), MAX_BESUCHE);
        assert_eq!(st["k"].besuche[0].von, 10);
    }

    #[test]
    fn besuchsvergleich() {
        let besuch = |von, med| Besuch {
            von,
            dauer_s: 60,
            signal: Some(Spanne {
                min: med,
                median: med,
                max: med,
            }),
            rate: None,
            wechsel: 0,
            abbrueche: 0,
            roaming_erkennbar: true,
        };
        let alle = [besuch(1, -50.0), besuch(2, -50.0), besuch(3, -56.0)];
        let t = vergleich(&alle, &alle[2]).unwrap();
        assert!(t.contains("6 dB schwächer"), "{t}");
        assert!(
            vergleich(&alle, &besuch(4, -45.0))
                .unwrap()
                .contains("stärker")
        );
        assert_eq!(vergleich(&alle[..1], &alle[0]), None);
    }

    #[test]
    fn probe_hat_nie_die_rohe_kennung() {
        let f = wlan::Funkverbindung {
            bssid: Some("00:00:5e:00:53:01".into()),
            rssi_dbm: Some(-55),
            ..Default::default()
        };
        let probe = probe_aus(Some(f.clone()), 1, Some("salz"));
        assert!(!serde_json::to_string(&probe).unwrap().contains("00:00:5e"));
        assert!(probe.bssid_hash.is_some());
        assert_eq!(probe_aus(Some(f), 1, None).bssid_hash, None);
        assert!(!probe_aus(None, 1, Some("salz")).verbunden);
    }

    #[test]
    fn bericht_nur_zahlen() {
        let mut v = Verlauf::neu(1_000);
        v.push(p(1_000, -55, "a"));
        v.push(p(1_005, -57, "b"));
        let mut st = Store::new();
        speichern_in(&mut st, "k", v.besuch().unwrap(), &v.ereignisse);
        let t = abschnitt_aus(&st["k"]).unwrap();
        assert!(
            t.contains("1 Wechsel") && t.contains("Signal Median"),
            "{t}"
        );
    }

    #[test]
    fn minuten_pruefen() {
        let a = |s: &[&str]| minuten_lesen(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&[]), Ok(10));
        assert_eq!(a(&["--minuten", "3"]), Ok(3));
        assert!(a(&["--minuten", "0"]).is_err());
        assert!(a(&["--minuten"]).is_err());
        assert!(a(&["--quatsch"]).is_err());
    }
}
