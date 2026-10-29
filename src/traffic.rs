//! Datenmenge je Programm ohne Root. Issue #58.
//!
//! `nettop` zählt Bytes je Prozess im Kernel. Wir rufen es einmal mit zwei Messungen auf und
//! nehmen die Differenz der beiden Zählerstände.

use serde::Serialize;
use std::collections::HashMap;

pub const MIN_SEKUNDEN: u64 = 5;
pub const MAX_SEKUNDEN: u64 = 300;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verbrauch {
    pub programm: String,
    /// Bytes empfangen / gesendet im Messfenster.
    pub ein: u64,
    pub aus: u64,
    /// Bytes pro Sekunde.
    pub rate_ein: f64,
    pub rate_aus: f64,
}

/// Splits `Name mit Leerzeichen.123` at the last dot. Without a numeric PID the whole key is the name.
fn programmname(schluessel: &str) -> &str {
    match schluessel.rsplit_once('.') {
        Some((name, pid)) if !name.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) => name,
        _ => schluessel,
    }
}

type Tabelle = HashMap<String, (u64, u64)>;

/// One counter table per measurement: `Name.PID` -> (bytes_in, bytes_out). A header line
/// (`,bytes_in,bytes_out,`) starts a new table; unparsable rows are skipped.
fn parse(ausgabe: &str) -> Vec<Tabelle> {
    let mut tabellen: Vec<Tabelle> = Vec::new();
    for zeile in ausgabe.lines() {
        let mut f = zeile.split(',');
        let (Some(name), Some(ein), Some(aus)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if name.is_empty() {
            tabellen.push(Tabelle::new());
        } else if let (Some(t), Ok(ein), Ok(aus)) = (
            tabellen.last_mut(),
            ein.trim().parse::<u64>(),
            aus.trim().parse::<u64>(),
        ) {
            t.insert(name.to_string(), (ein, aus));
        }
    }
    tabellen
}

/// Difference of the last two tables, summed per program name, busiest first. A counter that
/// went back (process restarted under the same PID) counts from zero; a process that only
/// exists in the second table counts in full.
fn differenz(ausgabe: &str, sekunden: u64) -> Result<Vec<Verbrauch>, String> {
    let tabellen = parse(ausgabe);
    let [.., vorher, nachher] = tabellen.as_slice() else {
        return Err("nettop lieferte keine zwei Messungen. Starte `netzinfo traffic` noch einmal; wenn es bleibt, hat macOS das Ausgabeformat geändert.".into());
    };
    let mut je_name: HashMap<&str, (u64, u64)> = HashMap::new();
    for (schluessel, &(ein, aus)) in nachher {
        let (v_ein, v_aus) = vorher.get(schluessel).copied().unwrap_or((0, 0));
        let sum = je_name.entry(programmname(schluessel)).or_default();
        sum.0 += ein.checked_sub(v_ein).unwrap_or(ein);
        sum.1 += aus.checked_sub(v_aus).unwrap_or(aus);
    }
    let s = sekunden.max(1) as f64;
    let mut liste: Vec<Verbrauch> = je_name
        .into_iter()
        .map(|(name, (ein, aus))| Verbrauch {
            programm: name.to_string(),
            ein,
            aus,
            rate_ein: ein as f64 / s,
            rate_aus: aus as f64 / s,
        })
        .collect();
    liste.sort_by(|a, b| {
        (b.rate_ein + b.rate_aus)
            .total_cmp(&(a.rate_ein + a.rate_aus))
            .then_with(|| a.programm.cmp(&b.programm))
    });
    Ok(liste)
}

#[cfg(target_os = "macos")]
fn nettop(sekunden: u64) -> Result<String, String> {
    let out = std::process::Command::new("/usr/bin/nettop")
        .args([
            "-P",
            "-L",
            "2",
            "-x",
            "-s",
            &sekunden.to_string(),
            "-J",
            "bytes_in,bytes_out",
        ])
        .output()
        .map_err(|e| format!("nettop ließ sich nicht starten ({}). Es gehört zu macOS, prüfe, ob /usr/bin/nettop existiert.", crate::io_satz(&e)))?;
    if !out.status.success() {
        return Err("nettop meldete einen Fehler. Versuche es noch einmal.".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn nettop(_sekunden: u64) -> Result<String, String> {
    Err("Die Datenmenge je Programm ist ohne Root nur auf dem Mac möglich (nettop). Unter Linux brauchen die Zähler je Prozess Root oder einen Mitschnitt.".into())
}

/// Measures for `sekunden` (blocks that long) and returns the programs, busiest first.
pub fn messen(sekunden: u64) -> Result<Vec<Verbrauch>, String> {
    if !(MIN_SEKUNDEN..=MAX_SEKUNDEN).contains(&sekunden) {
        return Err(format!(
            "Die Messdauer muss zwischen {MIN_SEKUNDEN} und {MAX_SEKUNDEN} Sekunden liegen. Beispiel: `netzinfo traffic --sekunden 10`."
        ));
    }
    differenz(&nettop(sekunden)?, sekunden)
}

/// Menge mit Einheit und deutschem Komma (Zehnerpotenzen wie im Finder).
fn menge(b: f64) -> String {
    match b {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b if b >= 1e3 => format!("{:.1} kB", b / 1e3),
        b => format!("{b:.0} B"),
    }
    .replace('.', ",")
}

pub const OPTIONEN: &[&str] = &["--json", "--sekunden"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "traffic")?;
    let (mut sekunden, mut json) = (10, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--sekunden" => {
                sekunden = it.next().and_then(|s| s.parse().ok()).ok_or(format!(
                    "Nach `--sekunden` gehört eine ganze Zahl von {MIN_SEKUNDEN} bis {MAX_SEKUNDEN}. Beispiel: `netzinfo traffic --sekunden 10`."
                ))?;
            }
            _ => {
                return Err(format!(
                    "Unbekannte Option „{a}“. Erlaubt: `--sekunden N`, `--json`."
                ));
            }
        }
    }
    let e = |t: &str| crate::explain(explain_on, t);
    e(
        "Ich frage nettop, den Zähler des Systems für Bytes je Programm. Das geht ohne Root und ohne Paketmitschnitt.",
    );
    e(&format!(
        "Ich lese die Zähler zweimal im Abstand von {sekunden} Sekunden. Die Differenz ist, was in dieser Zeit floss."
    ));
    if !json && (MIN_SEKUNDEN..=MAX_SEKUNDEN).contains(&sekunden) {
        println!("Messe {sekunden} Sekunden lang, bitte warten …");
    }
    let liste = messen(sekunden)?;
    e(
        "Die Zahlen zählen alle Wege, auch Verbindungen zum eigenen Router. Ein Programm, das viel hochlädt, obwohl du nichts hochlädst, ist einen Blick wert.",
    );
    if json {
        return serde_json::to_string_pretty(&liste)
            .map(|s| println!("{s}"))
            .map_err(|e| format!("Ausgabe als JSON nicht möglich ({e})."));
    }
    let aktiv: Vec<_> = liste.iter().filter(|v| v.ein + v.aus > 0).collect();
    if aktiv.is_empty() {
        println!("In {sekunden} Sekunden hat kein Programm Daten übertragen.");
        return Ok(());
    }
    println!("Letzte {sekunden} Sekunden, je Programm (ein = empfangen, aus = gesendet):");
    for v in aktiv.iter().take(15) {
        println!(
            "  {:<28} ein {:>9}  aus {:>9}   ({}/s ein, {}/s aus)",
            v.programm,
            menge(v.ein as f64),
            menge(v.aus as f64),
            menge(v.rate_ein),
            menge(v.rate_aus)
        );
    }
    if aktiv.len() > 15 {
        println!("… und {} weitere mit weniger Daten.", aktiv.len() - 15);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded nettop output shape, program names invented.
    const AUSGABE: &str = "\
,bytes_in,bytes_out,
kernel_task.0,1000,2000,
Beispiel Browser.101,5000000,100000,
Beispiel Browser.102,1000,1000,
Backup Dienst.v2.200,300,9000000,
Altlast.300,10,10,
,bytes_in,bytes_out,
kernel_task.0,1000,2000,
Beispiel Browser.101,5600000,100500,
Beispiel Browser.102,3000,1500,
Backup Dienst.v2.200,300,13000000,
Neustart.400,70,80,
Altlast.300,4,50,
kaputte zeile
";

    #[test]
    fn name_wird_am_letzten_punkt_getrennt() {
        assert_eq!(programmname("Backup Dienst.v2.200"), "Backup Dienst.v2");
        assert_eq!(programmname("Beispiel Browser.101"), "Beispiel Browser");
        assert_eq!(programmname("ohnepid"), "ohnepid");
    }

    #[test]
    fn differenz_fasst_pids_zusammen_und_sortiert() {
        let l = differenz(AUSGABE, 5).unwrap();
        assert_eq!(l[0].programm, "Backup Dienst.v2");
        let b = l.iter().find(|v| v.programm == "Beispiel Browser").unwrap();
        assert_eq!((b.ein, b.aus), (602_000, 1000));
        assert_eq!(b.rate_ein, 120_400.0);
        // Eingangszähler gesprungen (Prozess neu gestartet): zählt ab Null, Ausgang normal.
        let a = l.iter().find(|v| v.programm == "Altlast").unwrap();
        assert_eq!((a.ein, a.aus), (4, 40));
        // Neuer Prozess zählt voll, unveränderter hat Differenz 0.
        assert_eq!(l.iter().find(|v| v.programm == "Neustart").unwrap().ein, 70);
        assert_eq!(
            l.iter().find(|v| v.programm == "kernel_task").unwrap().aus,
            0
        );
    }

    #[test]
    fn eine_messung_ist_ein_fehler() {
        assert!(differenz(",bytes_in,bytes_out,\nA.1,1,1,\n", 5).is_err());
        assert!(differenz("", 5).is_err());
    }

    #[test]
    fn menge_mit_komma() {
        assert_eq!(menge(1_500.0), "1,5 kB");
        assert_eq!(menge(999.0), "999 B");
        assert_eq!(menge(2.5e9), "2,5 GB");
    }

    #[test]
    fn dauer_wird_geprueft() {
        assert!(messen(1).is_err());
        assert!(messen(301).is_err());
    }
}
