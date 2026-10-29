//! Netzinfo: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.
//!
//! Library used by the command line tool (src/main.rs) and the app (app/).

pub mod ausfall;
pub mod bonjour;
pub mod diagnose;
pub mod dns;
pub mod dnstempo;
pub mod dnsverschl;
pub mod egress;
pub mod gatewaywache;
pub mod geraete;
pub mod geraetekarte;
pub mod hersteller;
pub mod hier;
pub mod ipv6;
pub mod ipv6router;
pub mod kanal;
pub mod last;
pub mod live;
pub mod messen;
pub mod mtu;
pub mod nachbarn;
pub mod namen;
pub mod netze;
pub mod offen;
pub mod optionen;
pub mod ort;
pub mod pfad;
pub mod pfadspur;
pub mod portal;
pub mod portfreigaben;
pub mod ports;
pub mod proxy;
pub mod pruefen;
pub mod router;
pub mod sni;
pub mod statistik;
pub mod subnetz;
pub mod tls;
pub mod traffic;
pub mod upnp;
pub mod verbindungen;
pub mod wlan;
pub mod wlanverlauf;
pub mod woher;
pub mod zeit;

/// Prints a German explanation line when `--erklaer` is on.
pub fn explain(on: bool, text: &str) {
    if on {
        println!("  ↳ {text}");
    }
}
