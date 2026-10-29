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
pub mod fritzbox;
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
pub mod plattform;
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

/// Fehler des Betriebssystems auf Deutsch: `{e}` käme englisch („No route to host (os error 65)“).
/// Unbekannte Fälle mit Code werden zu „Systemfehler N“, ohne Code bleibt der Originaltext.
pub fn io_satz(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    let satz = match e.kind() {
        K::NotFound => "nicht gefunden",
        K::PermissionDenied => "keine Berechtigung",
        K::ConnectionRefused => "Verbindung abgelehnt",
        K::ConnectionReset => "Verbindung zurückgesetzt",
        K::ConnectionAborted => "Verbindung abgebrochen",
        K::TimedOut | K::WouldBlock => "keine Antwort in der Zeit",
        K::HostUnreachable | K::NetworkUnreachable => "Ziel nicht erreichbar",
        K::AddrInUse => "Adresse schon in Benutzung",
        K::AddrNotAvailable => "Adresse nicht verfügbar",
        K::AlreadyExists => "gibt es schon",
        K::UnexpectedEof => "Verbindung vorzeitig beendet",
        K::BrokenPipe => "Verbindung unterbrochen",
        _ => {
            return e
                .raw_os_error()
                .map_or_else(|| e.to_string(), |n| format!("Systemfehler {n}"));
        }
    };
    satz.into()
}

#[cfg(test)]
mod io_satz_tests {
    use super::io_satz;
    use std::io::{Error, ErrorKind as K};

    #[test]
    fn systemfehler_kommen_auf_deutsch() {
        for (k, s) in [
            (K::NotFound, "nicht gefunden"),
            (K::PermissionDenied, "keine Berechtigung"),
            (K::ConnectionRefused, "Verbindung abgelehnt"),
            (K::ConnectionReset, "Verbindung zurückgesetzt"),
            (K::ConnectionAborted, "Verbindung abgebrochen"),
            (K::TimedOut, "keine Antwort in der Zeit"),
            (K::WouldBlock, "keine Antwort in der Zeit"),
            (K::HostUnreachable, "Ziel nicht erreichbar"),
            (K::NetworkUnreachable, "Ziel nicht erreichbar"),
            (K::AddrInUse, "Adresse schon in Benutzung"),
            (K::AddrNotAvailable, "Adresse nicht verfügbar"),
            (K::AlreadyExists, "gibt es schon"),
            (K::UnexpectedEof, "Verbindung vorzeitig beendet"),
            (K::BrokenPipe, "Verbindung unterbrochen"),
        ] {
            assert_eq!(io_satz(&Error::from(k)), s);
        }
        // englisch „No route to host“, Nummer je System verschieden (Mac 65, Linux 113)
        assert_eq!(
            io_satz(&Error::from_raw_os_error(libc::EHOSTUNREACH)),
            "Ziel nicht erreichbar"
        );
        // Code ohne eigene Art: Nummer statt englischem Text
        let e = Error::from_raw_os_error(i32::MAX);
        assert_eq!(io_satz(&e), format!("Systemfehler {}", i32::MAX));
        // ohne Code bleibt der Originaltext
        assert_eq!(io_satz(&Error::other("kaputt")), "kaputt");
    }
}
