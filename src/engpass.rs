//! `netzinfo engpass`: Leitung laut Router, WLAN und der am Mac gemessene Durchsatz nebeneinander,
//! der Engpass in einem Satz. Issue #207.
//!
//! Die Leitung ist die Sync-Rate, die der Router meldet (Fritz!Box per TR-064, sonst UPnP-IGD),
//! nicht der Vertrag. Den Vertragswert schickt nur ein Teil der Anbieter. Alle Fragen an den
//! Router sind Lesen. Die Bewertung (`bewerten`) ist eine reine Funktion ohne Ein- und Ausgabe.

use crate::upnp::tag;
use crate::{explain, fritzbox, hier, last, netze, portfreigaben, wlan};
use serde::Serialize;
use std::net::Ipv4Addr;

/// Nutzdurchsatz als Anteil der WLAN-Senderate.
// ponytail: grobe Faustregel, je nach Standard und Umfeld 50 bis 70 Prozent (Header, Wartezeiten,
// Wiederholungen, andere Geräte auf dem Kanal). Einen genaueren Wert liefert nur eine Messung.
const WLAN_FAKTOR: f64 = 0.6;
/// Eine Sync-Rate unter 90 Prozent der Vertragsrate gilt als „bleibt unter dem Vertrag“.
/// Ein bisschen darunter ist normal (Leitungsdämpfung, Rundung), 10 Prozent nicht mehr.
const VERTRAG_ANTEIL: f64 = 0.9;
/// Ein WLAN, dessen Nutzdurchsatz unter 90 Prozent des Sollwerts liegt, bremst. Gleiche Toleranz.
const WLAN_ANTEIL: f64 = 0.9;
/// Am Mac kommen mindestens 80 Prozent des Sollwerts an: Protokoll-Overhead (etwa 5 bis 10
/// Prozent) und die Streuung von networkQuality liegen darunter, mehr Verlust ist ein Befund.
const MAC_ANTEIL: f64 = 0.8;
/// Manche Router melden 4294967295 (u32::MAX) für „unbekannt“.
const UNBEKANNT_BPS: f64 = 4_294_967_295.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Quelle {
    Fritzbox,
    Upnp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Leitung {
    pub down_mbit: f64,
    pub up_mbit: f64,
    pub vertrag_down_mbit: Option<f64>,
    pub vertrag_up_mbit: Option<f64>,
    pub quelle: Quelle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Art {
    Anmeldung,
    Upnp,
    KeinRouter,
}

/// Warum der Router nichts gesagt hat; die App ergänzt je `art` den Ausweg.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LeitungFehlt {
    pub art: Art,
    pub satz: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wlan {
    pub rate_mbit: f64,
    pub nutz_mbit: f64,
    pub band: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engpass {
    Leitung,
    Wlan,
    /// Leitung und WLAN reichen, am Mac kommt trotzdem viel weniger an.
    Router,
    Keiner,
    Unklar,
}

/// Genau der Vertrag mit der Karte, siehe Kopf von `app/vorschau/engpass.js`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ergebnis {
    pub leitung: Option<Leitung>,
    pub leitung_fehlt: Option<LeitungFehlt>,
    pub wlan: Option<Wlan>,
    pub kabel: bool,
    pub mac_down_mbit: Option<f64>,
    pub engpass: Engpass,
    pub satz: String,
    pub rat: Option<String>,
}

// ---- Antworten lesen ----

/// Bit/s aus einem Feld als Mbit/s; 0 und „unbekannt“ sind keine Zahl.
fn mbit(xml: &str, feld: &str) -> Option<f64> {
    let b: f64 = tag(xml, feld)?.parse().ok()?;
    (b > 0.0 && b < UNBEKANNT_BPS).then_some(b / 1e6)
}

/// `GetCommonLinkProperties` (TR-064 und UPnP-IGD gleich): Sync-Rate runter und hoch.
/// Ohne Downstream-Wert gibt es keine Leitung.
pub fn link_lesen(xml: &str, vertrag: Option<&str>, quelle: Quelle) -> Option<Leitung> {
    Some(Leitung {
        down_mbit: mbit(xml, "NewLayer1DownstreamMaxBitRate")?,
        up_mbit: mbit(xml, "NewLayer1UpstreamMaxBitRate").unwrap_or(0.0),
        vertrag_down_mbit: vertrag.and_then(|v| mbit(v, "NewX_AVM-DE_TariffDataRateDownstream")),
        vertrag_up_mbit: vertrag.and_then(|v| mbit(v, "NewX_AVM-DE_TariffDataRateUpstream")),
        quelle,
    })
}

// ---- Bewertung ----

/// 116 -> „116“, 2,4 -> „2,4“.
fn zahl(x: f64) -> String {
    if x >= 10.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.1}").replace('.', ",")
    }
}

const BREITBAND: &str = "Das liegt beim Anbieter. Miss mit der Breitbandmessung der Bundesnetzagentur (breitbandmessung.de), wenn du eine Minderung willst.";

/// Nennt den Engpass in einem Satz. Kleinste Zahl gewinnt, aber nur mit Schwelle:
/// 1. Leitung unter Vertrag, 2. am Mac kommt, was kommen soll, 3. WLAN unter Soll,
/// 4. sonst bremst etwas zwischen Router und Mac. Sollwert ist der Vertrag, sonst die Leitung.
pub fn bewerten(
    leitung: Option<&Leitung>,
    wlan: Option<&Wlan>,
    kabel: bool,
    mac: Option<f64>,
) -> (Engpass, String, Option<String>) {
    let mac = mac.filter(|m| m.is_finite() && *m >= 0.0);
    let w = wlan.map(|w| w.nutz_mbit);
    let Some(l) = leitung else {
        let satz = match (w, mac) {
            (Some(w), Some(m)) => format!(
                "Das WLAN schafft etwa {}, am Mac kommen {} an. Ohne die Zahl des Routers bleibt offen, ob die Leitung mehr kann.",
                zahl(w),
                zahl(m)
            ),
            (None, Some(m)) => format!(
                "Am Mac kommen {} Mbit/s an. Ohne die Zahl des Routers bleibt offen, ob die Leitung mehr kann.",
                zahl(m)
            ),
            (Some(w), None) => format!(
                "Das WLAN schafft etwa {} Mbit/s. Ohne die Zahl des Routers und ohne Messung am Mac bleibt der Engpass offen.",
                zahl(w)
            ),
            (None, None) => {
                "Ohne die Zahl des Routers und ohne Messung am Mac bleibt der Engpass offen.".into()
            }
        };
        return (Engpass::Unklar, satz, None);
    };
    let (down, vertrag) = (l.down_mbit, l.vertrag_down_mbit);
    let soll = vertrag.map_or(down, |v| v.min(down));
    let leitung_satz = match vertrag {
        Some(v) => format!(
            "Gebucht sind {} Mbit/s, die Leitung schafft {}",
            zahl(v),
            zahl(down)
        ),
        None => format!("Die Leitung schafft {}", zahl(down)),
    };
    let wlan_bremst = w.is_some_and(|w| w < soll * WLAN_ANTEIL);
    let wlan_satz = || {
        format!(
            "{leitung_satz}, das WLAN nur etwa {}.",
            w.map_or(String::new(), zahl)
        )
    };
    let wlan_rat = || {
        if wlan.and_then(|w| w.band.as_deref()) == Some("2,4 GHz") {
            "Geh näher an den Router oder nimm das 5-GHz-Netz."
        } else {
            "Geh näher an den Router oder wechsle den Kanal."
        }
        .to_string()
    };
    // 1. Die Leitung bleibt unter dem Vertrag (nur mit Vertragswert).
    if vertrag.is_some_and(|v| down < v * VERTRAG_ANTEIL) {
        let v = vertrag.unwrap_or(0.0);
        return (
            Engpass::Leitung,
            format!(
                "Die Leitung schafft nur {} von {} gebuchten Mbit/s.",
                zahl(down),
                zahl(v)
            ),
            Some(BREITBAND.into()),
        );
    }
    let Some(m) = mac else {
        // Ohne Messung am Mac: nur ein klares WLAN-Urteil, sonst offen.
        if wlan_bremst {
            return (Engpass::Wlan, wlan_satz(), Some(wlan_rat()));
        }
        let wlan_teil = w.map_or(String::new(), |w| format!(", das WLAN etwa {}", zahl(w)));
        return (
            Engpass::Unklar,
            format!(
                "Die Leitung schafft {} Mbit/s{wlan_teil}. Was am Mac ankommt, zeigt „Messen“.",
                zahl(down)
            ),
            None,
        );
    };
    // 2. Es kommt an, was kommen soll.
    if m >= soll * MAC_ANTEIL {
        let ueber = if kabel { "über Kabel " } else { "" };
        return match vertrag {
            Some(v) => (
                Engpass::Keiner,
                format!(
                    "Gebucht sind {} Mbit/s, am Mac kommen {ueber}{} an. Du bekommst, wofür du zahlst.",
                    zahl(v),
                    zahl(m)
                ),
                None,
            ),
            // Ohne Vertragswert ist die Leitung die kleinste Zahl, aber ob sie zu wenig ist, weiß nur der Vertrag.
            None => (
                Engpass::Unklar,
                format!(
                    "Die Leitung schafft {} Mbit/s, am Mac kommen {ueber}{} an. Ob das dem Vertrag entspricht, meldet der Router nicht.",
                    zahl(down),
                    zahl(m)
                ),
                None,
            ),
        };
    }
    // 3. Das WLAN liegt unter dem Soll.
    if wlan_bremst {
        return (Engpass::Wlan, wlan_satz(), Some(wlan_rat()));
    }
    // 4. Leitung und WLAN reichen, am Mac kommt trotzdem viel weniger an.
    let davor = if kabel {
        "Die Leitung reicht"
    } else {
        "Leitung und WLAN reichen"
    };
    (
        Engpass::Router,
        format!("{davor}, am Mac kommen aber nur {} Mbit/s an. Dazwischen bremst etwas: der Router, das Kabel oder der Mac selbst.", zahl(m)),
        Some("Miss noch einmal ohne VPN und ohne andere Downloads. Bleibt es so, starte den Router neu.".into()),
    )
}

// ---- Einsammeln ----

fn privat(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

const SATZ_ANMELDUNG: &str = "Die Fritz!Box nennt die Leitung nur mit Anmeldung. Lege einen Zugang an (netzinfo fritzbox --zugang <Benutzer>) oder schalte „Statusinformationen über UPnP übertragen“ ein (Heimnetz, Netzwerk, Netzwerkeinstellungen).";
const SATZ_UPNP: &str = "Der Router nennt die Leitung nicht per UPnP. Schalte UPnP im Router ein oder sieh in seiner Oberfläche nach der Sync-Rate.";

fn fehlt(art: Art, satz: &str) -> LeitungFehlt {
    LeitungFehlt {
        art,
        satz: satz.into(),
    }
}

/// Fragt den Router (nur Lesen). Erst die Fritz!Box mit Zugang (liefert auch den Vertrag),
/// dann UPnP ohne Anmeldung.
fn leitung_holen(gw: Option<Ipv4Addr>, meins: bool) -> Result<Leitung, LeitungFehlt> {
    let gw = gw.filter(|g| privat(*g)).ok_or_else(|| {
        fehlt(
            Art::KeinRouter,
            "Netzinfo findet in diesem Netz keinen Router, den es fragen kann.",
        )
    })?;
    if !meins {
        return Err(fehlt(
            Art::KeinRouter,
            "Netzinfo fragt den Router nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es wurde nichts gesendet.",
        ));
    }
    let mut anmeldung = None;
    if let Some(z) = fritzbox::zugang_lesen().ok().flatten() {
        match fritzbox::leitung_xml(gw, &z) {
            Ok((link, addon)) => {
                if let Some(l) = link_lesen(&link, addon.as_deref(), Quelle::Fritzbox) {
                    return Ok(l);
                }
            }
            Err((fritzbox::Zustand::ZugangFalsch, satz)) => anmeldung = Some(satz),
            Err(_) => {}
        }
    }
    if let Ok(Some(xml)) = portfreigaben::link_xml(gw)
        && let Some(l) = link_lesen(&xml, None, Quelle::Upnp)
    {
        return Ok(l);
    }
    Err(match anmeldung {
        Some(satz) => LeitungFehlt {
            art: Art::Anmeldung,
            satz,
        },
        None if fritzbox::ist_fritzbox(gw) => fehlt(Art::Anmeldung, SATZ_ANMELDUNG),
        None => fehlt(Art::Upnp, SATZ_UPNP),
    })
}

/// `(wlan, kabel)`. Kein Funk (`Ok(None)`) heißt Kabel; ein Lesefehler heißt „unbekannt“.
fn wlan_holen() -> (Option<Wlan>, bool) {
    match wlan::lesen() {
        Ok(Some(f)) => (
            f.rate_mbit.map(|r| Wlan {
                rate_mbit: r,
                nutz_mbit: r * WLAN_FAKTOR,
                band: f.band,
            }),
            false,
        ),
        Ok(None) => (None, true),
        Err(_) => (None, false),
    }
}

/// Für die App und die Kommandozeile. `mac_down_mbit`: was am Mac ankommt (App: `last_messen`).
pub fn lesen(mac_down_mbit: Option<f64>) -> Result<Ergebnis, String> {
    let info = hier::collect()?;
    let (wlan, kabel) = wlan_holen();
    let (leitung, leitung_fehlt) = match leitung_holen(info.router, netze::current_is_meins()) {
        Ok(l) => (Some(l), None),
        Err(f) => (None, Some(f)),
    };
    let (engpass, satz, rat) = bewerten(leitung.as_ref(), wlan.as_ref(), kabel, mac_down_mbit);
    Ok(Ergebnis {
        leitung,
        leitung_fehlt,
        wlan,
        kabel,
        mac_down_mbit,
        engpass,
        satz,
        rat,
    })
}

// ---- Kommandozeile ----

pub const OPTIONEN: &[&str] = &["--start", "--trotzdem"];

const MARKE: &str = " (Engpass)";

/// Die Ausgabe von `netzinfo engpass`. `mac_zeit`: wann der Lauf war, `None` bei frischer Messung.
fn ausgabe(e: &Ergebnis, mac_zeit: Option<u64>, mac_fehlt: &str, erklaer: bool) -> String {
    let zeile = |name: &str, wert: Option<f64>, text: &str, marke: bool| {
        let w = wert.map_or("-".to_string(), zahl);
        let text = format!("{text}{}", if marke { MARKE } else { "" });
        crate::umbruch(&text, &format!("{name:<8}{w:>7}  "), &" ".repeat(17)) + "\n"
    };
    let ek = |t: &str| {
        if erklaer {
            crate::umbruch(
                t,
                crate::ERKLAERUNG,
                &" ".repeat(crate::ERKLAERUNG.chars().count()),
            ) + "\n"
        } else {
            String::new()
        }
    };
    let mut s = format!("{:<8}{:>7}\n", "", "Mbit/s");
    s += &match (&e.leitung, &e.leitung_fehlt) {
        (Some(l), _) => {
            let quelle = match l.quelle {
                Quelle::Fritzbox => "Fritz!Box, TR-064",
                Quelle::Upnp => "Router, UPnP",
            };
            let vertrag = l
                .vertrag_down_mbit
                .map_or(String::new(), |v| format!(", gebucht {}", zahl(v)));
            zeile(
                "Leitung",
                Some(l.down_mbit),
                &format!("runter ({quelle}{vertrag}), hoch {}", zahl(l.up_mbit)),
                e.engpass == Engpass::Leitung,
            )
        }
        (None, f) => zeile(
            "Leitung",
            None,
            f.as_ref().map_or("nicht bekannt", |f| f.satz.as_str()),
            false,
        ),
    };
    s += &ek(
        "Die Leitung ist die Sync-Rate, die der Router meldet: so schnell reden Router und Anbieter miteinander. Das ist nicht der Vertrag. Den Vertragswert schicken nur manche Anbieter, deshalb steht „gebucht“ nur, wenn er da ist.",
    );
    s += &match &e.wlan {
        Some(w) => zeile(
            "WLAN",
            Some(w.nutz_mbit),
            &format!(
                "nutzbar (Senderate {}{})",
                zahl(w.rate_mbit),
                w.band.as_ref().map_or(String::new(), |b| format!(", {b}"))
            ),
            e.engpass == Engpass::Wlan,
        ),
        None if e.kabel => zeile("WLAN", None, "Kabel, die Zeile entfällt", false),
        None => zeile("WLAN", None, "Senderate nicht lesbar", false),
    };
    s += &ek(
        "Die Senderate ist die Geschwindigkeit des Funks auf dem Papier. Davon bleiben im Alltag etwa 60 Prozent übrig, weil Verwaltungsdaten, Wiederholungen und andere Geräte auf dem Kanal Zeit kosten. Deshalb zeigt die Zeile den umgerechneten Wert, grob geschätzt.",
    );
    s += &match (e.mac_down_mbit, mac_zeit) {
        (Some(m), Some(z)) => zeile(
            "Am Mac",
            Some(m),
            &format!("gemessen am {}", netze::format_utc(z)),
            e.engpass == Engpass::Router,
        ),
        (Some(m), None) => zeile(
            "Am Mac",
            Some(m),
            "gerade gemessen",
            e.engpass == Engpass::Router,
        ),
        (None, _) => zeile("Am Mac", None, mac_fehlt, false),
    };
    s += &ek(
        "„Am Mac“ misst Apples networkQuality: so viel kommt wirklich im Mac an. Es belastet die Leitung etwa 30 Sekunden, darum startet es nur mit --start.",
    );
    s += "\n";
    s += &crate::umbruch(&e.satz, "", "");
    s += "\n";
    if let Some(r) = &e.rat {
        s += &crate::umbruch(r, "", "");
        s += "\n";
    }
    s
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "engpass")?;
    let (start, trotzdem) = (
        args.iter().any(|a| a == "--start"),
        args.iter().any(|a| a == "--trotzdem"),
    );
    explain(
        explain_on,
        "Ich frage deinen Router nach der Leitung (nur Lesen), lese die WLAN-Senderate aus dem Mac und lege beides neben das, was am Mac ankommt. Die kleinste Zahl ist der Engpass. Ohne --start nehme ich den letzten Lauf von `netzinfo last` aus diesem Netz.",
    );
    let (lauf, mac_fehlt) = if start {
        if last::hotspot() && !trotzdem {
            return Err(format!(
                "{} Wenn das gewollt ist: netzinfo engpass --start --trotzdem",
                last::HOTSPOT_FRAGE
            ));
        }
        println!("{}\nMesse, das dauert etwa 30 Sekunden …", last::HINWEIS);
        (Some(last::messen_und_speichern(trotzdem)?), "")
    } else if cfg!(target_os = "macos") {
        (
            last::letzter(),
            "nicht gemessen. Starte mit: netzinfo engpass --start",
        )
    } else {
        (
            None,
            "gibt es hier nicht: networkQuality läuft nur auf dem Mac",
        )
    };
    let mac = lauf
        .as_ref()
        .and_then(|l| l.ergebnis.down_bps)
        .map(|b| b / 1e6);
    let e = lesen(mac)?;
    print!(
        "{}",
        ausgabe(
            &e,
            lauf.filter(|_| !start).map(|l| l.zeit),
            mac_fehlt,
            explain_on
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Beispielwerte, keine echten.
    const LINK: &str = "<s:Envelope><s:Body><u:GetCommonLinkPropertiesResponse><NewWANAccessType>DSL</NewWANAccessType><NewLayer1UpstreamMaxBitRate>42000000</NewLayer1UpstreamMaxBitRate><NewLayer1DownstreamMaxBitRate>116000000</NewLayer1DownstreamMaxBitRate><NewPhysicalLinkStatus>Up</NewPhysicalLinkStatus></u:GetCommonLinkPropertiesResponse></s:Body></s:Envelope>";
    const ADDON: &str = "<u:X_AVM-DE_GetAddonInfosResponse><NewX_AVM-DE_TariffDataRateDownstream>100000000</NewX_AVM-DE_TariffDataRateDownstream><NewX_AVM-DE_TariffDataRateUpstream>40000000</NewX_AVM-DE_TariffDataRateUpstream><NewX_AVM-DE_L2Downstream>0</NewX_AVM-DE_L2Downstream></u:X_AVM-DE_GetAddonInfosResponse>";
    const ADDON_LEER: &str = "<NewX_AVM-DE_TariffDataRateDownstream>0</NewX_AVM-DE_TariffDataRateDownstream><NewX_AVM-DE_TariffDataRateUpstream>0</NewX_AVM-DE_TariffDataRateUpstream>";
    const UPNP: &str = "<NewLayer1UpstreamMaxBitRate>4294967295</NewLayer1UpstreamMaxBitRate><NewLayer1DownstreamMaxBitRate>95000000</NewLayer1DownstreamMaxBitRate>";

    fn leitung(down: f64, up: f64, vertrag: Option<f64>) -> Leitung {
        Leitung {
            down_mbit: down,
            up_mbit: up,
            vertrag_down_mbit: vertrag,
            vertrag_up_mbit: None,
            quelle: Quelle::Fritzbox,
        }
    }

    fn funk(rate: f64, band: &str) -> Wlan {
        Wlan {
            rate_mbit: rate,
            nutz_mbit: rate * WLAN_FAKTOR,
            band: Some(band.into()),
        }
    }

    #[test]
    fn fritzbox_antworten_werden_gelesen() {
        let l = link_lesen(LINK, Some(ADDON), Quelle::Fritzbox).unwrap();
        assert_eq!(
            (
                l.down_mbit,
                l.up_mbit,
                l.vertrag_down_mbit,
                l.vertrag_up_mbit
            ),
            (116.0, 42.0, Some(100.0), Some(40.0))
        );
        // „0“ heißt: der Anbieter schickt keinen Vertragswert
        let l = link_lesen(LINK, Some(ADDON_LEER), Quelle::Fritzbox).unwrap();
        assert_eq!((l.vertrag_down_mbit, l.vertrag_up_mbit), (None, None));
        // ohne Recht für die Zusatzfrage bleibt die Leitung
        assert_eq!(
            link_lesen(LINK, None, Quelle::Fritzbox)
                .unwrap()
                .vertrag_down_mbit,
            None
        );
    }

    #[test]
    fn upnp_antwort_mit_unbekanntem_upstream() {
        let l = link_lesen(UPNP, None, Quelle::Upnp).unwrap();
        assert_eq!(
            (l.down_mbit, l.up_mbit, l.quelle),
            (95.0, 0.0, Quelle::Upnp)
        );
        assert!(link_lesen("<x>kaputt</x>", None, Quelle::Upnp).is_none());
        assert!(
            link_lesen(
                "<NewLayer1DownstreamMaxBitRate>0</NewLayer1DownstreamMaxBitRate>",
                None,
                Quelle::Upnp
            )
            .is_none()
        );
    }

    #[test]
    fn wlan_bremst() {
        let (e, satz, rat) = bewerten(
            Some(&leitung(116.0, 42.0, Some(100.0))),
            Some(&funk(72.0, "2,4 GHz")),
            false,
            Some(38.0),
        );
        assert_eq!(e, Engpass::Wlan);
        assert_eq!(
            satz,
            "Gebucht sind 100 Mbit/s, die Leitung schafft 116, das WLAN nur etwa 43."
        );
        assert!(rat.unwrap().contains("5-GHz"));
        // ohne Vertrag und im 5-GHz-Band: anderer Rat, kein „gebucht“
        let (e, satz, rat) = bewerten(
            Some(&leitung(100.0, 20.0, None)),
            Some(&funk(60.0, "5 GHz")),
            false,
            Some(30.0),
        );
        assert_eq!(e, Engpass::Wlan);
        assert!(
            !satz.contains("Gebucht") && satz.contains("das WLAN nur etwa 36"),
            "{satz}"
        );
        assert!(rat.unwrap().contains("Kanal"));
    }

    #[test]
    fn leitung_unter_vertrag() {
        let (e, satz, rat) = bewerten(
            Some(&leitung(16.0, 2.4, Some(50.0))),
            Some(&funk(866.0, "5 GHz")),
            false,
            Some(15.0),
        );
        assert_eq!(e, Engpass::Leitung);
        assert_eq!(satz, "Die Leitung schafft nur 16 von 50 gebuchten Mbit/s.");
        assert!(rat.unwrap().contains("Breitbandmessung"));
        // auch ohne Messung am Mac, an der Grenze (90 Prozent) nicht mehr
        assert_eq!(
            bewerten(Some(&leitung(44.0, 5.0, Some(50.0))), None, true, None).0,
            Engpass::Leitung
        );
        assert_ne!(
            bewerten(
                Some(&leitung(45.0, 5.0, Some(50.0))),
                None,
                true,
                Some(44.0)
            )
            .0,
            Engpass::Leitung
        );
    }

    #[test]
    fn alles_gut() {
        let (e, satz, rat) = bewerten(
            Some(&leitung(116.0, 42.0, Some(100.0))),
            Some(&funk(866.0, "5 GHz")),
            false,
            Some(104.0),
        );
        assert_eq!(e, Engpass::Keiner);
        assert_eq!(
            satz,
            "Gebucht sind 100 Mbit/s, am Mac kommen 104 an. Du bekommst, wofür du zahlst."
        );
        assert!(rat.is_none());
        // 80 Prozent des Sollwerts reichen, 79 nicht
        assert_eq!(
            bewerten(
                Some(&leitung(116.0, 42.0, Some(100.0))),
                Some(&funk(866.0, "5 GHz")),
                false,
                Some(80.0)
            )
            .0,
            Engpass::Keiner
        );
        assert_eq!(
            bewerten(
                Some(&leitung(116.0, 42.0, Some(100.0))),
                Some(&funk(866.0, "5 GHz")),
                false,
                Some(79.0)
            )
            .0,
            Engpass::Router
        );
    }

    #[test]
    fn dazwischen_bremst_etwas() {
        let (e, satz, rat) = bewerten(
            Some(&leitung(116.0, 42.0, Some(100.0))),
            Some(&funk(866.0, "5 GHz")),
            false,
            Some(30.0),
        );
        assert_eq!(e, Engpass::Router);
        assert!(
            satz.starts_with("Leitung und WLAN reichen, am Mac kommen aber nur 30 Mbit/s an."),
            "{satz}"
        );
        assert!(rat.is_some());
    }

    #[test]
    fn unklar_ohne_router_oder_ohne_messung() {
        let (e, satz, rat) = bewerten(None, Some(&funk(866.0, "5 GHz")), false, Some(94.0));
        assert_eq!(e, Engpass::Unklar);
        assert_eq!(
            satz,
            "Das WLAN schafft etwa 520, am Mac kommen 94 an. Ohne die Zahl des Routers bleibt offen, ob die Leitung mehr kann."
        );
        assert!(rat.is_none());
        let (e, satz, _) = bewerten(
            Some(&leitung(116.0, 42.0, Some(100.0))),
            Some(&funk(866.0, "5 GHz")),
            false,
            None,
        );
        assert_eq!(e, Engpass::Unklar);
        assert_eq!(
            satz,
            "Die Leitung schafft 116 Mbit/s, das WLAN etwa 520. Was am Mac ankommt, zeigt „Messen“."
        );
        // ohne Vertragswert bleibt „reicht das für den Vertrag?“ offen, auch bei guter Messung
        let (e, satz, _) = bewerten(Some(&leitung(95.0, 0.0, None)), None, true, Some(90.0));
        assert_eq!(e, Engpass::Unklar);
        assert!(satz.contains("Ob das dem Vertrag entspricht"), "{satz}");
        assert_eq!(bewerten(None, None, true, None).0, Engpass::Unklar);
        // kaputte Messung zählt als nicht gemessen
        assert_eq!(
            bewerten(Some(&leitung(95.0, 0.0, None)), None, true, Some(f64::NAN)).0,
            Engpass::Unklar
        );
    }

    #[test]
    fn kabel() {
        let (e, satz, _) = bewerten(
            Some(&leitung(250.0, 40.0, Some(250.0))),
            None,
            true,
            Some(231.0),
        );
        assert_eq!(e, Engpass::Keiner);
        assert_eq!(
            satz,
            "Gebucht sind 250 Mbit/s, am Mac kommen über Kabel 231 an. Du bekommst, wofür du zahlst."
        );
        let (e, satz, _) = bewerten(
            Some(&leitung(250.0, 40.0, Some(250.0))),
            None,
            true,
            Some(60.0),
        );
        assert_eq!(e, Engpass::Router);
        assert!(satz.starts_with("Die Leitung reicht, am Mac"), "{satz}");
    }

    #[test]
    fn zahlen_deutsch() {
        assert_eq!(
            (
                zahl(116.4).as_str(),
                zahl(2.4).as_str(),
                zahl(9.96).as_str()
            ),
            ("116", "2,4", "10,0")
        );
    }

    #[test]
    fn kein_router_oder_fremde_adresse_sendet_nichts() {
        assert_eq!(leitung_holen(None, true).unwrap_err().art, Art::KeinRouter);
        assert_eq!(
            leitung_holen(Some("203.0.113.1".parse().unwrap()), true)
                .unwrap_err()
                .art,
            Art::KeinRouter
        );
        let f = leitung_holen(Some("192.168.0.1".parse().unwrap()), false).unwrap_err();
        assert!(f.art == Art::KeinRouter && f.satz.contains("nichts gesendet"));
    }

    #[test]
    fn vertrag_als_json() {
        let e = Ergebnis {
            leitung: None,
            leitung_fehlt: Some(fehlt(Art::KeinRouter, "x")),
            wlan: Some(funk(72.0, "2,4 GHz")),
            kabel: false,
            mac_down_mbit: None,
            engpass: Engpass::Unklar,
            satz: "s".into(),
            rat: None,
        };
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["leitung_fehlt"]["art"], "kein_router");
        assert_eq!(j["engpass"], "unklar");
        assert_eq!(j["wlan"]["band"], "2,4 GHz");
        assert!(j["mac_down_mbit"].is_null() && j["leitung"].is_null());
        assert_eq!(serde_json::to_value(Quelle::Fritzbox).unwrap(), "fritzbox");
    }

    fn beispiel() -> Ergebnis {
        let l = link_lesen(LINK, Some(ADDON), Quelle::Fritzbox);
        let w = Some(funk(72.0, "2,4 GHz"));
        let (engpass, satz, rat) = bewerten(l.as_ref(), w.as_ref(), false, Some(38.0));
        Ergebnis {
            leitung: l,
            leitung_fehlt: None,
            wlan: w,
            kabel: false,
            mac_down_mbit: Some(38.0),
            engpass,
            satz,
            rat,
        }
    }

    #[test]
    fn ausgabe_mit_beispieldaten() {
        let e = beispiel();
        let kurz = ausgabe(&e, Some(1_793_309_420), "", false);
        let lang = ausgabe(&e, Some(1_793_309_420), "", true);
        // Hilfe zum Abschreiben ins Issue: cargo test beispielausgabe -- --nocapture
        println!("=== ohne --erklaer ===\n{kurz}\n=== mit --erklaer ===\n{lang}");
        assert!(
            kurz.contains("Leitung     116  runter (Fritz!Box, TR-064, gebucht 100), hoch 42"),
            "{kurz}"
        );
        assert!(
            kurz.contains("WLAN         43  nutzbar (Senderate 72, 2,4 GHz) (Engpass)"),
            "{kurz}"
        );
        assert!(kurz.contains("Am Mac       38  gemessen am"), "{kurz}");
        assert!(kurz.contains("Geh näher an den Router oder nimm das 5-GHz-Netz."));
        assert!(!kurz.contains("Erklärung"));
        assert!(lang.contains("nicht der Vertrag") && lang.contains("60 Prozent"));
        for z in lang.lines() {
            assert!(z.chars().count() <= crate::ZEILE_MAX, "{z}");
        }
        // ohne Router: die Zeile sagt, was fehlt, statt zu raten
        let mut ohne = e;
        ohne.leitung = None;
        ohne.leitung_fehlt = Some(fehlt(Art::Upnp, SATZ_UPNP));
        ohne.mac_down_mbit = None;
        let s = ausgabe(
            &ohne,
            None,
            "nicht gemessen. Starte mit: netzinfo engpass --start",
            false,
        );
        assert!(
            s.contains("Leitung       -  Der Router nennt die Leitung nicht per UPnP."),
            "{s}"
        );
        assert!(s.contains("Starte mit: netzinfo engpass --start"));
    }
}
