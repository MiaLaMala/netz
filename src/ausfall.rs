//! Ausfallprotokoll (issue #51): erkennt aus dem Stichproben-Strom von `messen` Ausfälle, ordnet
//! sie einer Schicht zu und führt ein Protokoll je Netz (`ausfall.json` neben `qualitaet.json`,
//! gleicher Schlüssel). Daraus entsteht der Abschnitt „Ausfälle“ und der Text für den Anbieter.
//! Im Protokoll und im Text stehen keine Adressen und keine Namen.

use crate::messen::{LOSS, SKIP, Sample, dur};
use crate::netze;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Verluste in Folge bis „Ausfall beginnt“, Antworten in Folge bis „Ausfall endet“.
/// ponytail: Der Takt von `messen` ist 5 s, also 15 s (das Issue rechnete mit 1 s Takt und 3 s).
/// Der Messabstand bleibt, er kostet Akku und Datenvolumen. Kürzer geht nur mit engerem Takt.
const FOLGE: u32 = 3;
/// Mehr Pause als das zwischen zwei Stichproben heißt: der Rechner schlief oder die Messung
/// stand. Takt 5 s plus bis zu 6 s Wartezeit bei Totalverlust bleiben darunter.
/// ponytail: gilt für den Takt 5 s; ab `--takt 30` erkennt das Protokoll nichts mehr.
const LUECKE_S: u64 = 30;
const MAX_EINTRAEGE: usize = 200;
/// Zeitleiste und Aussetzer-Liste zeigen und halten diese Spanne.
const TAGE: u64 = 7;

/// Wo es hängt. Die Worte folgen `diagnose::Schicht` (WlanOderRouter, Anbieter); im Strom gibt es
/// keinen Anbieter-Hop und nur ein Internet-Ziel (1.1.1.1), also bleibt „Anbieter oder weiter weg“.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ort {
    RouterOderWlan,
    AnbieterOderWeiter,
    NurDns,
    /// Der Router wird in diesem Netz nicht gemessen: nicht zuzuordnen.
    Unklar,
}

impl Ort {
    fn wort(self) -> &'static str {
        match self {
            Ort::RouterOderWlan => "Router oder WLAN",
            Ort::AnbieterOderWeiter => "Anbieter oder weiter weg",
            Ort::NurDns => "nur DNS",
            Ort::Unklar => "nicht zuzuordnen",
        }
    }

    fn satz(self) -> &'static str {
        match self {
            Ort::RouterOderWlan => {
                "Der Router war nicht erreichbar. Das spricht für ein Problem im WLAN oder am Router."
            }
            Ort::AnbieterOderWeiter => {
                "Der Router war erreichbar, das Internet nicht. Das spricht für eine Störung beim Anbieter oder weiter weg."
            }
            Ort::NurDns => {
                "Nur die Namensauflösung fiel aus, eine direkte Verbindung ging. Das liegt am DNS-Server, nicht an der Leitung."
            }
            Ort::Unklar => {
                "Der Router lässt sich in diesem Netz nicht messen, deshalb ist die Ursache nicht zuzuordnen."
            }
        }
    }
}

/// Ein Eintrag im Protokoll. Keine Adressen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Eintrag {
    /// Unix-Sekunden: erste verlorene Stichprobe.
    pub beginn: u64,
    /// Unix-Sekunden: erste Stichprobe mit Antwort am Ende.
    pub ende: u64,
    pub dauer_s: u64,
    pub schicht: Ort,
    /// Kurze Antworten mitten im Ausfall (weniger als 3 in Folge).
    #[serde(default)]
    pub aussetzer: u32,
}

/// Zustand zwischen zwei Stichproben, auch zwischen zwei Aufrufen der App.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Lauf {
    zuletzt: u64,
    /// Verluste in Folge vor einem Ausfall: Zeit der ersten und Zahl.
    schlecht: Option<(u64, Ort, u32)>,
    /// Laufender Ausfall: Beginn, Ort, Antworten in Folge, Aussetzer, Zeit des letzten Verlusts.
    aus: Option<Aus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Aus {
    beginn: u64,
    ort: Ort,
    gut: u32,
    aussetzer: u32,
    letzter_verlust: u64,
    /// Zeit der ersten Antwort der laufenden Gut-Folge.
    gut_ab: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)] // Dateien älterer Versionen haben nicht alle Felder.
pub struct Daten {
    pub liste: Vec<Eintrag>,
    /// Zeiten kurzer Verlust-Folgen (1 oder 2), die kein Ausfall wurden. Nur Zählung für die Leiste.
    pub aussetzer: Vec<u64>,
    lauf: Lauf,
}

type Store = BTreeMap<String, Daten>;

/// Schlecht ist eine Stichprobe, wenn das Internet fehlt (1.1.1.1 per DNS und TCP) oder nur DNS.
fn klasse(v: &[u32; 3]) -> Option<Ort> {
    let (dns, tcp) = (v[1] == LOSS, v[2] == LOSS);
    match (dns, tcp) {
        (true, true) => Some(match v[0] {
            LOSS => Ort::RouterOderWlan,
            SKIP => Ort::Unklar,
            _ => Ort::AnbieterOderWeiter,
        }),
        (true, false) => Some(Ort::NurDns),
        _ => None,
    }
}

impl Daten {
    /// Nimmt eine Stichprobe auf. `gegenprobe` ist die TCP-Verbindung zu 1.1.1.1:443; sie läuft nur
    /// im Moment, in dem ein Ausfall beginnen würde, und `true` heißt: das Internet geht doch.
    fn aufnehmen(&mut self, s: &Sample, gegenprobe: &dyn Fn() -> bool) {
        let l = &mut self.lauf;
        if l.zuletzt != 0 && s.t.saturating_sub(l.zuletzt) > LUECKE_S {
            // Ruhezustand oder Messung stand: nie als Ausfall zählen, ein offener Ausfall endet beim letzten Verlust.
            if let Some(a) = l.aus.take() {
                let ende = a.letzter_verlust;
                self.liste.push(Eintrag {
                    beginn: a.beginn,
                    ende,
                    dauer_s: ende.saturating_sub(a.beginn),
                    schicht: a.ort,
                    aussetzer: a.aussetzer,
                });
            }
            l.schlecht = None;
        }
        l.zuletzt = s.t;
        match (klasse(&s.v), l.aus.as_mut()) {
            (Some(_), Some(a)) => {
                if a.gut > 0 {
                    a.aussetzer += 1;
                }
                a.gut = 0;
                a.letzter_verlust = s.t;
            }
            (None, Some(a)) => {
                if a.gut == 0 {
                    a.gut_ab = s.t;
                }
                a.gut += 1;
                if a.gut >= FOLGE {
                    let (beginn, ende) = (a.beginn, a.gut_ab);
                    self.liste.push(Eintrag {
                        beginn,
                        ende,
                        dauer_s: ende.saturating_sub(beginn),
                        schicht: a.ort,
                        aussetzer: a.aussetzer,
                    });
                    l.aus = None;
                }
            }
            (Some(ort), None) => {
                let (t0, o0, n) = l.schlecht.unwrap_or((s.t, ort, 0));
                if n + 1 < FOLGE {
                    l.schlecht = Some((t0, o0, n + 1));
                } else if gegenprobe() {
                    l.schlecht = None; // ICMP-Sperre oder DNS-Hänger, das Internet geht
                } else {
                    l.schlecht = None;
                    l.aus = Some(Aus {
                        beginn: t0,
                        ort: o0,
                        gut: 0,
                        aussetzer: 0,
                        letzter_verlust: s.t,
                        gut_ab: 0,
                    });
                }
            }
            (None, None) => {
                if l.schlecht.take().is_some() {
                    self.aussetzer.push(s.t);
                }
            }
        }
        let alt = s.t.saturating_sub(TAGE * 86400);
        self.aussetzer.retain(|&t| t >= alt);
        let zuviel = self.liste.len().saturating_sub(MAX_EINTRAEGE);
        self.liste.drain(..zuviel);
    }
}

/// Hängt eine Stichprobe an das Protokoll des Netzes `key`.
pub(crate) fn verarbeiten(
    pfad: &Path,
    key: &str,
    s: &Sample,
    gegenprobe: &dyn Fn() -> bool,
) -> Result<(), String> {
    let _sperre = netze::sperre(pfad);
    let mut store: Store = netze::load_json(pfad)?;
    store
        .entry(key.to_string())
        .or_default()
        .aufnehmen(s, gegenprobe);
    netze::save(pfad, &store)
}

pub(crate) fn laden(pfad: &Path, key: &str) -> Result<Daten, String> {
    let mut store: Store = netze::load_json(pfad)?;
    Ok(store.remove(key).unwrap_or_default())
}

fn woche_ab(t: u64) -> u64 {
    // 1.1.1.1970 war ein Donnerstag, +3 macht den Montag zum Wochenanfang
    ((t / 86400 + 3) / 7 * 7).saturating_sub(3) * 86400
}

fn tag_zeit(t: u64) -> String {
    netze::format_utc(t)
}

/// Abschnitt „Ausfälle“ als Markdown: Tabelle, Summe je Woche. `None`: nichts protokolliert.
pub fn abschnitt(d: &Daten) -> Option<String> {
    if d.liste.is_empty() {
        return None;
    }
    let mut out =
        String::from("| Beginn | Dauer | Schicht | Aussetzer dabei |\n|---|---|---|---|\n");
    for e in &d.liste {
        out += &format!(
            "| {} | {} | {} | {} |\n",
            tag_zeit(e.beginn),
            dur(e.dauer_s),
            e.schicht.wort(),
            e.aussetzer
        );
    }
    let mut wochen: BTreeMap<u64, u64> = BTreeMap::new();
    for e in &d.liste {
        *wochen.entry(woche_ab(e.beginn)).or_default() += e.dauer_s;
    }
    out += "\nSumme je Woche:\n\n";
    for (w, s) in &wochen {
        out += &format!("- Woche ab {}: {}\n", &tag_zeit(*w)[..13], dur(*s));
    }
    Some(out)
}

/// Sachlicher Text für den Anbieter. Zeiten in UTC, ohne Adressen und ohne Namen von Netz oder Anbieter.
pub fn text_fuer_anbieter(d: &Daten, jetzt: u64) -> String {
    let ab = jetzt.saturating_sub(TAGE * 86400);
    let l: Vec<&Eintrag> = d.liste.iter().filter(|e| e.beginn >= ab).collect();
    if l.is_empty() {
        return "In den letzten 7 Tagen wurde kein Ausfall gemessen.".into();
    }
    let summe: u64 = l.iter().map(|e| e.dauer_s).sum();
    let mut out = format!(
        "Störungsmeldung\n\nIch habe an meinem Anschluss in den letzten 7 Tagen {} {} gemessen, zusammen {}. Alle Zeiten in UTC.\n\n",
        l.len(),
        if l.len() == 1 { "Ausfall" } else { "Ausfälle" },
        dur(summe)
    );
    for e in &l {
        out += &format!(
            "- {}, Dauer {}: {}\n",
            tag_zeit(e.beginn),
            dur(e.dauer_s),
            e.schicht.satz()
        );
    }
    out += &format!(
        "\nGemessen wurde mit DNS-Anfragen und TCP-Verbindungen (kein ICMP) alle 5 Sekunden. Ein Ausfall zählt ab {FOLGE} verlorenen Messungen in Folge, kürzere Aussetzer sind nicht mitgezählt ({} in diesem Zeitraum). Die Zuordnung ist eine Vermutung aus der Messung. Bitte prüfen Sie die Leitung.",
        d.aussetzer.iter().filter(|&&t| t >= ab).count()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: [u32; 3] = [1000, 1000, 1000];
    const INTERNET_WEG: [u32; 3] = [1000, LOSS, LOSS];

    /// Reihe im 5-s-Takt ab t=1000.
    fn lauf(reihe: &[[u32; 3]], gegen: bool) -> Daten {
        let mut d = Daten::default();
        for (i, v) in reihe.iter().enumerate() {
            d.aufnehmen(
                &Sample {
                    t: 1000 + 5 * i as u64,
                    v: *v,
                },
                &|| gegen,
            );
        }
        d
    }

    fn mit(n_aus: usize, aus: [u32; 3], n_ok: usize) -> Vec<[u32; 3]> {
        let mut v = vec![OK; 2];
        v.extend(std::iter::repeat_n(aus, n_aus));
        v.extend(std::iter::repeat_n(OK, n_ok));
        v
    }

    #[test]
    fn grenze_zwei_und_drei_verluste() {
        let d = lauf(&mit(2, INTERNET_WEG, 5), false);
        assert!(d.liste.is_empty());
        assert_eq!(d.aussetzer.len(), 1);
        let d = lauf(&mit(3, INTERNET_WEG, 5), false);
        assert_eq!(d.liste.len(), 1);
        assert!(d.aussetzer.is_empty());
        // Verluste bei t=1010,1015,1020; erste Antwort 1025: 15 s
        let e = &d.liste[0];
        assert_eq!(
            (e.beginn, e.ende, e.dauer_s, e.aussetzer),
            (1010, 1025, 15, 0)
        );
    }

    #[test]
    fn ende_erst_nach_drei_antworten() {
        // zwei Antworten reichen nicht: der Ausfall läuft weiter, die Antworten sind ein Aussetzer
        let mut r = mit(3, INTERNET_WEG, 2);
        r.extend([INTERNET_WEG; 2]);
        r.extend([OK; 3]);
        let d = lauf(&r, false);
        assert_eq!(d.liste.len(), 1);
        let e = &d.liste[0];
        assert_eq!((e.beginn, e.dauer_s, e.aussetzer), (1010, 35, 1));
        // noch offen nach zwei Antworten: kein Eintrag
        assert!(lauf(&mit(3, INTERNET_WEG, 2), false).liste.is_empty());
    }

    #[test]
    fn ruhezustand_ist_kein_ausfall() {
        let mut d = Daten::default();
        for t in [1000, 1005, 1010] {
            d.aufnehmen(&Sample { t, v: OK }, &|| false);
        }
        // zwei Verluste, dann 10 Minuten Schlaf, dann Verluste und Antworten
        d.aufnehmen(
            &Sample {
                t: 1015,
                v: INTERNET_WEG,
            },
            &|| false,
        );
        d.aufnehmen(
            &Sample {
                t: 1020,
                v: INTERNET_WEG,
            },
            &|| false,
        );
        d.aufnehmen(
            &Sample {
                t: 1620,
                v: INTERNET_WEG,
            },
            &|| false,
        );
        for t in [1625, 1630, 1635] {
            d.aufnehmen(&Sample { t, v: OK }, &|| false);
        }
        assert!(d.liste.is_empty());
        // offener Ausfall vor dem Schlaf endet beim letzten Verlust, nicht nach dem Schlaf
        let mut d = lauf(&mit(3, INTERNET_WEG, 0), false);
        d.aufnehmen(&Sample { t: 9000, v: OK }, &|| false);
        assert_eq!((d.liste[0].beginn, d.liste[0].dauer_s), (1010, 10));
    }

    #[test]
    fn zuordnung_aller_faelle() {
        let ort = |v| lauf(&mit(3, v, 3), false).liste.first().map(|e| e.schicht);
        assert_eq!(ort([LOSS, LOSS, LOSS]), Some(Ort::RouterOderWlan));
        assert_eq!(ort([1000, LOSS, LOSS]), Some(Ort::AnbieterOderWeiter));
        assert_eq!(ort([1000, LOSS, 1000]), Some(Ort::NurDns));
        assert_eq!(ort([SKIP, LOSS, LOSS]), Some(Ort::Unklar));
        // Router stumm, Internet geht: kein Ausfall
        assert_eq!(ort([LOSS, 1000, 1000]), None);
    }

    #[test]
    fn gegenprobe_verwirft_ausfall() {
        assert!(lauf(&mit(6, INTERNET_WEG, 5), true).liste.is_empty());
    }

    #[test]
    fn nachbildung_wlan_20_sekunden_aus() {
        // 20 s ohne WLAN im 5-s-Takt: 4 Stichproben, Router und Internet weg
        let mut r = vec![OK; 3];
        r.extend([[LOSS; 3]; 4]);
        r.extend([OK; 4]);
        let d = lauf(&r, false);
        assert_eq!(d.liste.len(), 1);
        assert_eq!(d.liste[0].schicht, Ort::RouterOderWlan);
        assert_eq!(d.liste[0].dauer_s, 20);
    }

    #[test]
    fn protokoll_haelt_200_eintraege() {
        let mut d = Daten::default();
        let mut t = 1000;
        for _ in 0..205 {
            for v in std::iter::repeat_n(INTERNET_WEG, 3).chain([OK; 3]) {
                d.aufnehmen(&Sample { t, v }, &|| false);
                t += 5;
            }
        }
        assert_eq!(d.liste.len(), 200);
        assert_eq!(d.liste[0].beginn, 1000 + 5 * 6 * 5);
    }

    #[test]
    fn bericht_ohne_adressen_und_namen() {
        let d = lauf(&mit(3, INTERNET_WEG, 5), false);
        let s = abschnitt(&d).unwrap() + &text_fuer_anbieter(&d, 1100);
        assert!(s.contains("Anbieter oder weiter weg") && s.contains("Summe je Woche"));
        assert!(s.contains("Dauer 15 s") && s.contains("kein ICMP"));
        let json = serde_json::to_string(&d).unwrap();
        for text in [s, json] {
            assert!(!text.contains("1.1.1.1"));
            // keine vier Zahlengruppen mit Punkten, also keine IPv4
            assert!(
                !text
                    .split(|c: char| !c.is_ascii_digit() && c != '.')
                    .any(|w| w.split('.').filter(|p| !p.is_empty()).count() == 4
                        && w.matches('.').count() == 3)
            );
        }
        assert!(text_fuer_anbieter(&Daten::default(), 5).contains("kein Ausfall"));
    }

    #[test]
    fn wochenanfang_ist_montag() {
        // Do 29.10.2026 gehört zur Woche ab Mo 26.10.2026
        assert!(netze::format_utc(woche_ab(1_793_000_000)).starts_with("Mo 26.10.2026, 00:00"));
    }
}

#[cfg(test)]
mod robust {
    use super::*;
    use crate::messen::LOSS;

    /// Springt die Uhr zurück, steht `ende` vor `beginn`: Dauer 0 statt Überlauf (Panik).
    #[test]
    fn uhr_springt_zurueck_ohne_panik() {
        let mut d: Daten = serde_json::from_str(
            r#"{"lauf":{"zuletzt":100,"aus":{"beginn":500,"ort":"nur_dns","gut":0,
            "aussetzer":0,"letzter_verlust":400,"gut_ab":0}}}"#,
        )
        .unwrap();
        d.aufnehmen(
            &Sample {
                t: 1000,
                v: [LOSS, 1, 1],
            },
            &|| false,
        );
        assert_eq!(d.liste.len(), 1);
        assert_eq!(d.liste[0].dauer_s, 0);
    }

    /// Eine Datei aus einer älteren Version (Felder fehlen) lädt weiter.
    #[test]
    fn alte_datei_ohne_felder_laedt() {
        let d: Daten = serde_json::from_str("{}").unwrap();
        assert!(d.liste.is_empty());
        let d: Daten = serde_json::from_str(
            r#"{"liste":[{"beginn":1,"ende":9,"dauer_s":8,"schicht":"unklar"}]}"#,
        )
        .unwrap();
        assert_eq!(d.liste[0].aussetzer, 0);
    }
}
