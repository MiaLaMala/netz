//! The Mac app: a thin Tauri shell around the `netzinfo` library.
//!
//! Every command returns data or a German sentence; the UI (app/ui) shows the sentence where it
//! happened.

use netzinfo::netze::{self, Aktuell, AppFehler as Fehler, Netz};

#[tauri::command(async)]
fn dieses_netz() -> Result<Aktuell, Fehler> {
    netze::aktuell().map_err(Into::into)
}

#[tauri::command(async)]
fn netz_merken(name: String, meins: bool) -> Result<Netz, Fehler> {
    netze::merken(&name, meins).map_err(Into::into)
}

#[tauri::command(async)]
fn gemerkte_netze() -> Result<Vec<Netz>, Fehler> {
    netze::liste().map_err(Into::into)
}

#[tauri::command(async)]
fn netz_umbenennen(alt: String, neu: String) -> Result<Netz, Fehler> {
    netze::umbenennen(&alt, &neu).map_err(Into::into)
}

#[tauri::command(async)]
fn netz_markieren(name: String, meins: bool) -> Result<Netz, Fehler> {
    netze::markieren(&name, meins).map_err(Into::into)
}

#[tauri::command(async)]
fn pruefen() -> Result<netzinfo::pruefen::PruefErgebnis, Fehler> {
    netzinfo::pruefen::pruefen().map_err(Into::into)
}

#[tauri::command(async)]
fn messen_einmal() -> Result<netzinfo::messen::Probe, Fehler> {
    netzinfo::messen::messen_einmal().map_err(Into::into)
}

#[tauri::command(async)]
fn bericht(name: Option<String>) -> Result<netzinfo::messen::Bericht, Fehler> {
    netzinfo::messen::bericht_daten(name.as_deref()).map_err(Into::into)
}

#[tauri::command(async)]
fn geraete_suchen(
    cidr: Option<String>,
    gross: bool,
) -> Result<netzinfo::geraete::GeraeteErgebnis, Fehler> {
    netzinfo::geraete::suchen_mit(cidr.as_deref(), gross, true).map_err(Into::into)
}

/// Cards of the devices stored for this network, see issue #29. Sends nothing.
#[tauri::command(async)]
fn geraete_karten() -> Result<Vec<netzinfo::geraetekarte::Karte>, Fehler> {
    netzinfo::geraetekarte::karten_jetzt().map_err(Into::into)
}

/// Name and/or type given by the user; no scan overwrites it, see issue #29.
#[tauri::command(async)]
fn geraet_setzen(
    schluessel: String,
    name: Option<String>,
    typ: Option<String>,
) -> Result<(), Fehler> {
    netzinfo::geraetekarte::setzen_jetzt(&schluessel, name.as_deref(), typ.as_deref())
        .map_err(Into::into)
}

/// Hops to `ziel` (default 1.1.1.1), see issue #73.
#[tauri::command(async)]
fn pfad_messen(ziel: Option<String>) -> Result<netzinfo::pfad::PfadErgebnis, Fehler> {
    netzinfo::pfad::pfad_daten(ziel.as_deref().unwrap_or("1.1.1.1"), 20, 10, true)
        .map_err(Into::into)
}

/// Captive portal check, see issue #73.
#[tauri::command(async)]
fn portal_pruefen() -> netzinfo::portal::PortalErgebnis {
    netzinfo::portal::check()
}

/// "Wo hakt es?": four ping series, one verdict, see issue #31.
#[tauri::command(async)]
fn diagnose() -> Result<netzinfo::diagnose::DiagnoseErgebnis, Fehler> {
    netzinfo::diagnose::diagnose_daten().map_err(Into::into)
}

/// Opens the portal login page in the default browser. Fixed URL on purpose: the UI passes nothing.
#[tauri::command]
fn portal_im_browser_oeffnen() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("http://captive.apple.com")
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Der Browser ließ sich nicht öffnen ({e}). Öffne http://captive.apple.com von Hand."))
    }
    #[cfg(not(target_os = "macos"))]
    Err("Den Browser öffnet nur die Mac-App. Öffne http://captive.apple.com von Hand.".into())
}

#[tauri::command(async)]
fn dns_verschluesselt() -> Result<netzinfo::dnsverschl::DnsErgebnis, Fehler> {
    netzinfo::dnsverschl::dns_daten().map_err(Into::into)
}

/// Radio link of the current Wi-Fi connection, `None` without one, see issue #34.
#[tauri::command(async)]
fn wlan_lesen() -> Result<Option<netzinfo::wlan::Funkverbindung>, Fehler> {
    netzinfo::wlan::lesen().map_err(Into::into)
}

/// Active connections per program, see issue #39. Stores only in „meins“ networks or with `speichern`.
#[tauri::command(async)]
fn verbindungen_zeigen(speichern: bool) -> Result<netzinfo::verbindungen::Ergebnis, Fehler> {
    netzinfo::verbindungen::sammeln(speichern).map_err(Into::into)
}

#[tauri::command(async)]
fn verbindungen_loeschen() -> Result<bool, Fehler> {
    netzinfo::verbindungen::loeschen().map_err(Into::into)
}

/// Location permission for SSID and BSSID, see issue #35. Reading the state shows no dialog.
#[tauri::command]
fn ort_status() -> netzinfo::ort::OrtStatus {
    netzinfo::ort::status()
}

/// Shows the system dialog (first time only). Not `async`: Tauri then runs it on the main thread.
#[tauri::command]
fn ort_anfragen() -> Result<netzinfo::ort::OrtStatus, String> {
    netzinfo::ort::anfragen()
}

/// Opens the Location Services pane. Fixed URL on purpose: the UI passes nothing.
#[tauri::command]
fn ort_einstellungen_oeffnen() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices")
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Die Systemeinstellungen ließen sich nicht öffnen ({e}). Öffne sie von Hand unter Datenschutz & Sicherheit > Ortungsdienste."))
    }
    #[cfg(not(target_os = "macos"))]
    Err("Die Systemeinstellungen gibt es nur auf dem Mac.".into())
}

#[tauri::command(async)]
fn netz_vergessen(name: String) -> Result<(), Fehler> {
    netze::vergessen(&name).map_err(Into::into)
}

fn main() {
    let r = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            dieses_netz,
            netz_merken,
            gemerkte_netze,
            netz_vergessen,
            netz_umbenennen,
            netz_markieren,
            pruefen,
            messen_einmal,
            bericht,
            geraete_suchen,
            pfad_messen,
            portal_pruefen,
            diagnose,
            portal_im_browser_oeffnen,
            dns_verschluesselt,
            wlan_lesen,
            verbindungen_zeigen,
            verbindungen_loeschen,
            geraete_karten,
            geraet_setzen,
            ort_status,
            ort_anfragen,
            ort_einstellungen_oeffnen
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("Die App ließ sich nicht starten: {e}");
        std::process::exit(1);
    }
}
