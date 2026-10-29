//! `netzinfo wlan`: radio link of the current Wi-Fi connection (issue #34).
//!
//! SSID and BSSID only come with the location permission (`ort`, #35). Every value is an `Option`: a platform or driver
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
    /// Only with the location permission (`ort::status()` is `Erlaubt`).
    pub ssid: Option<String>,
    /// Raw BSSID, only in memory and for the screen. The store only gets `ort::bssid_hash`.
    pub bssid: Option<String>,
}

/// Signal minus noise.
pub fn snr(rssi: Option<i32>, rauschen: Option<i32>) -> Option<i32> {
    Some(rssi? - rauschen?)
}

#[cfg(target_os = "macos")]
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
    let mut f = Funkverbindung {
        ssid: val("SSID:").map(Into::into),
        bssid: text.split_whitespace().nth(2).map(Into::into),
        ..Funkverbindung::default()
    };
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
        let frei = crate::ort::status() == crate::ort::OrtStatus::Erlaubt;
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
            ssid: i.ssid().filter(|_| frei).map(|s| s.to_string()),
            bssid: i.bssid().filter(|_| frei).map(|s| s.to_string()),
        }))
    }
}

/// First interface of `iw dev` ("Interface wlan0"). `None`: the machine has no Wi-Fi card.
pub fn parse_iw_dev(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Interface "))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
}

/// Security from `wpa_cli status` (`key_mgmt=`). `None`: not readable, `key_mgmt=NONE` could be
/// open or WEP and stays unknown rather than raising a false alarm.
pub fn parse_wpa_status(text: &str) -> Option<Sicherheit> {
    let km = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("key_mgmt="))?;
    Some(if km.contains("SAE") {
        if km.contains("PSK") {
            Sicherheit::Wpa2Wpa3Gemischt
        } else {
            Sicherheit::Wpa3
        }
    } else if km.contains("WPA2") {
        Sicherheit::Wpa2
    } else if km.contains("WPA") {
        Sicherheit::Wpa
    } else {
        return None;
    })
}

#[cfg(target_os = "linux")]
fn befehl(prog: &str, args: &[&str]) -> std::io::Result<String> {
    let o = std::process::Command::new(prog).args(args).output()?;
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(target_os = "linux")]
pub fn lesen() -> Result<Option<Funkverbindung>, String> {
    let kein_iw = |e: std::io::Error| {
        match e.kind() {
        std::io::ErrorKind::NotFound => "Das Programm `iw` fehlt. Installiere das Paket `iw` (z. B. `sudo apt install iw`) und starte den Befehl neu.".to_string(),
        _ => format!("`iw` ließ sich nicht starten ({}).", crate::io_satz(&e)),
    }
    };
    let Some(ifname) = parse_iw_dev(&befehl("iw", &["dev"]).map_err(kein_iw)?) else {
        return Ok(None);
    };
    let Some(mut f) = parse_iw(&befehl("iw", &["dev", &ifname, "link"]).map_err(kein_iw)?)? else {
        return Ok(None);
    };
    // ponytail: only wpa_supplicant; NetworkManager (`nmcli`) as a second source if it is missed.
    f.sicherheit = befehl("wpa_cli", &["-i", &ifname, "status"])
        .ok()
        .and_then(|t| parse_wpa_status(&t));
    Ok(Some(f))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn lesen() -> Result<Option<Funkverbindung>, String> {
    Err("`netzinfo wlan` liest die Funkverbindung nur auf dem Mac (CoreWLAN) und unter Linux (iw). Starte den Befehl auf einem dieser Systeme.".into())
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
    /// SSID of the last visit (#35), empty without the location permission.
    #[serde(default)]
    pub ssid: Option<String>,
    #[serde(default)]
    pub meins: bool,
    /// Access points seen in this network, newest last. Only hashes, never the raw BSSID.
    #[serde(default)]
    pub funkpunkte: Vec<Funkpunkt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Funkpunkt {
    pub hash: String,
    /// First three bytes of the BSSID ("00:00:5e"): the maker of the radio.
    pub praefix: String,
    pub zuerst: u64,
    pub zuletzt: u64,
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
    let alt_merk = store.get(key).cloned();
    let alt = alt_merk.as_ref().map(|m| m.staerkste_sicherheit);
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
            ssid: alt_merk.as_ref().and_then(|m| m.ssid.clone()),
            meins: alt_merk.as_ref().is_some_and(|m| m.meins),
            funkpunkte: alt_merk.map(|m| m.funkpunkte).unwrap_or_default(),
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

fn router_von(key: &str) -> &str {
    key.split('|').next().unwrap_or("")
}

/// Rules for access points (#35), after `aufzeichnen_in` created the entry for `key`.
///
/// Same SSID as a „meins“ network with another router, and an access point that network never
/// had: „gleicher Name, anderes Netz“ (possible fake hotspot), conspicuous. In a „meins“ network,
/// an access point whose hash and maker prefix were not seen there: „neuer Funkpunkt“, conspicuous
/// (same prefix is roaming or mesh and harmless). Elsewhere (hotel, school) many access points are
/// normal and only remembered. Never an alarm: the BSSID changes with private addresses and mesh.
fn funkpunkt_aufzeichnen(
    store: &mut Store,
    key: &str,
    meins: bool,
    ssid: &str,
    bssid: [u8; 6],
    salz: &str,
    now: u64,
) -> Vec<(Klasse, String)> {
    let hash = crate::ort::bssid_hash(salz, bssid);
    let praefix = crate::ort::praefix(bssid);
    let mut out = Vec::new();
    let fremd = store.iter().any(|(k, m)| {
        k != key
            && m.meins
            && m.ssid.as_deref() == Some(ssid)
            && router_von(k) != router_von(key)
            && !m.funkpunkte.is_empty()
            && m.funkpunkte.iter().all(|p| p.hash != hash)
    });
    if fremd {
        out.push((
            Klasse::Auffaellig,
            format!("Gleicher Name, anderes Netz: Das WLAN „{ssid}“ kenne ich aus einem deiner Netze, aber mit anderem Router und anderem Funkpunkt. Das kann ein nachgebauter Hotspot sein. Prüfe, ob du mit dem richtigen WLAN verbunden bist."),
        ));
    }
    let Some(m) = store.get_mut(key) else {
        return out;
    };
    let bekannt = m.funkpunkte.iter().any(|p| p.hash == hash);
    let gleicher_hersteller = m.funkpunkte.iter().any(|p| p.praefix == praefix);
    if meins && !m.funkpunkte.is_empty() && !bekannt && !gleicher_hersteller {
        out.push((
            Klasse::Auffaellig,
            "Neuer Funkpunkt in einem deiner Netze, von einem anderen Hersteller als die bekannten. Bei einem neuen Repeater oder Router ist das richtig, sonst sieh nach, wer das WLAN anbietet.".into(),
        ));
    }
    match m.funkpunkte.iter_mut().find(|p| p.hash == hash) {
        Some(p) => p.zuletzt = now,
        None => m.funkpunkte.push(Funkpunkt {
            hash,
            praefix,
            zuerst: now,
            zuletzt: now,
        }),
    }
    // ponytail: at most 32 per network (hotel Wi-Fi has hundreds), oldest first out.
    if m.funkpunkte.len() > 32 {
        m.funkpunkte.remove(0);
    }
    m.ssid = Some(ssid.to_string());
    m.meins = meins;
    out
}

/// Reads the link and runs the rules for the network `key`: security step down (#34) and
/// access points (#35). Any problem, or no location permission, means no finding.
pub fn funk_pruefen(store_path: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let Ok(Some(f)) = lesen() else {
        return Vec::new();
    };
    let Ok(mut store) = netze::load_json::<Store>(store_path) else {
        return Vec::new();
    };
    let now = netze::now();
    let mut out: Vec<_> = aufzeichnen_in(&mut store, key, &f, now)
        .into_iter()
        .collect();
    if let (Some(ssid), Some(b)) = (
        &f.ssid,
        f.bssid.as_deref().and_then(crate::ort::bssid_bytes),
    ) && let Ok(salz) = crate::ort::salz(&store_path.with_file_name("salz.json"))
    {
        out.extend(funkpunkt_aufzeichnen(
            &mut store, key, meins, ssid, b, &salz, now,
        ));
    }
    let _ = netze::save(store_path, &store);
    out
}

/// Hash of the BSSID the Mac is connected to, for the fingerprint. `None` without permission.
pub fn bssid_hash_jetzt(salz_pfad: &Path) -> Option<String> {
    let b = crate::ort::bssid_bytes(lesen().ok()??.bssid.as_deref()?)?;
    Some(crate::ort::bssid_hash(
        &crate::ort::salz(salz_pfad).ok()?,
        b,
    ))
}

/// Key of the current network if it is stored, with the store file next to `netze.json`.
fn bekanntes_netz() -> Option<(std::path::PathBuf, String)> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let path = netze::data_path().ok()?;
    netze::find(&netze::load(&path).ok()?, &fp)?;
    Some((path.with_file_name("wlan.json"), fp.key()))
}

pub const OPTIONEN: &[&str] = &["--json", "--verlauf", "--umfeld"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "wlan")?;
    if let Some((a, rest)) = args.split_first()
        && a == "--verlauf"
    {
        return crate::wlanverlauf::run(rest, explain_on);
    }
    if let Some((a, rest)) = args.split_first()
        && a == "--umfeld"
    {
        return crate::kanal::run(rest, explain_on);
    }
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
    zeile("SSID", f.ssid.clone());
    zeile("Kennung (BSSID)", f.bssid.clone());
    if f.ssid.is_none() {
        println!("Standort: {}", crate::ort::status().satz());
        println!(
            "Hinweis: Name und Kennung zeigt macOS nur Apps mit Standortfreigabe. Die Kommandozeile hat ohne App-Bündel und Signatur keine, die Netzinfo-App zeigt beides."
        );
    }
    explain(
        on,
        "Die Kennung des Zugangspunkts (BSSID) ist die Hardware-Adresse des Funkgeräts. Zwei Netze mit gleichem Namen, aber anderer Kennung sind nicht dasselbe Netz. Der Standort wird nur gebraucht, weil macOS Netznamen als Ortsdaten behandelt. Netzinfo speichert keinen Standort.",
    );
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

    const MAC_A: &str = "00:00:5e:00:53:01"; // Router zu Hause
    const MAC_B: &str = "00:00:5e:00:53:02"; // gleicher Hersteller (Mesh-Knoten)
    const MAC_D: &str = "02:00:5e:00:53:03"; // anderer Hersteller-Präfix

    fn punkt(
        st: &mut Store,
        key: &str,
        meins: bool,
        ssid: &str,
        mac: &str,
        now: u64,
    ) -> Vec<(Klasse, String)> {
        // Der Eintrag entsteht in `aufzeichnen_in`, wie in `funk_pruefen`.
        aufzeichnen_in(st, key, &mit(Sicherheit::Wpa2), now);
        let b = crate::ort::bssid_bytes(mac).unwrap();
        funkpunkt_aufzeichnen(st, key, meins, ssid, b, "salz", now)
    }

    #[test]
    fn funkpunkte_im_meins_netz() {
        let mut st = Store::new();
        let k = "192.0.2.1|192.0.2.1||192.0.2.0/24";
        assert!(
            punkt(&mut st, k, true, "Beispiel", MAC_A, 1).is_empty(),
            "erster Besuch"
        );
        assert!(
            punkt(&mut st, k, true, "Beispiel", MAC_A, 2).is_empty(),
            "derselbe"
        );
        assert!(
            punkt(&mut st, k, true, "Beispiel", MAC_B, 3).is_empty(),
            "gleicher Hersteller: Mesh"
        );
        assert_eq!(st[k].funkpunkte.len(), 2);
        let r = punkt(&mut st, k, true, "Beispiel", MAC_D, 4);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, Klasse::Auffaellig, "nie Alarm");
        assert!(r[0].1.contains("Neuer Funkpunkt"));
        assert!(
            punkt(&mut st, k, true, "Beispiel", MAC_D, 5).is_empty(),
            "jetzt bekannt"
        );
    }

    #[test]
    fn funkpunkte_ausserhalb_von_meins_nur_merken() {
        let mut st = Store::new();
        let k = "198.51.100.1|198.51.100.1||198.51.100.0/24";
        for (i, m) in [MAC_A, MAC_D, MAC_B].into_iter().enumerate() {
            assert!(punkt(&mut st, k, false, "Gast", m, i as u64).is_empty());
        }
        assert_eq!(st[k].funkpunkte.len(), 3);
    }

    #[test]
    fn gleicher_name_anderes_netz() {
        let mut st = Store::new();
        let daheim = "192.0.2.1|192.0.2.1||192.0.2.0/24";
        let cafe = "203.0.113.1|203.0.113.1||203.0.113.0/24";
        punkt(&mut st, daheim, true, "Beispiel", MAC_A, 1);
        // anderer Name: nichts
        assert!(punkt(&mut st, cafe, false, "Anderes", MAC_D, 2).is_empty());
        // gleicher Name, anderer Router, anderer Funkpunkt: auffällig
        let r = punkt(&mut st, cafe, false, "Beispiel", MAC_D, 3);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, Klasse::Auffaellig);
        assert!(r[0].1.starts_with("Gleicher Name, anderes Netz"));
        // derselbe Funkpunkt (z. B. Repeater in beiden Netzen): nichts
        assert!(punkt(&mut st, cafe, false, "Beispiel", MAC_A, 4).is_empty());
        // gleiches Netz, anderer Funkpunkt ist Sache der Meins-Regel, nicht dieser
        assert!(punkt(&mut st, daheim, true, "Beispiel", MAC_B, 5).is_empty());
    }

    #[test]
    fn speicher_hat_nie_die_rohe_bssid() {
        let mut st = Store::new();
        punkt(&mut st, "k", true, "Beispiel", MAC_A, 1);
        let json = serde_json::to_string(&st).unwrap();
        assert!(!json.contains(MAC_A));
        assert!(json.contains(&crate::ort::bssid_hash(
            "salz",
            crate::ort::bssid_bytes(MAC_A).unwrap()
        )));
    }

    #[test]
    fn alter_speicher_ohne_neue_felder_laedt() {
        let alt = r#"{"k":{"zeit":1,"letzte_sicherheit":"Wpa2","staerkste_sicherheit":"Wpa3","band":null,"standard":null}}"#;
        let mut st: Store = serde_json::from_str(alt).unwrap();
        assert!(st["k"].funkpunkte.is_empty() && st["k"].ssid.is_none());
        // die Sicherheitsregel läuft weiter, die neuen Felder bleiben beim Fortschreiben erhalten
        assert!(aufzeichnen_in(&mut st, "k", &mit(Sicherheit::Wpa2), 2).is_some());
    }

    #[test]
    fn iw_dev_und_wpa_status() {
        let dev = "phy#0\n\tInterface wlan0\n\t\tifindex 3\n\t\taddr 00:00:5e:00:53:09\n\t\ttype managed\n";
        assert_eq!(parse_iw_dev(dev).as_deref(), Some("wlan0"));
        assert_eq!(parse_iw_dev(""), None);
        let wpa = |km: &str| parse_wpa_status(&format!("ssid=Beispielnetz\nkey_mgmt={km}\n"));
        assert_eq!(wpa("WPA2-PSK"), Some(Sicherheit::Wpa2));
        assert_eq!(wpa("SAE"), Some(Sicherheit::Wpa3));
        assert_eq!(wpa("WPA2-PSK SAE"), Some(Sicherheit::Wpa2Wpa3Gemischt));
        assert_eq!(wpa("WPA-PSK"), Some(Sicherheit::Wpa));
        assert_eq!(wpa("NONE"), None);
        assert_eq!(parse_wpa_status("Failed to connect"), None);
    }

    #[test]
    fn iw_gibt_ssid_und_bssid() {
        let f = parse_iw(IW).unwrap().unwrap();
        assert_eq!(f.ssid.as_deref(), Some("Beispiel"));
        assert_eq!(f.bssid.as_deref(), Some(MAC_A));
    }
}
