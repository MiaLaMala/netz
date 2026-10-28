//! `netzinfo wlan`: radio link of the current Wi-Fi connection (issue #34).
//!
//! No SSID and no BSSID here (that is #35). Every value is an `Option`: a platform or driver
//! that does not report a value leaves it empty, and the output says so.

use crate::explain;
use crate::hier;
use crate::netze;
use crate::pruefen::Klasse;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sicherheit {
    Offen,
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    Wpa2Wpa3Gemischt,
    Unbekannt,
}

impl Sicherheit {
    /// From the raw value of CoreWLAN's `CWSecurity` (kept as a number so this works everywhere).
    pub fn aus_cw(v: i64) -> Self {
        match v {
            0 => Self::Offen,
            1 | 6 => Self::Wep,
            2 | 3 | 7 | 8 => Self::Wpa,
            4 | 5 | 9 | 10 => Self::Wpa2,
            11 | 12 => Self::Wpa3,
            13 => Self::Wpa2Wpa3Gemischt,
            _ => Self::Unbekannt,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Offen => "offen (unverschlüsselt)",
            Self::Wep => "WEP",
            Self::Wpa => "WPA",
            Self::Wpa2 => "WPA2",
            Self::Wpa3 => "WPA3",
            Self::Wpa2Wpa3Gemischt => "WPA2/WPA3 gemischt",
            Self::Unbekannt => "unbekannt",
        }
    }

    /// Strength for the comparison with earlier visits. `None`: cannot be compared.
    /// Mixed counts like WPA3: a network that offers both is not a step down from WPA3.
    fn rang(self) -> Option<u8> {
        match self {
            Self::Offen => Some(0),
            Self::Wep => Some(1),
            Self::Wpa => Some(2),
            Self::Wpa2 => Some(3),
            Self::Wpa3 | Self::Wpa2Wpa3Gemischt => Some(4),
            Self::Unbekannt => None,
        }
    }
}

/// Open and WEP are an alarm, WPA is conspicuous, the rest is harmless.
pub fn sicherheit_note(s: Sicherheit) -> (Klasse, &'static str) {
    match s {
        Sicherheit::Offen => (
            Klasse::Alarm,
            "Das WLAN ist offen: Jeder in Reichweite kann mitlesen. Nutze nur verschlüsselte Verbindungen oder ein VPN.",
        ),
        Sicherheit::Wep => (
            Klasse::Alarm,
            "WEP gilt seit Jahren als geknackt, es schützt kaum. Stelle den Router auf WPA2 oder WPA3 um.",
        ),
        Sicherheit::Wpa => (
            Klasse::Auffaellig,
            "WPA (die erste Version) ist veraltet. Stelle den Router auf WPA2 oder WPA3 um.",
        ),
        _ => (Klasse::Harmlos, "Die Verschlüsselung ist zeitgemäß."),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Funkverbindung {
    /// e.g. "Wi-Fi 6 (802.11ax)".
    pub standard: Option<String>,
    /// "2,4 GHz", "5 GHz" or "6 GHz".
    pub band: Option<String>,
    pub kanal: Option<u32>,
    pub breite_mhz: Option<u32>,
    pub rssi_dbm: Option<i32>,
    pub rauschen_dbm: Option<i32>,
    pub snr_db: Option<i32>,
    pub rate_mbit: Option<f64>,
    pub sicherheit: Option<Sicherheit>,
    /// Two-letter country code of the regulatory domain.
    pub land: Option<String>,
}

/// Signal minus noise.
pub fn snr(rssi: Option<i32>, rauschen: Option<i32>) -> Option<i32> {
    Some(rssi? - rauschen?)
}

fn standard_name(phy: i64) -> Option<&'static str> {
    Some(match phy {
        1 => "802.11a",
        2 => "802.11b",
        3 => "802.11g",
        4 => "Wi-Fi 4 (802.11n)",
        5 => "Wi-Fi 5 (802.11ac)",
        6 => "Wi-Fi 6 (802.11ax)",
        _ => return None,
    })
}

fn band_name(freq_mhz: u32) -> &'static str {
    match freq_mhz {
        ..3000 => "2,4 GHz",
        3000..5925 => "5 GHz",
        _ => "6 GHz",
    }
}

/// Parses `iw dev <if> link`. `Ok(None)`: not connected. Security is not part of that output.
pub fn parse_iw(text: &str) -> Result<Option<Funkverbindung>, String> {
    if text.trim_start().starts_with("Not connected") {
        return Ok(None);
    }
    if !text.starts_with("Connected to") {
        return Err("Die Ausgabe von `iw` ließ sich nicht lesen. Prüfe, ob `iw dev <Schnittstelle> link` von Hand funktioniert.".into());
    }
    let val = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(str::trim)
    };
    let mut f = Funkverbindung::default();
    if let Some(freq) = val("freq:").and_then(|v| v.split('.').next()?.trim().parse::<u32>().ok()) {
        f.band = Some(band_name(freq).into());
        f.kanal = match freq {
            2484 => Some(14),
            2412..=2472 => Some((freq - 2407) / 5),
            5000..=5895 => Some((freq - 5000) / 5),
            5955.. => Some((freq - 5950) / 5),
            _ => None,
        };
    }
    f.rssi_dbm = val("signal:").and_then(|v| v.split_whitespace().next()?.parse().ok());
    if let Some(tx) = val("tx bitrate:") {
        f.rate_mbit = tx.split_whitespace().next().and_then(|v| v.parse().ok());
        f.breite_mhz = tx
            .split_whitespace()
            .find_map(|w| w.strip_suffix("MHz")?.parse().ok())
            .or(Some(20));
        f.standard = if tx.contains("HE-MCS") {
            Some("Wi-Fi 6 (802.11ax)".into())
        } else if tx.contains("VHT-MCS") {
            Some("Wi-Fi 5 (802.11ac)".into())
        } else if tx.contains(" MCS ") {
            Some("Wi-Fi 4 (802.11n)".into())
        } else {
            None
        };
    }
    Ok(Some(f))
}

/// Reads the current connection. `Ok(None)`: the Mac has no active Wi-Fi link (e.g. cable).
#[cfg(target_os = "macos")]
pub fn lesen() -> Result<Option<Funkverbindung>, String> {
    use objc2_core_wlan::CWWiFiClient;
    // SAFETY: plain getters on CoreWLAN objects, no pointers or callbacks are passed.
    unsafe {
        let Some(i) = CWWiFiClient::sharedWiFiClient().interface() else {
            return Ok(None);
        };
        let Some(ch) = i.wlanChannel() else {
            return Ok(None);
        };
        let rssi = i.rssiValue() as i32;
        let noise = i.noiseMeasurement() as i32;
        let rssi = (rssi != 0).then_some(rssi);
        let rauschen = (noise != 0).then_some(noise);
        let rate = i.transmitRate();
        Ok(Some(Funkverbindung {
            standard: standard_name(i.activePHYMode().0 as i64).map(Into::into),
            band: match ch.channelBand().0 {
                1 => Some("2,4 GHz".into()),
                2 => Some("5 GHz".into()),
                3 => Some("6 GHz".into()),
                _ => None,
            },
            kanal: u32::try_from(ch.channelNumber()).ok().filter(|k| *k > 0),
            breite_mhz: match ch.channelWidth().0 {
                1 => Some(20),
                2 => Some(40),
                3 => Some(80),
                4 => Some(160),
                _ => None,
            },
            rssi_dbm: rssi,
            rauschen_dbm: rauschen,
            snr_db: snr(rssi, rauschen),
            rate_mbit: (rate > 0.0).then_some(rate),
            sicherheit: Some(Sicherheit::aus_cw(i.security().0 as i64)),
            land: i.countryCode().map(|c| c.to_string()),
        }))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn lesen() -> Result<Option<Funkverbindung>, String> {
    Err("`netzinfo wlan` liest die Funkverbindung bisher nur auf dem Mac (CoreWLAN). Starte den Befehl auf einem Mac.".into())
}

/// What the store remembers per network (key: `Fingerprint::key()`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Merk {
    pub zeit: u64,
    pub letzte_sicherheit: Sicherheit,
    /// Strongest mode ever seen in this network.
    pub staerkste_sicherheit: Sicherheit,
    pub band: Option<String>,
    pub standard: Option<String>,
}

type Store = BTreeMap<String, Merk>;

/// Compares the security with the strongest one seen before in this network, then remembers
/// the visit. Only a step down counts, and only as `Auffaellig`. Unknown security is skipped.
fn aufzeichnen_in(
    store: &mut Store,
    key: &str,
    f: &Funkverbindung,
    now: u64,
) -> Option<(Klasse, String)> {
    let neu = f.sicherheit.filter(|s| s.rang().is_some())?;
    let alt = store.get(key).map(|m| m.staerkste_sicherheit);
    let schwaecher = alt.filter(|a| neu.rang() < a.rang());
    let staerkste = alt.filter(|a| a.rang() >= neu.rang()).unwrap_or(neu);
    store.insert(
        key.to_string(),
        Merk {
            zeit: now,
            letzte_sicherheit: neu,
            staerkste_sicherheit: staerkste,
            band: f.band.clone(),
            standard: f.standard.clone(),
        },
    );
    schwaecher.map(|a| {
        (
            Klasse::Auffaellig,
            format!(
                "Die WLAN-Sicherheit ist schwächer als bei früheren Besuchen: jetzt {}, vorher {}. Ein falscher Zugangspunkt kann schwächere Sicherheit vortäuschen. Prüfe, ob du den Router verändert hast.",
                neu.name(),
                a.name()
            ),
        )
    })
}

/// Reads the link and runs the rule for the network `key`. Any problem means no finding.
pub fn abstieg_pruefen(store_path: &Path, key: &str) -> Option<(Klasse, String)> {
    let f = lesen().ok()??;
    let mut store: Store = netze::load_json(store_path).ok()?;
    let r = aufzeichnen_in(&mut store, key, &f, netze::now());
    netze::save(store_path, &store).ok()?;
    r
}

/// Key of the current network if it is stored, with the store file next to `netze.json`.
fn bekanntes_netz() -> Option<(std::path::PathBuf, String)> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let path = netze::data_path().ok()?;
    netze::find(&netze::load(&path).ok()?, &fp)?;
    Some((path.with_file_name("wlan.json"), fp.key()))
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let json = args == ["--json"];
    if !json && let Some(a) = args.first() {
        return Err(format!(
            "`netzinfo wlan` kennt „{a}“ nicht. Erlaubt ist nur `--json`, `--erklaer` erklärt die Werte."
        ));
    }
    explain(
        explain_on,
        "Ich frage das WLAN-System des Mac (CoreWLAN) nach der aktuellen Verbindung. Dabei sende ich nichts ins Netz, ich lese nur, was die WLAN-Karte schon weiß.",
    );
    let f = lesen()?;
    if json {
        let text = serde_json::to_string_pretty(&f)
            .map_err(|e| format!("Die Werte ließen sich nicht umwandeln ({e})."))?;
        println!("{text}");
    }
    let Some(f) = f else {
        if !json {
            println!(
                "Keine Funkverbindung. Der Mac hängt nicht per WLAN im Netz, z. B. weil er am Kabel hängt."
            );
        }
        return Ok(());
    };
    let finding = bekanntes_netz().and_then(|(path, key)| {
        let mut store: Store = netze::load_json(&path).ok()?;
        let r = aufzeichnen_in(&mut store, &key, &f, netze::now());
        netze::save(&path, &store).ok()?;
        r
    });
    if json {
        return Ok(());
    }
    zeigen(&f, explain_on);
    if let Some((k, t)) = finding {
        println!("{}: {t}", klasse_name(k));
    }
    Ok(())
}

fn klasse_name(k: Klasse) -> &'static str {
    match k {
        Klasse::Harmlos => "Harmlos",
        Klasse::Auffaellig => "Auffällig",
        Klasse::Alarm => "Alarm",
    }
}

fn zeigen(f: &Funkverbindung, on: bool) {
    let leer = "nicht verfügbar";
    let zeile =
        |name: &str, v: Option<String>| println!("{name}: {}", v.as_deref().unwrap_or(leer));
    zeile("Standard", f.standard.clone());
    zeile("Band", f.band.clone());
    zeile("Kanal", f.kanal.map(|k| k.to_string()));
    zeile("Kanalbreite", f.breite_mhz.map(|b| format!("{b} MHz")));
    zeile("Signal", f.rssi_dbm.map(|v| format!("{v} dBm")));
    if let Some(v) = f.rssi_dbm {
        explain(
            on,
            &format!(
                "Das Signal liegt bei minus {} dBm. Je näher an null, desto besser, ab minus 70 wird es merklich schlechter.",
                -v
            ),
        );
    }
    zeile("Rauschen", f.rauschen_dbm.map(|v| format!("{v} dBm")));
    explain(
        on,
        "Rauschen ist der Störpegel auf dem Kanal (Nachbarnetze, Mikrowelle, Elektronik). Je kleiner (weiter von null weg), desto besser. Manche Treiber melden es nicht, dann bleibt das Feld leer.",
    );
    zeile(
        "Abstand zum Rauschen (SNR)",
        f.snr_db.map(|v| format!("{v} dB")),
    );
    if let Some(v) = f.snr_db {
        explain(
            on,
            &format!(
                "Der Abstand zum Rauschen beträgt {v} dB. Über 25 dB gilt als gut für Videoanrufe."
            ),
        );
    }
    zeile("Senderate", f.rate_mbit.map(|v| format!("{v:.0} MBit/s")));
    explain(
        on,
        "Die Senderate ist das, was Mac und Router gerade aushandeln, nicht die echte Geschwindigkeit. Echt kommt meist die Hälfte oder weniger an.",
    );
    zeile("Sicherheit", f.sicherheit.map(|s| s.name().to_string()));
    if let Some(s) = f.sicherheit {
        explain(on, sicherheit_note(s).1);
    }
    zeile("Land", f.land.clone());
    explain(
        on,
        "Standard, Band, Kanal und Breite bestimmen, wie schnell und wie weit das Funknetz trägt: 2,4 GHz reicht weiter, 5 und 6 GHz sind schneller, aber näher am Router. Breitere Kanäle sind schneller und störanfälliger.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mit(s: Sicherheit) -> Funkverbindung {
        Funkverbindung {
            sicherheit: Some(s),
            ..Funkverbindung::default()
        }
    }

    #[test]
    fn cw_security_mapping() {
        use Sicherheit::*;
        let erwartet = [
            (0, Offen),
            (1, Wep),
            (6, Wep),
            (2, Wpa),
            (3, Wpa),
            (7, Wpa),
            (4, Wpa2),
            (5, Wpa2),
            (9, Wpa2),
            (11, Wpa3),
            (12, Wpa3),
            (13, Wpa2Wpa3Gemischt),
            (14, Unbekannt),
            (i64::MAX, Unbekannt),
        ];
        for (cw, s) in erwartet {
            assert_eq!(Sicherheit::aus_cw(cw), s, "CWSecurity {cw}");
        }
    }

    #[test]
    fn snr_is_signal_minus_noise() {
        assert_eq!(snr(Some(-58), Some(-90)), Some(32));
        assert_eq!(snr(Some(-58), None), None);
        assert_eq!(snr(None, Some(-90)), None);
    }

    #[test]
    fn note_classes() {
        use Sicherheit::*;
        for (s, k) in [
            (Offen, Klasse::Alarm),
            (Wep, Klasse::Alarm),
            (Wpa, Klasse::Auffaellig),
            (Wpa2, Klasse::Harmlos),
            (Wpa3, Klasse::Harmlos),
            (Wpa2Wpa3Gemischt, Klasse::Harmlos),
            (Unbekannt, Klasse::Harmlos),
        ] {
            assert_eq!(sicherheit_note(s).0, k);
        }
    }

    #[test]
    fn step_down_rule() {
        use Sicherheit::*;
        let mut st = Store::new();
        // first visit: nothing to compare
        assert!(aufzeichnen_in(&mut st, "n", &mit(Wpa3), 1).is_none());
        // unknown security changes nothing
        assert!(aufzeichnen_in(&mut st, "n", &mit(Unbekannt), 2).is_none());
        assert_eq!(st["n"].zeit, 1);
        // WPA3 -> mixed is no step down
        assert!(aufzeichnen_in(&mut st, "n", &mit(Wpa2Wpa3Gemischt), 3).is_none());
        // WPA3 -> WPA2 is conspicuous, never an alarm, even down to open
        let (k, t) = aufzeichnen_in(&mut st, "n", &mit(Wpa2), 4).unwrap();
        assert_eq!(k, Klasse::Auffaellig);
        assert!(t.contains("WPA2") && t.contains("WPA3"));
        let (k, _) = aufzeichnen_in(&mut st, "n", &mit(Offen), 5).unwrap();
        assert_eq!(k, Klasse::Auffaellig);
        // the strongest mode stays, the last one is updated
        assert_eq!(st["n"].staerkste_sicherheit, Wpa3);
        assert_eq!(st["n"].letzte_sicherheit, Offen);
        // another network is independent; a network that only got stronger is fine
        assert!(aufzeichnen_in(&mut st, "m", &mit(Wpa2), 6).is_none());
        assert!(aufzeichnen_in(&mut st, "m", &mit(Wpa3), 7).is_none());
        assert_eq!(st["m"].staerkste_sicherheit, Wpa3);
    }

    const IW: &str = "Connected to 00:00:5e:00:53:01 (on wlan0)\n\tSSID: Beispiel\n\tfreq: 5180\n\tRX: 1000 bytes (10 packets)\n\tTX: 500 bytes (5 packets)\n\tsignal: -58 dBm\n\trx bitrate: 866.7 MBit/s VHT-MCS 9 80MHz short GI VHT-NSS 2\n\ttx bitrate: 780.0 MBit/s VHT-MCS 8 80MHz short GI VHT-NSS 2\n\tbss flags: short-slot-time\n";

    #[test]
    fn iw_link_output() {
        let f = parse_iw(IW).unwrap().unwrap();
        assert_eq!(f.band.as_deref(), Some("5 GHz"));
        assert_eq!(f.kanal, Some(36));
        assert_eq!(f.rssi_dbm, Some(-58));
        assert_eq!(f.rate_mbit, Some(780.0));
        assert_eq!(f.breite_mhz, Some(80));
        assert_eq!(f.standard.as_deref(), Some("Wi-Fi 5 (802.11ac)"));
        assert_eq!(f.sicherheit, None);
        let g = parse_iw("Connected to 00:00:5e:00:53:02 (on wlan0)\n\tfreq: 2437\n\tsignal: -71 dBm\n\ttx bitrate: 72.2 MBit/s MCS 7 short GI\n").unwrap().unwrap();
        assert_eq!((g.kanal, g.breite_mhz), (Some(6), Some(20)));
        assert_eq!(g.band.as_deref(), Some("2,4 GHz"));
        assert_eq!(g.standard.as_deref(), Some("Wi-Fi 4 (802.11n)"));
        assert_eq!(parse_iw("Not connected.\n").unwrap(), None);
        assert!(parse_iw("Quatsch").is_err());
    }
}
