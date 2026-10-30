//! Standortfreigabe für SSID und BSSID (issue #35), plus der Hash, mit dem die BSSID gespeichert wird.
//!
//! macOS behandelt Netznamen als Ortsdaten: ohne Freigabe liefert CoreWLAN weder SSID noch BSSID.
//! Die Freigabe ordnet macOS über die Code-Signatur zu, die Kommandozeile bekommt sie nicht.

use crate::netze;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrtStatus {
    NichtGefragt,
    Erlaubt,
    Abgelehnt,
    Eingeschraenkt,
}

impl OrtStatus {
    /// Aus `CLAuthorizationStatus`: 0 offen, 1 eingeschränkt (Jugendschutz o. Ä.), 2 abgelehnt,
    /// 3 und 4 erlaubt. Unbekanntes zählt als nicht gefragt.
    pub fn aus_cl(v: i32) -> Self {
        match v {
            1 => Self::Eingeschraenkt,
            2 => Self::Abgelehnt,
            3 | 4 => Self::Erlaubt,
            _ => Self::NichtGefragt,
        }
    }

    pub fn satz(self) -> &'static str {
        match self {
            Self::NichtGefragt => {
                "Netzinfo hat noch nicht nach dem Standort gefragt, deshalb zeigt macOS Name und Kennung des WLANs nicht."
            }
            Self::Erlaubt => {
                "Der Standort ist freigegeben, Netzinfo liest Name und Kennung des WLANs. Der Standort selbst wird nicht gespeichert."
            }
            Self::Abgelehnt => {
                "Der Standort ist abgelehnt. Netzinfo funktioniert weiter, erkennt Netze aber ungenauer. Ändern geht in den Systemeinstellungen unter Datenschutz & Sicherheit > Ortungsdienste."
            }
            Self::Eingeschraenkt => {
                "macOS beschränkt den Standort auf diesem Mac (z. B. durch Bildschirmzeit oder ein Profil). Netzinfo kann ihn nicht selbst freigeben."
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2_core_location::CLLocationManager;
    use std::cell::RefCell;

    thread_local! {
        // Der Manager muss leben, bis der Nutzer im Dialog geantwortet hat.
        static MANAGER: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
    }

    pub fn status() -> i32 {
        // SAFETY: neuer Manager nur zum Lesen, kein Zeiger, kein Rückruf. (Die Klassenmethode ist veraltet.)
        unsafe { CLLocationManager::new().authorizationStatus().0 }
    }

    pub fn anfragen() {
        MANAGER.with(|m| {
            // SAFETY: neuer Manager, einfacher Aufruf ohne Zeiger. Der Systemdialog erscheint nur
            // für ein gebündeltes Programm mit `NSLocationWhenInUseUsageDescription`.
            let mgr = m
                .borrow_mut()
                .get_or_insert_with(|| unsafe { CLLocationManager::new() })
                .clone();
            unsafe { mgr.requestWhenInUseAuthorization() };
        });
    }
}

#[cfg(target_os = "macos")]
pub fn status() -> OrtStatus {
    OrtStatus::aus_cl(mac::status())
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> OrtStatus {
    OrtStatus::NichtGefragt
}

/// Löst den Systemdialog aus (nur beim ersten Mal, nur in einer signierten Bundle-App) und gibt
/// den Stand davor zurück. Die Antwort des Nutzers kommt später: danach `status()` noch mal lesen.
/// Muss auf dem Hauptthread laufen.
#[cfg(target_os = "macos")]
pub fn anfragen() -> Result<OrtStatus, String> {
    mac::anfragen();
    Ok(status())
}

#[cfg(not(target_os = "macos"))]
pub fn anfragen() -> Result<OrtStatus, String> {
    Err(
        "Die Standortfreigabe gibt es nur auf dem Mac. Starte die Netzinfo-App auf einem Mac."
            .into(),
    )
}

/// Die sechs Bytes einer MAC-Adresse. macOS lässt führende Nullen weg ("0:0:5e:0:53:1"), beides geht.
pub fn bssid_bytes(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut teile = text.trim().split(':');
    for b in &mut out {
        *b = u8::from_str_radix(teile.next()?, 16).ok()?;
    }
    teile.next().is_none().then_some(out)
}

/// Die ersten drei Bytes ("00:00:5e"): wer das Funkgerät gebaut hat, nicht welches.
pub fn praefix(b: [u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}", b[0], b[1], b[2])
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ponytail: HMAC-SHA256 aus `ring` (steckt schon über rustls im Cargo.lock, nichts Neues). Stabil
// über Rust-Versionen, anders als `DefaultHasher`. Eine MAC hat nur 48 Bit: wer Salz UND Speicher
// hat, kann raten. Das Salz liegt daher in einer eigenen Datei neben `netze.json`.
pub fn bssid_hash(salz: &str, bssid: [u8; 6]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, salz.as_bytes());
    hex(&hmac::sign(&key, &bssid).as_ref()[..16])
}

/// Salz dieser Installation. Fehlt die Datei, wird eins zufällig erzeugt und gespeichert.
pub fn salz(pfad: &Path) -> Result<String, String> {
    let _sperre = crate::sperre(pfad)?;
    let s: String = netze::load_json(pfad)?;
    if !s.is_empty() {
        return Ok(s);
    }
    let mut b = [0u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "Der Zufallsgenerator des Systems antwortet nicht, das Salz für die Kennung ließ sich nicht erzeugen. Versuche es noch mal.".to_string())?;
    let s = hex(&b);
    netze::save(pfad, &s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_aus_cl_werten() {
        use OrtStatus::*;
        for (v, s) in [
            (0, NichtGefragt),
            (1, Eingeschraenkt),
            (2, Abgelehnt),
            (3, Erlaubt),
            (4, Erlaubt),
            (99, NichtGefragt),
        ] {
            assert_eq!(OrtStatus::aus_cl(v), s, "CLAuthorizationStatus {v}");
        }
    }

    #[test]
    fn bssid_schreibweisen() {
        let b = bssid_bytes("00:00:5e:00:53:01").unwrap();
        assert_eq!(bssid_bytes("0:0:5E:0:53:1"), Some(b));
        assert_eq!(praefix(b), "00:00:5e");
        for falsch in [
            "",
            "00:00:5e:00:53",
            "00:00:5e:00:53:01:02",
            "zz:00:5e:00:53:01",
            "100:0:5e:0:53:1",
        ] {
            assert_eq!(bssid_bytes(falsch), None, "{falsch}");
        }
    }

    #[test]
    fn hash_ist_stabil_und_salzabhaengig() {
        let b = bssid_bytes("00:00:5e:00:53:01").unwrap();
        // fester Wert: fällt dieser Test um, würden gespeicherte Hashes ungültig
        assert_eq!(bssid_hash("salz-a", b), "be99d822ee4e3cbaf14477703dcf8f2b");
        assert_ne!(bssid_hash("salz-b", b), bssid_hash("salz-a", b));
        assert_ne!(
            bssid_hash("salz-a", bssid_bytes("00:00:5e:00:53:02").unwrap()),
            bssid_hash("salz-a", b)
        );
    }

    #[test]
    fn salz_wird_einmal_erzeugt() {
        let dir = std::env::temp_dir().join(format!("netzinfo-salz-{}", std::process::id()));
        let pfad = dir.join("salz.json");
        let a = salz(&pfad).unwrap();
        assert_eq!(a.len(), 32);
        assert_eq!(salz(&pfad).unwrap(), a);
        let _ = std::fs::remove_dir_all(dir);
    }
}
