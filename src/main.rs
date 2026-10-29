//! Command line front end of Netzinfo. The logic lives in the library (src/lib.rs).

use netzinfo::{
    bonjour, diagnose, dns, geraete, hier, ipv6, live, messen, netze, offen, pfad, portal, ports,
    proxy, pruefen, router, sni, tls, traffic, upnp, verbindungen, wlan, woher,
};

use std::process::ExitCode;

const HELP: &str = "\
Netzinfo: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.

Befehle:
  netzinfo hier              Was ist das für ein Netz? IP, Subnetz, Router, DNS
  netzinfo subnetz <CIDR>    Subnetz ausrechnen, z. B. für 172.16.0.0/16
  netzinfo dns <Name>        DNS-Antwort deines Netzes mit 1.1.1.1 und Quad9 vergleichen
  netzinfo dns --verschluesselt
                             Verschlüsseltes DNS (DoT, DoH) prüfen
  netzinfo sni               Filtert das Netz Seiten am Servernamen (SNI)?
  netzinfo dns --tempo       Wer antwortet am schnellsten? Dein DNS gegen 1.1.1.1, 8.8.8.8, 9.9.9.9
  netzinfo tls [Host]        Wer hat das Zertifikat unterschrieben? Erkennt SSL-Inspection
  netzinfo ports <Host>      Offene Ports eines eigenen Geräts
  netzinfo merken <Name> [--meins]
                             Aktuelles Netz merken, optional als „meins“ markieren
  netzinfo netze             Gemerkte Netze auflisten
  netzinfo vergessen <Name>  Ein gemerktes Netz löschen
  netzinfo pruefen           Was ist anders als beim letzten Besuch?
  netzinfo messen [--minuten N]
                             Antwortzeiten und Aussetzer messen
  netzinfo bericht [Netz]    Qualität eines Netzes über die Zeit
  netzinfo geraete [--nachbarn]
                             Geräte im eigenen Netz, neu und verschwunden, mit MAC
  netzinfo geraete --karten  Gerätekarten mit Typ und Hinweisen aus dem Gemerkten
  netzinfo pfad [Ziel]       Weg ins Internet Hop für Hop
  netzinfo mtu [Ziel]        Größte Paketgröße des Weges, mit Erklärung
  netzinfo portal [--json]   Hängt das Netz hinter einer Anmeldeseite?
  netzinfo offen [--json]    Welche Programme auf diesem Rechner Ports offen haben
  netzinfo offen --aussen [--meins]  Dazu: Port auch von außen erreichbar? (wie ports)
  netzinfo bonjour           Dienste im Netz, die sich per Bonjour melden
  netzinfo diagnose [--json] Wo liegt das Problem? Schicht für Schicht eingrenzen
  netzinfo wlan [--json]     Funkverbindung, Signal und Sicherheit des WLANs
  netzinfo wlan --verlauf [--minuten N]
                             Signal, Roaming und Abbrüche über Minuten verfolgen
  netzinfo wlan --umfeld [--json]
                             Scan: Netze je Kanal und Band, Rat für einen leereren Kanal
  netzinfo proxy [--ansehen] Proxy, PAC und WPAD in diesem Netz erkennen
  netzinfo verbindungen      Aktive Verbindungen je Programm mit Zielname
  netzinfo woher <Port|Programm> [--json]
                             Wer hat dieses Programm gestartet? Kette bis launchd
  netzinfo woher --pid <PID> Dasselbe für ein Programm ohne offenen Port
  netzinfo live <Ziel>...    Ping-Kurve mit Jitter, Verlust und Eignung für Anrufe und Spiele
  netzinfo ipv6 [Dienst]     IPv4 und IPv6 im Vergleich, mit Happy Eyeballs
  netzinfo zeit [--json]     Geht deine Uhr (oder die des Netzes) richtig? NTP und HTTPS-Date
  netzinfo traffic [--sekunden N] [--json]
                             Datenmenge je Programm ohne Root (nur Mac)
  netzinfo upnp              Geräte, die sich per UPnP/SSDP vorstellen, mit Modell
  netzinfo router [--json]   Router-Seite passiv einschätzen: http, https, Zertifikat

Optionen:
  --erklaer                  Erklärt bei jedem Schritt, was im Netz passiert
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let explain_on = take_flag(&mut args, "--erklaer");
    let Some((command, rest)) = args.split_first() else {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    };
    let result = match command.as_str() {
        "hier" => hier::run(rest, explain_on),
        "subnetz" => hier::run_subnet(rest, explain_on),
        "dns" => dns::run(rest, explain_on),
        "tls" => tls::run(rest, explain_on),
        "ports" => ports::run(rest, explain_on),
        "merken" => netze::run_merken(rest, explain_on),
        "netze" => netze::run_netze(rest, explain_on),
        "vergessen" => netze::run_vergessen(rest, explain_on),
        "pruefen" => pruefen::run(rest, explain_on),
        "messen" => messen::run_messen(rest, explain_on),
        "bericht" => messen::run_bericht(rest, explain_on),
        "geraete" => geraete::run(rest, explain_on),
        "pfad" => pfad::run(rest, explain_on),
        "mtu" => netzinfo::mtu::run(rest, explain_on),
        "portal" => portal::run(rest, explain_on),
        "offen" => offen::run(rest, explain_on),
        "sni" => sni::run(rest, explain_on),
        "bonjour" => bonjour::run(rest, explain_on),
        "diagnose" => diagnose::run(rest, explain_on),
        "wlan" => wlan::run(rest, explain_on),
        "proxy" => proxy::run(rest, explain_on),
        "verbindungen" => verbindungen::run(rest, explain_on),
        "woher" => woher::run(rest, explain_on),
        "live" => live::run(rest, explain_on),
        "ipv6" => ipv6::run(rest, explain_on),
        "zeit" => netzinfo::zeit::run(rest, explain_on),
        "traffic" => traffic::run(rest, explain_on),
        "upnp" => upnp::run(rest, explain_on),
        "router" => router::run(rest, explain_on),
        "hilfe" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!(
            "Unbekannter Befehl „{other}“. `netzinfo hilfe` zeigt alle Befehle."
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Removes `flag` from `args` and reports whether it was there.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

#[cfg(test)]
mod tests {
    use super::take_flag;

    #[test]
    fn take_flag_removes_every_occurrence() {
        let mut args = vec!["hier".into(), "--erklaer".into(), "--erklaer".into()];
        assert!(take_flag(&mut args, "--erklaer"));
        assert_eq!(args, vec!["hier".to_string()]);
        assert!(!take_flag(&mut args, "--erklaer"));
    }
}
