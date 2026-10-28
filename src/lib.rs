//! Netzinfo: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.
//!
//! Library used by the command line tool (src/main.rs) and the app (app/).

pub mod bonjour;
pub mod diagnose;
pub mod dns;
pub mod dnsverschl;
pub mod geraete;
pub mod hersteller;
pub mod hier;
pub mod messen;
pub mod nachbarn;
pub mod netze;
pub mod offen;
pub mod pfad;
pub mod portal;
pub mod ports;
pub mod pruefen;
pub mod subnetz;
pub mod tls;
pub mod verbindungen;
pub mod wlan;

/// Prints a German explanation line when `--erklaer` is on.
pub fn explain(on: bool, text: &str) {
    if on {
        println!("  ↳ {text}");
    }
}
