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
    netzinfo::geraete::suchen(cidr.as_deref(), gross)
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
            geraete_suchen
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("Die App ließ sich nicht starten: {e}");
        std::process::exit(1);
    }
}
