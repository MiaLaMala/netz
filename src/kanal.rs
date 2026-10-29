//! WLAN-Kanalumfeld: Auslastung und Bandempfehlung. Issue #55.
//!
//! `netzinfo wlan --umfeld` scannt auf Befehl (nie im Hintergrund) und zählt je Kanal, wie viele Netze senden.
//! Von den Nachbarn bleibt nur Kanal, Band, Breite und Signal im Speicher: Name und Kennung werden nie gelesen,
//! nichts davon wird gespeichert.

use crate::explain;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Band {
    #[serde(rename = "2,4 GHz")]
    B24,
    #[serde(rename = "5 GHz")]
    B5,
    #[serde(rename = "6 GHz")]
    B6,
}

impl Band {
    pub fn name(self) -> &'static str {
        match self {
            Self::B24 => "2,4 GHz",
            Self::B5 => "5 GHz",
            Self::B6 => "6 GHz",
        }
    }

    pub fn satz(self) -> &'static str {
        match self {
            Self::B24 => {
                "Du bist im 2,4 GHz Band. Das reicht weiter, ist aber voller. Wenn du nah am Router bist, ist 5 GHz schneller."
            }
            Self::B5 | Self::B6 => {
                "Du bist in einem 5 oder 6 GHz Band. Es ist meist leerer und schneller, kommt aber durch Wände schlechter."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Kanal {
    pub band: Band,
    pub kanal: u32,
}

/// Ein gehörtes Netz. Absichtlich ohne Name und Kennung.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Beobachtung {
    pub kanal: u32,
    pub band: Band,
    pub breite_mhz: Option<u32>,
    pub rssi: i32,
}

/// Auslastung eines Kanals: Netze, die auf ihm oder überlappend senden, und ihre Leistung zusammen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Last {
    pub band: Band,
    pub kanal: u32,
    pub netze: usize,
    pub dbm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Empfehlung {
    pub text: String,
    pub ziel: Option<Kanal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ansicht {
    pub beobachtungen: Vec<Beobachtung>,
    pub aktuell: Option<Kanal>,
    pub land: Option<String>,
    pub belegung: Vec<Last>,
    pub empfehlung: Empfehlung,
}

/// Rohergebnis des Scans.
pub struct Scan {
    pub beobachtungen: Vec<Beobachtung>,
    pub aktuell: Option<Kanal>,
    pub unterstuetzt: Vec<Kanal>,
    pub land: Option<String>,
}

/// 2,4 GHz: Kanäle liegen 5 MHz auseinander und sind 20 MHz breit, ab 4 Kanälen Abstand stören sie sich nicht mehr.
/// 5 und 6 GHz: nur derselbe Kanal.
// ponytail: breite Kanäle (40 bis 160 MHz) zählen nur auf ihrem Hauptkanal; Kanal 14 (Japan) zählt wie ein Nachbar.
fn ueberlappt(band: Band, a: u32, b: u32) -> bool {
    a == b || (band == Band::B24 && a.abs_diff(b) < 4)
}

/// Leistungssumme in dBm (Summe von 10^(rssi/10), zurück in dBm); `None` ohne Werte.
pub fn summe_dbm(rssi: impl IntoIterator<Item = i32>) -> Option<f64> {
    let mw: Vec<f64> = rssi
        .into_iter()
        .map(|r| 10f64.powf(f64::from(r) / 10.0))
        .collect();
    (!mw.is_empty()).then(|| 10.0 * mw.iter().sum::<f64>().log10())
}

pub fn last(beob: &[Beobachtung], band: Band, kanal: u32) -> Last {
    let treffer: Vec<i32> = beob
        .iter()
        .filter(|b| b.band == band && ueberlappt(band, b.kanal, kanal))
        .map(|b| b.rssi)
        .collect();
    Last {
        band,
        kanal,
        netze: treffer.len(),
        dbm: summe_dbm(treffer),
    }
}

/// Auslastung jedes Kanals, auf dem etwas gehört wurde, nach Band und Kanal.
pub fn belegung(beob: &[Beobachtung]) -> Vec<Last> {
    let mut k: Vec<(Band, u32)> = beob.iter().map(|b| (b.band, b.kanal)).collect();
    k.sort();
    k.dedup();
    k.into_iter().map(|(b, c)| last(beob, b, c)).collect()
}

fn netze_wort(n: usize) -> String {
    if n == 1 {
        "1 Netz".into()
    } else {
        format!("{n} Netze")
    }
}

/// Ein Kanal ist deutlich leerer, wenn er 6 dB weniger Leistung oder 3 Netze weniger hat.
fn leerer(jetzt: &Last, alt: &Last) -> bool {
    let (j, a) = (jetzt.dbm.unwrap_or(-200.0), alt.dbm.unwrap_or(-200.0));
    j - a >= 6.0 || jetzt.netze >= alt.netze + 3
}

/// Rat aus Scan, aktuellem Kanal und den Kanälen, die der Chip kann (das Land steckt schon in dieser Liste).
pub fn empfehlung(
    beob: &[Beobachtung],
    aktuell: Option<Kanal>,
    unterstuetzt: &[Kanal],
) -> Empfehlung {
    let kein = |text: &str| Empfehlung {
        text: text.into(),
        ziel: None,
    };
    let Some(a) = aktuell else {
        return kein(
            "Der Mac hängt nicht per WLAN im Netz, deshalb gibt es keinen eigenen Kanal zu vergleichen.",
        );
    };
    // 2,4 GHz: nur 1, 6 und 11, die sich nicht überlappen.
    let kandidaten: Vec<Last> = unterstuetzt
        .iter()
        .filter(|k| k.band != Band::B24 || [1, 6, 11].contains(&k.kanal))
        .map(|k| last(beob, k.band, k.kanal))
        .collect();
    if kandidaten.is_empty() {
        return kein(
            "Die Kanalliste der WLAN-Karte fehlt, deshalb gebe ich keinen Rat. Das Land der Karte wird oft erst nach der ersten Verbindung gesetzt.",
        );
    }
    let jetzt = last(beob, a.band, a.kanal);
    let beste = |gleiches_band: bool| {
        kandidaten
            .iter()
            .filter(|c| (c.band == a.band) == gleiches_band)
            .min_by(|x, y| {
                x.dbm
                    .unwrap_or(-200.0)
                    .total_cmp(&y.dbm.unwrap_or(-200.0))
                    .then(x.netze.cmp(&y.netze))
            })
    };
    let ort = "Den Kanal stellst du in den WLAN-Einstellungen deines Routers ein.";
    let jetzt_satz = format!(
        "Auf Kanal {} ({}) senden {}.",
        a.kanal,
        a.band.name(),
        netze_wort(jetzt.netze)
    );
    for gleiches in [true, false] {
        if let Some(b) = beste(gleiches).filter(|b| leerer(&jetzt, b)) {
            let wo = if gleiches {
                format!("Kanal {} ist leerer", b.kanal)
            } else {
                format!("Das {} Band ist leerer: Kanal {}", b.band.name(), b.kanal)
            };
            return Empfehlung {
                text: format!("{jetzt_satz} {wo} ({} dort). {ort}", netze_wort(b.netze)),
                ziel: Some(Kanal {
                    band: b.band,
                    kanal: b.kanal,
                }),
            };
        }
    }
    kein(&format!(
        "{jetzt_satz} Die anderen Kanäle sind nicht deutlich leerer, ein Wechsel bringt wenig."
    ))
}

/// Scan und Auswertung in einem Schritt, für Kommandozeile und App. Speichert nichts.
pub fn umfeld() -> Result<Ansicht, String> {
    let s = scan()?;
    Ok(Ansicht {
        belegung: belegung(&s.beobachtungen),
        empfehlung: empfehlung(&s.beobachtungen, s.aktuell, &s.unterstuetzt),
        beobachtungen: s.beobachtungen,
        aktuell: s.aktuell,
        land: s.land,
    })
}

/// Blockiert 2 bis 5 Sekunden. Liest je Netz nur Kanal, Breite und Signal.
#[cfg(target_os = "macos")]
pub fn scan() -> Result<Scan, String> {
    use objc2_core_wlan::{CWChannel, CWWiFiClient};
    let kanal = |c: &CWChannel| -> Option<Kanal> {
        // SAFETY: plain getters on a CoreWLAN object.
        let (b, n) = unsafe { (c.channelBand().0, c.channelNumber()) };
        let band = match b {
            1 => Band::B24,
            2 => Band::B5,
            3 => Band::B6,
            _ => return None,
        };
        Some(Kanal {
            band,
            kanal: u32::try_from(n).ok().filter(|k| *k > 0)?,
        })
    };
    // SAFETY: plain getters and one blocking scan, no pointers or callbacks are passed.
    unsafe {
        let Some(i) = CWWiFiClient::sharedWiFiClient().interface() else {
            return Err("Der Mac hat keine WLAN-Karte, die Netzinfo ansprechen kann. Schalte das WLAN ein und versuche es noch mal.".into());
        };
        let netze = i.scanForNetworksWithName_error(None).map_err(|e| {
            format!("Der WLAN-Scan ging schief ({e}). Prüfe, ob das WLAN eingeschaltet ist, und versuche es noch mal.")
        })?;
        let beobachtungen = netze
            .to_vec()
            .iter()
            .filter_map(|n| {
                let c = n.wlanChannel()?;
                let k = kanal(&c)?;
                Some(Beobachtung {
                    kanal: k.kanal,
                    band: k.band,
                    breite_mhz: match c.channelWidth().0 {
                        1 => Some(20),
                        2 => Some(40),
                        3 => Some(80),
                        4 => Some(160),
                        _ => None,
                    },
                    rssi: n.rssiValue() as i32,
                })
            })
            .collect();
        Ok(Scan {
            beobachtungen,
            aktuell: i.wlanChannel().and_then(|c| kanal(&c)),
            unterstuetzt: i
                .supportedWLANChannels()
                .map(|s| s.to_vec().iter().filter_map(|c| kanal(c)).collect())
                .unwrap_or_default(),
            land: i
                .countryCode()
                .map(|c| c.to_string())
                .filter(|c| !c.is_empty()),
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn scan() -> Result<Scan, String> {
    Err("`netzinfo wlan --umfeld` scannt bisher nur auf dem Mac (CoreWLAN). Unter Linux und Windows gibt es das noch nicht.".into())
}

pub const OPTIONEN: &[&str] = &["--json"];

/// `netzinfo wlan --umfeld [--json]`, aufgerufen aus `wlan::run`.
pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "wlan --umfeld")?;
    let json = args == ["--json"];
    if !json && let Some(a) = args.first() {
        return Err(format!(
            "`netzinfo wlan --umfeld` kennt „{a}“ nicht. Erlaubt ist nur `--json`, `--erklaer` erklärt die Werte."
        ));
    }
    explain(
        explain_on,
        "Ich starte einen WLAN-Scan: Der Mac hört 2 bis 5 Sekunden auf allen Kanälen, welche Netze in der Nähe senden. Ich lese nur Kanal und Signal, keine Namen, und speichere nichts von den Nachbarn. Ein Scan kann die Verbindung kurz stören, deshalb läuft er nur auf Befehl.",
    );
    let a = umfeld()?;
    if json {
        let t = serde_json::to_string_pretty(&a)
            .map_err(|e| format!("Die Werte ließen sich nicht umwandeln ({e})."))?;
        println!("{t}");
        return Ok(());
    }
    if a.beobachtungen.is_empty() {
        println!(
            "Der Scan hat kein Netz gehört. Ohne Standortfreigabe liefert macOS manchmal nichts; in der App kannst du sie unter „Dieses Netz“ erteilen."
        );
        return Ok(());
    }
    println!(
        "{} in Reichweite{}.",
        netze_wort(a.beobachtungen.len()),
        a.land
            .as_deref()
            .map(|l| format!(", Regelgebiet {l}"))
            .unwrap_or_default()
    );
    for b in [Band::B24, Band::B5, Band::B6] {
        let zeilen: Vec<&Last> = a.belegung.iter().filter(|l| l.band == b).collect();
        if zeilen.is_empty() {
            continue;
        }
        println!("\n{}", b.name());
        for l in zeilen {
            let du = if a.aktuell
                == Some(Kanal {
                    band: b,
                    kanal: l.kanal,
                }) {
                "  <- dein Kanal"
            } else {
                ""
            };
            println!(
                "  Kanal {:>3}: {:>8}, zusammen {:.0} dBm{du}",
                l.kanal,
                netze_wort(l.netze),
                l.dbm.unwrap_or(f64::NAN)
            );
        }
    }
    println!("\n{}", a.empfehlung.text);
    if explain_on {
        explain(
            true,
            "Die Stärke je Kanal ist die Summe der Sendeleistungen aller Netze, die ihn stören (im 2,4 GHz Band auch Nachbarkanäle bis 3 Kanäle daneben), in dBm: je näher an 0, desto lauter. Ich rate zum Wechsel, wenn ein anderer Kanal 6 dB leiser ist oder 3 Netze weniger hat.",
        );
        if let Some(k) = a.aktuell {
            explain(true, k.band.satz());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(band: Band, kanal: u32, rssi: i32) -> Beobachtung {
        Beobachtung {
            kanal,
            band,
            breite_mhz: Some(20),
            rssi,
        }
    }
    fn k(band: Band, kanal: u32) -> Kanal {
        Kanal { band, kanal }
    }

    #[test]
    fn leistungssumme() {
        assert!(summe_dbm([]).is_none());
        assert!((summe_dbm([-60]).unwrap() + 60.0).abs() < 1e-9);
        // zwei gleich starke Netze: +3 dB
        assert!((summe_dbm([-60, -60]).unwrap() + 56.99).abs() < 0.01);
    }

    #[test]
    fn ueberlappung_24ghz() {
        let o = [
            b(Band::B24, 1, -60),
            b(Band::B24, 3, -60),
            b(Band::B24, 6, -60),
            b(Band::B5, 1, -60),
        ];
        assert_eq!(last(&o, Band::B24, 1).netze, 2); // 1 und 3 stören sich, 6 nicht
        assert_eq!(last(&o, Band::B24, 8).netze, 1); // nur 6
        assert_eq!(last(&o, Band::B24, 4).netze, 3); // 1, 3 und 6 liegen höchstens 3 Kanäle entfernt
        assert_eq!(last(&o, Band::B24, 11).netze, 0);
        assert_eq!(last(&o, Band::B5, 1).netze, 1); // 5 GHz nur derselbe Kanal, anderes Band zählt nicht
        assert_eq!(last(&o, Band::B5, 3).netze, 0);
    }

    #[test]
    fn rat_wechselt_auf_leereren_kanal() {
        let mut o: Vec<_> = (0..9).map(|i| b(Band::B24, 6, -55 - i)).collect();
        o.push(b(Band::B24, 1, -80));
        let kanaele = [
            k(Band::B24, 1),
            k(Band::B24, 6),
            k(Band::B24, 11),
            k(Band::B24, 13),
        ];
        let e = empfehlung(&o, Some(k(Band::B24, 6)), &kanaele);
        assert_eq!(e.ziel, Some(k(Band::B24, 11)));
        assert!(e.text.contains("9 Netze"), "{}", e.text);
    }

    #[test]
    fn rat_anderes_band_und_kein_rat() {
        let o = [
            b(Band::B24, 1, -50),
            b(Band::B24, 6, -50),
            b(Band::B24, 11, -50),
            b(Band::B5, 36, -75),
        ];
        let alle = [
            k(Band::B24, 1),
            k(Band::B24, 6),
            k(Band::B24, 11),
            k(Band::B5, 36),
            k(Band::B5, 40),
        ];
        let e = empfehlung(&o, Some(k(Band::B24, 6)), &alle);
        assert_eq!(e.ziel, Some(k(Band::B5, 40)));
        assert!(e.text.contains("5 GHz Band"), "{}", e.text);
        // 2 dB Unterschied und gleich viele Netze: kein Wechsel
        let still = [b(Band::B24, 6, -50), b(Band::B24, 1, -52)];
        let kan = [k(Band::B24, 1), k(Band::B24, 6)];
        let e = empfehlung(&still, Some(k(Band::B24, 6)), &kan);
        assert_eq!(e.ziel, None);
        assert!(e.text.contains("bringt wenig"), "{}", e.text);
    }

    #[test]
    fn rat_ohne_kanalliste_oder_verbindung() {
        // Ländercode leer: die Karte nennt keine Kanäle, also kein Rat statt eines geratenen.
        let e = empfehlung(&[b(Band::B24, 6, -50)], Some(k(Band::B24, 6)), &[]);
        assert_eq!(e.ziel, None);
        assert!(e.text.contains("Kanalliste"));
        assert_eq!(empfehlung(&[], None, &[k(Band::B24, 1)]).ziel, None);
    }

    #[test]
    fn keine_namen_in_den_daten() {
        let o = [b(Band::B5, 36, -60)];
        let a = Ansicht {
            beobachtungen: o.to_vec(),
            aktuell: Some(k(Band::B5, 36)),
            land: Some("DE".into()),
            belegung: belegung(&o),
            empfehlung: empfehlung(&[], None, &[]),
        };
        let v = serde_json::to_value(&a).unwrap();
        let mut schluessel: Vec<_> = v["beobachtungen"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        schluessel.sort();
        assert_eq!(schluessel, ["band", "breite_mhz", "kanal", "rssi"]);
        let text = v.to_string().to_lowercase();
        assert!(!text.contains("ssid") && !text.contains("name"));
    }

    #[test]
    fn falsche_option() {
        assert!(
            run(&["--x".to_string()], false)
                .unwrap_err()
                .contains("--x")
        );
    }
}
