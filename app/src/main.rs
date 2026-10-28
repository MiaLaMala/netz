//! The Mac app: a thin Tauri shell around the `netzinfo` library.
//!
//! Every command returns data or a German sentence; the UI (app/ui) shows the sentence where it
//! happened.

use netzinfo::netze::{self, Aktuell, Netz};

#[tauri::command(async)]
fn dieses_netz() -> Result<Aktuell, String> {
    netze::aktuell()
}

#[tauri::command(async)]
fn netz_merken(name: String, meins: bool) -> Result<Netz, String> {
    netze::merken(&name, meins)
}

#[tauri::command(async)]
fn gemerkte_netze() -> Result<Vec<Netz>, String> {
    netze::liste()
}

#[tauri::command(async)]
fn netz_umbenennen(alt: String, neu: String) -> Result<Netz, String> {
    netze::umbenennen(&alt, &neu)
}

#[tauri::command(async)]
fn netz_markieren(name: String, meins: bool) -> Result<Netz, String> {
    netze::markieren(&name, meins)
}

#[tauri::command(async)]
fn pruefen() -> Result<netzinfo::pruefen::PruefErgebnis, String> {
    netzinfo::pruefen::pruefen()
}

#[tauri::command(async)]
fn messen_einmal() -> Result<netzinfo::messen::Probe, String> {
    netzinfo::messen::messen_einmal()
}

#[tauri::command(async)]
fn bericht(name: Option<String>) -> Result<netzinfo::messen::Bericht, String> {
    netzinfo::messen::bericht_daten(name.as_deref())
}

#[tauri::command(async)]
fn geraete_suchen(
    cidr: Option<String>,
    gross: bool,
) -> Result<netzinfo::geraete::GeraeteErgebnis, String> {
    netzinfo::geraete::suchen_mit(cidr.as_deref(), gross, true)
}

/// Hops to `ziel` (default 1.1.1.1), see issue #73.
#[tauri::command(async)]
fn pfad_messen(ziel: Option<String>) -> Result<netzinfo::pfad::PfadErgebnis, String> {
    netzinfo::pfad::pfad_daten(ziel.as_deref().unwrap_or("1.1.1.1"), 20, 10, true)
}

/// Captive portal check, see issue #73.
#[tauri::command(async)]
fn portal_pruefen() -> netzinfo::portal::PortalErgebnis {
    netzinfo::portal::check()
}

/// "Wo hakt es?": four ping series, one verdict, see issue #31.
#[tauri::command(async)]
fn diagnose() -> Result<netzinfo::diagnose::DiagnoseErgebnis, String> {
    netzinfo::diagnose::diagnose_daten()
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
fn dns_verschluesselt() -> Result<netzinfo::dnsverschl::DnsErgebnis, String> {
    netzinfo::dnsverschl::dns_daten()
}

#[tauri::command(async)]
fn netz_vergessen(name: String) -> Result<(), String> {
    netze::vergessen(&name)
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
            dns_verschluesselt
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("Die App ließ sich nicht starten: {e}");
        std::process::exit(1);
    }
}
