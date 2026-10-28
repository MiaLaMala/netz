//! The Mac app: a thin Tauri shell around the `netz` library.
//!
//! Every command returns data or a German sentence; the UI (app/ui) shows the sentence where it
//! happened.

use netz::netze::{self, Aktuell, Netz};

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
fn netz_vergessen(name: String) -> Result<(), String> {
    netze::vergessen(&name)
}

fn main() {
    let r = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            dieses_netz,
            netz_merken,
            gemerkte_netze,
            netz_vergessen
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("Die App ließ sich nicht starten: {e}");
        std::process::exit(1);
    }
}
