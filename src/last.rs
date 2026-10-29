//! Reaktionsfähigkeit unter Last mit Apples networkQuality, nur auf Knopfdruck. Issue #49.
//! Der Test belastet die Leitung voll: ohne `Bestaetigung` startet kein Prozess.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{explain, hier, netze};

const PROGRAMM: &str = "/usr/bin/networkQuality";
/// Laufzeit, die networkQuality selbst begrenzt (`-M`).
const LAUFZEIT_S: u64 = 30;
/// Harte Frist: Laufzeit plus Zeit für Konfiguration und Auswertung. Danach wird der Prozess beendet.
const FRIST: Duration = Duration::from_secs(LAUFZEIT_S + 20);
const MAX_VERLAUF: usize = 20;
/// Standard-Gateway des iPhone-Hotspots (Apple-Vorgabe), Hinweis auf Mobilfunk.
const HOTSPOT_GATEWAY: &str = "172.20.10.1";

pub const HINWEIS: &str = "Der Test belastet deine Leitung etwa 30 Sekunden mit voller Geschwindigkeit und kann je nach Anschluss mehrere hundert Megabyte Daten verbrauchen. Der Datenverkehr geht zu Apples Testservern. Nicht im Mobilfunk-Hotspot starten, wenn dein Volumen knapp ist. Ein VPN verfälscht die Werte.";
pub const HOTSPOT_FRAGE: &str = "Dieses Netz sieht nach einem Mobilfunk-Hotspot aus. Der Test kann dort mehrere hundert Megabyte deines Datenvolumens verbrauchen.";

/// Beweis, dass jemand den Test ausdrücklich gestartet hat (Klick oder `--start`).
pub struct Bestaetigung(());

impl Bestaetigung {
    /// `None`, wenn nicht ausdrücklich bestätigt.
    pub fn von(ja: bool) -> Option<Self> {
        ja.then_some(Self(()))
    }
}

/// Mess-Ergebnis; `None`, was networkQuality nicht geliefert hat.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LastErgebnis {
    /// Bit pro Sekunde.
    pub down_bps: Option<f64>,
    pub up_bps: Option<f64>,
    /// Antwortzeit ohne Last in ms.
    pub ruhe_ms: Option<f64>,
    /// Antwortzeit unter Last in ms, aus den Antworten pro Minute je Richtung.
    pub last_down_ms: Option<f64>,
    pub last_up_ms: Option<f64>,
    /// Antworten pro Minute (Round trips Per Minute).
    pub rpm: Option<f64>,
    pub dauer_s: f64,
}

/// Was networkQuality als JSON ausgibt; unbekannte Felder werden ignoriert.
#[derive(Deserialize)]
struct Roh {
    dl_throughput: Option<f64>,
    ul_throughput: Option<f64>,
    base_rtt: Option<f64>,
    responsiveness: Option<f64>,
    dl_responsiveness: Option<f64>,
    ul_responsiveness: Option<f64>,
}

fn rpm_zu_ms(rpm: Option<f64>) -> Option<f64> {
    rpm.filter(|r| *r > 0.0).map(|r| 60_000.0 / r)
}

pub fn parse(json: &str, dauer_s: f64) -> Result<LastErgebnis, String> {
    let r: Roh = serde_json::from_str(json.trim()).map_err(|_| {
        let anfang: String = json.trim().chars().take(120).collect();
        format!("networkQuality hat keine lesbare Antwort geliefert („{anfang}“). Prüfe die Verbindung und versuche es noch mal.")
    })?;
    if r.dl_throughput.is_none() && r.ul_throughput.is_none() && r.responsiveness.is_none() {
        return Err("networkQuality hat weder Geschwindigkeit noch Antwortzeit gemeldet. Prüfe die Verbindung und versuche es noch mal.".into());
    }
    Ok(LastErgebnis {
        down_bps: r.dl_throughput,
        up_bps: r.ul_throughput,
        ruhe_ms: r.base_rtt,
        last_down_ms: rpm_zu_ms(r.dl_responsiveness),
        last_up_ms: rpm_zu_ms(r.ul_responsiveness),
        rpm: r.responsiveness,
        dauer_s,
    })
}

/// Selbst festgelegte Einstufung nach RPM.
pub fn einstufung(rpm: f64) -> &'static str {
    match rpm {
        r if r >= 800.0 => "sehr reaktionsfähig",
        r if r >= 200.0 => "gut",
        r if r >= 100.0 => "mäßig",
        _ => "schlecht",
    }
}

/// Aufblähung: schlechtere Antwortzeit unter Last minus Antwortzeit ohne Last, in ms.
pub fn aufblaehung_ms(e: &LastErgebnis) -> Option<f64> {
    let last = match (e.last_down_ms, e.last_up_ms) {
        (Some(d), Some(u)) => d.max(u),
        (d, u) => d.or(u)?,
    };
    Some((last - e.ruhe_ms?).max(0.0))
}

/// Läuft `programm` höchstens `frist`; gibt stdout zurück. Danach wird es beendet.
fn starten(programm: &str, args: &[&str], frist: Duration) -> Result<String, String> {
    let mut kind = Command::new(programm)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("{programm} ließ sich nicht starten ({e}). networkQuality gibt es ab macOS 12.")
        })?;
    let lesen = |mut p: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = p.read_to_string(&mut s);
            s
        })
    };
    let (out, err) = match (kind.stdout.take(), kind.stderr.take()) {
        (Some(o), Some(e)) => (lesen(Box::new(o)), lesen(Box::new(e))),
        _ => return Err("Die Ausgabe von networkQuality ließ sich nicht abgreifen.".into()),
    };
    let start = Instant::now();
    let status = loop {
        match kind.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < frist => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = kind.kill();
                let _ = kind.wait();
                return Err(format!(
                    "Der Test hat nach {} Sekunden nicht geantwortet und wurde beendet. Prüfe die Verbindung und versuche es noch mal.",
                    frist.as_secs_f64().round()
                ));
            }
            Err(e) => return Err(format!("Der Test ließ sich nicht überwachen ({e}).")),
        }
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    if status.success() {
        return Ok(out);
    }
    let grund = [err.trim(), out.trim()]
        .into_iter()
        .find_map(|t| t.lines().next().filter(|l| !l.is_empty()))
        .unwrap_or("keine Meldung");
    Err(format!(
        "networkQuality ist fehlgeschlagen: {grund}. Prüfe die Verbindung und versuche es noch mal."
    ))
}

/// Ist dieses Netz wahrscheinlich ein Mobilfunk-Hotspot (Schnittstelle `pdp_ip*`, iPhone-Gateway)?
pub fn hotspot() -> bool {
    hier::collect().is_ok_and(|n| {
        n.interface.starts_with("pdp_ip")
            || n.router.is_some_and(|r| r.to_string() == HOTSPOT_GATEWAY)
    })
}

/// Der echte Test. Nur macOS; belastet die Leitung etwa 30 Sekunden.
pub fn messen(_ok: &Bestaetigung) -> Result<LastErgebnis, String> {
    if !cfg!(target_os = "macos") {
        return Err(
            "Die Messung unter Last gibt es bisher nur auf dem Mac (networkQuality ab macOS 12)."
                .into(),
        );
    }
    let start = Instant::now();
    let m = LAUFZEIT_S.to_string();
    let json = starten(PROGRAMM, &["-c", "-M", &m], FRIST)?;
    parse(&json, start.elapsed().as_secs_f64())
}

/// Ein gespeicherter Lauf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lauf {
    pub zeit: u64,
    pub ergebnis: LastErgebnis,
}

type Store = HashMap<String, Vec<Lauf>>;

fn ablegen(store: &mut Store, key: &str, e: LastErgebnis, zeit: u64) {
    let l = store.entry(key.to_string()).or_default();
    l.push(Lauf { zeit, ergebnis: e });
    let zu_viel = l.len().saturating_sub(MAX_VERLAUF);
    l.drain(..zu_viel);
}

/// Speicherdatei und Schlüssel des aktuellen Netzes, wenn es gemerkt ist.
fn ziel() -> Option<(PathBuf, String)> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let pfad = netze::data_path().ok()?;
    netze::find(&netze::load(&pfad).ok()?, &fp)?;
    Some((pfad.with_file_name("last.json"), fp.key()))
}

/// Für die App: Hotspot-Verdacht und Verlauf des aktuellen Netzes, ohne Test.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Vorab {
    pub hinweis: &'static str,
    pub hotspot: bool,
    pub hotspot_frage: &'static str,
    pub verlauf: Vec<Lauf>,
}

pub fn vorab() -> Vorab {
    Vorab {
        hinweis: HINWEIS,
        hotspot: hotspot(),
        hotspot_frage: HOTSPOT_FRAGE,
        verlauf: ziel()
            .and_then(|(p, k)| netze::load_json::<Store>(&p).ok()?.remove(&k))
            .unwrap_or_default(),
    }
}

/// Für die App: Test starten (Klick) und im gemerkten Netz speichern. In einem Hotspot nur mit
/// `trotz_hotspot`.
pub fn messen_und_speichern(trotz_hotspot: bool) -> Result<Lauf, String> {
    if hotspot() && !trotz_hotspot {
        return Err(HOTSPOT_FRAGE.into());
    }
    let ok = Bestaetigung::von(true).ok_or("Nicht bestätigt.")?;
    let lauf = Lauf {
        ergebnis: messen(&ok)?,
        zeit: netze::now(),
    };
    if let Some((pfad, key)) = ziel() {
        let mut store: Store = netze::load_json(&pfad)?;
        ablegen(&mut store, &key, lauf.ergebnis.clone(), lauf.zeit);
        netze::save(&pfad, &store)?;
    }
    Ok(lauf)
}

/// Abschnitt „Unter Last“ für `netzinfo bericht`.
pub fn abschnitt(netzname: &str) -> Option<String> {
    let pfad = netze::data_path().ok()?;
    let netz = netze::load(&pfad)
        .ok()?
        .into_iter()
        .find(|n| n.name == netzname)?;
    let mut store: Store = netze::load_json(&pfad.with_file_name("last.json")).ok()?;
    let v = store.remove(&netz.fingerprint.key())?;
    (!v.is_empty()).then(|| {
        v.iter()
            .rev()
            .take(5)
            .map(|l| format!("- {}: {}\n", netze::format_utc(l.zeit), zeile(&l.ergebnis)))
            .collect()
    })
}

fn mbit(bps: Option<f64>) -> String {
    bps.map_or("-".into(), |b| format!("{:.0} Mbit/s", b / 1e6))
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:.0} ms"))
}

/// Eine Zeile: Geschwindigkeit, Antwortzeiten, Einstufung.
pub fn zeile(e: &LastErgebnis) -> String {
    format!(
        "Down {}, Up {}, Ruhe {}, unter Last {} / {}, {}",
        mbit(e.down_bps),
        mbit(e.up_bps),
        ms(e.ruhe_ms),
        ms(e.last_down_ms),
        ms(e.last_up_ms),
        e.rpm.map_or("keine RPM".into(), |r| format!(
            "{r:.0} RPM ({})",
            einstufung(r)
        ))
    )
}

fn ausgeben(e: &LastErgebnis, explain_on: bool) {
    println!("Download:             {}", mbit(e.down_bps));
    println!("Upload:               {}", mbit(e.up_bps));
    println!("Antwortzeit in Ruhe:  {}", ms(e.ruhe_ms));
    println!("Unter Last (Down):    {}", ms(e.last_down_ms));
    println!("Unter Last (Up):      {}", ms(e.last_up_ms));
    match e.rpm {
        Some(r) => println!("Antworten pro Minute: {r:.0}, {}", einstufung(r)),
        None => println!("Antworten pro Minute: nicht gemeldet"),
    }
    if let Some(a) = aufblaehung_ms(e) {
        println!("Aufblähung:           {a:.0} ms");
    }
    if let (Some(r), Some(l)) = (e.ruhe_ms, e.last_down_ms) {
        explain(
            explain_on,
            &format!(
                "Ohne Last antwortet dein Netz in {r:.0} ms, bei vollem Download in {l:.0} ms. Wenn jemand im Haus streamt, ruckeln Anrufe, sobald dieser Wert hoch ist."
            ),
        );
    }
    explain(
        explain_on,
        "Steigt die Antwortzeit unter Last stark, heißt das Bufferbloat: der Router staut Pakete in einem zu großen Zwischenspeicher. Einstufung nach Antworten pro Minute (RPM): ab 800 sehr reaktionsfähig, 200 bis 799 gut, 100 bis 199 mäßig, darunter schlecht.",
    );
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let usage = "Aufruf: netzinfo last --start [--trotzdem]";
    let (mut start, mut trotzdem) = (false, false);
    for a in args {
        match a.as_str() {
            "--start" => start = true,
            "--trotzdem" => trotzdem = true,
            _ => return Err(format!("Unbekannte Angabe „{a}“. {usage}")),
        }
    }
    explain(
        explain_on,
        "Ich rufe das in macOS eingebaute Programm networkQuality auf. Es lädt etwa 30 Sekunden lang so schnell wie möglich herunter und hoch und misst dabei zugleich, wie lange kleine Anfragen brauchen. So zeigt sich, ob dein Anschluss unter Last einbricht, zum Beispiel beim Foto-Upload während eines Anrufs. Ohne --start passiert nichts.",
    );
    let Some(ok) = Bestaetigung::von(start) else {
        println!("{HINWEIS}\nStarten mit: netzinfo last --start");
        if let Some((p, k)) = ziel() {
            for l in netze::load_json::<Store>(&p)?
                .remove(&k)
                .unwrap_or_default()
                .iter()
                .rev()
                .take(5)
            {
                println!("{}: {}", netze::format_utc(l.zeit), zeile(&l.ergebnis));
            }
        }
        return Ok(());
    };
    if hotspot() && !trotzdem {
        return Err(format!(
            "{HOTSPOT_FRAGE} Wenn das gewollt ist: netzinfo last --start --trotzdem"
        ));
    }
    println!("{HINWEIS}\nMesse, das dauert etwa 30 Sekunden …");
    let e = messen(&ok)?;
    ausgeben(&e, explain_on);
    if let Some((pfad, key)) = ziel() {
        let mut store: Store = netze::load_json(&pfad)?;
        ablegen(&mut store, &key, e, netze::now());
        netze::save(&pfad, &store)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"dl_throughput": 94000000, "ul_throughput": 21000000, "base_rtt": 18.5,
        "responsiveness": 250, "dl_responsiveness": 250, "ul_responsiveness": 600, "neues_feld": [1]}"#;

    #[test]
    fn parser_liest_felder_und_rechnet_rpm_um() {
        let e = parse(JSON, 31.0).unwrap();
        assert_eq!(
            (e.down_bps, e.up_bps, e.ruhe_ms),
            (Some(94e6), Some(21e6), Some(18.5))
        );
        assert_eq!(
            (e.last_down_ms, e.last_up_ms, e.rpm),
            (Some(240.0), Some(100.0), Some(250.0))
        );
        assert_eq!(aufblaehung_ms(&e), Some(221.5));
    }

    #[test]
    fn parser_mit_fehlenden_feldern() {
        let e = parse(r#"{"dl_throughput": 5e6}"#, 1.0).unwrap();
        assert_eq!((e.down_bps, e.rpm, e.last_down_ms), (Some(5e6), None, None));
        assert_eq!(aufblaehung_ms(&e), None);
        assert!(zeile(&e).contains("keine RPM"));
        // RPM 0 darf nicht durch null teilen
        let null = parse(r#"{"responsiveness":0,"dl_responsiveness":0}"#, 1.0).unwrap();
        assert_eq!(null.last_down_ms, None);
        assert!(parse("{}", 1.0).is_err());
    }

    #[test]
    fn parser_mit_fehlerausgabe() {
        let f = parse("Error: Network is unreachable", 1.0).unwrap_err();
        assert!(f.contains("Network is unreachable"), "{f}");
    }

    #[test]
    fn einstufung_an_den_grenzen() {
        for (r, s) in [
            (800.0, "sehr reaktionsfähig"),
            (799.0, "gut"),
            (200.0, "gut"),
            (199.0, "mäßig"),
            (100.0, "mäßig"),
            (99.0, "schlecht"),
            (0.0, "schlecht"),
        ] {
            assert_eq!(einstufung(r), s, "{r}");
        }
    }

    #[test]
    fn ohne_bestaetigung_kein_prozess() {
        assert!(Bestaetigung::von(false).is_none());
        // ohne --start läuft `run` durch, ohne messen() aufzurufen (sonst Fehler auf Linux)
        assert!(run(&[], false).is_ok());
        assert!(run(&["--quatsch".into()], false).is_err());
    }

    #[test]
    fn fake_befehl_liefert_ausgabe() {
        let skript = format!("echo '{JSON}'");
        let out = starten("/bin/sh", &["-c", &skript], Duration::from_secs(5)).unwrap();
        assert!(parse(&out, 1.0).is_ok());
    }

    #[test]
    fn fehlerhafter_befehl_meldet_grund() {
        let skript = "echo kaputt >&2; exit 2";
        let f = starten("/bin/sh", &["-c", skript], Duration::from_secs(5)).unwrap_err();
        assert!(f.contains("kaputt"), "{f}");
    }

    #[test]
    fn zeitlimit_beendet_haengenden_prozess() {
        let t = Instant::now();
        let f = starten("/bin/sh", &["-c", "sleep 30"], Duration::from_millis(300)).unwrap_err();
        assert!(f.contains("nicht geantwortet"), "{f}");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn verlauf_haelt_zwanzig() {
        let mut st = Store::new();
        for t in 0..25 {
            ablegen(&mut st, "k", LastErgebnis::default(), t);
        }
        let v = &st["k"];
        assert_eq!((v.len(), v[0].zeit, v[19].zeit), (20, 5, 24));
    }
}
