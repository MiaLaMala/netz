//! netz: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.
//!
//! Library used by the command line tool (src/main.rs) and the app (app/).

pub mod dns;
pub mod geraete;
pub mod hier;
pub mod messen;
pub mod netze;
pub mod ports;
pub mod pruefen;
pub mod subnetz;
pub mod tls;

/// Prints a German explanation line when `--erklaer` is on.
pub fn explain(on: bool, text: &str) {
    if on {
        println!("  ↳ {text}");
    }
}
