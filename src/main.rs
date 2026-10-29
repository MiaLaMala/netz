//! Command line front end of Netzinfo. The logic lives in the library (src/lib.rs).

use netzinfo::{
    bonjour, diagnose, dns, geraete, hier, ipv6, last, live, messen, netze, offen, pfad, portal,
    portfreigaben, ports, proxy, pruefen, router, sni, tls, traffic, upnp, verbindungen, wlan,
    woher,
};

use std::process::ExitCode;

/// Hilfe nach Themen: (Überschrift, [(Aufruf, Kurzbeschreibung)]). Eine Zeile je Variante,
/// Varianten eines Befehls stehen zusammen. Die Befehlsnamen für „Meintest du“ kommen von hier.
const GRUPPEN: &[(&str, &[(&str, &str)])] = &[
    (
        "Dieses Netz",
        &[
            ("hier", "Was ist das für ein Netz? IP, Subnetz, Router, DNS"),
            (
                "subnetz <CIDR>",
                "Subnetz ausrechnen, z. B. für 172.16.0.0/16",
            ),
            (
                "dns <Name>",
                "DNS-Antwort deines Netzes mit 1.1.1.1 und Quad9 vergleichen",
            ),
            (
                "dns --verschluesselt",
                "Verschlüsseltes DNS (DoT, DoH) prüfen",
            ),
            (
                "dns --tempo",
                "Wer antwortet am schnellsten? Dein DNS gegen 1.1.1.1, 8.8.8.8, 9.9.9.9",
            ),
            ("sni", "Filtert das Netz Seiten am Servernamen (SNI)?"),
            (
                "tls [Host]",
                "Wer hat das Zertifikat unterschrieben? Erkennt SSL-Inspection",
            ),
            (
                "portal [--json]",
                "Hängt das Netz hinter einer Anmeldeseite?",
            ),
            (
                "proxy [--ansehen]",
                "Proxy, PAC und WPAD in diesem Netz erkennen",
            ),
            (
                "ipv6 [Dienst]",
                "IPv4 und IPv6 im Vergleich, mit Happy Eyeballs",
            ),
            (
                "zeit [--json]",
                "Geht deine Uhr (oder die des Netzes) richtig? NTP und HTTPS-Date",
            ),
            (
                "router [--json]",
                "Router-Seite passiv einschätzen: http, https, Zertifikat",
            ),
            (
                "portfreigaben",
                "Ports, die der Router per UPnP ins Internet geöffnet hat (nur lesen)",
            ),
        ],
    ),
    (
        "Merken und Prüfen",
        &[
            (
                "merken <Name> [--meins]",
                "Aktuelles Netz merken, optional als „meins“ markieren",
            ),
            ("netze", "Gemerkte Netze auflisten"),
            ("vergessen <Name>", "Ein gemerktes Netz löschen"),
            (
                "pruefen [--pfad]",
                "Was ist anders als beim letzten Besuch?",
            ),
        ],
    ),
    (
        "Qualität",
        &[
            ("messen [--minuten N]", "Antwortzeiten und Aussetzer messen"),
            ("bericht [Netz]", "Qualität eines Netzes über die Zeit"),
            (
                "live <Ziel>...",
                "Ping-Kurve mit Jitter, Verlust und Eignung für Anrufe und Spiele",
            ),
            (
                "last --start",
                "Antwortzeit unter Last (Bufferbloat), belastet die Leitung 30 s",
            ),
            ("pfad [Ziel]", "Weg ins Internet Hop für Hop"),
            ("mtu [Ziel]", "Größte Paketgröße des Weges, mit Erklärung"),
            (
                "diagnose [--json]",
                "Wo liegt das Problem? Schicht für Schicht eingrenzen",
            ),
            (
                "wlan [--json]",
                "Funkverbindung, Signal und Sicherheit des WLANs",
            ),
            (
                "wlan --verlauf [--minuten N]",
                "Signal, Roaming und Abbrüche über Minuten verfolgen",
            ),
            (
                "wlan --umfeld [--json]",
                "Scan: Netze je Kanal und Band, Rat für einen leereren Kanal",
            ),
        ],
    ),
    (
        "Geräte im Netz",
        &[
            (
                "geraete [--nachbarn]",
                "Geräte im eigenen Netz, neu und verschwunden, mit MAC",
            ),
            (
                "geraete --karten",
                "Gerätekarten mit Typ und Hinweisen aus dem Gemerkten",
            ),
            ("ports <Host>", "Offene Ports eines eigenen Geräts"),
            (
                "ports --raus [--ziel <Server>]",
                "Welche ausgehenden Ports lässt dieses Netz durch?",
            ),
            ("bonjour", "Dienste im Netz, die sich per Bonjour melden"),
            (
                "upnp",
                "Geräte, die sich per UPnP/SSDP vorstellen, mit Modell",
            ),
        ],
    ),
    (
        "Dieser Mac",
        &[
            (
                "offen [--json]",
                "Welche Programme auf diesem Rechner Ports offen haben",
            ),
            (
                "offen --aussen [--meins]",
                "Dazu: Port auch von außen erreichbar? (wie ports)",
            ),
            (
                "verbindungen",
                "Aktive Verbindungen je Programm mit Zielname",
            ),
            (
                "woher <Port|Programm> [--json]",
                "Wer hat dieses Programm gestartet? Kette bis launchd",
            ),
            (
                "woher --pid <PID>",
                "Dasselbe für ein Programm ohne offenen Port",
            ),
            (
                "traffic [--sekunden N] [--json]",
                "Datenmenge je Programm ohne Root (nur Mac)",
            ),
        ],
    ),
];

const BEISPIELE: &[(&str, &str)] = &[
    ("hier", "Wo bin ich gerade?"),
    ("merken Zuhause --meins", "Dieses Netz als „meins“ merken"),
    (
        "pruefen",
        "Hat sich seit dem letzten Besuch etwas geändert?",
    ),
    (
        "dns example.org --erklaer",
        "DNS vergleichen, jeder Schritt erklärt",
    ),
];

fn help() -> String {
    let breite = GRUPPEN
        .iter()
        .flat_map(|(_, z)| z.iter())
        .map(|(a, _)| a.chars().count())
        .max()
        .unwrap_or(0);
    let mut s = String::from(
        "Netzinfo: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.\n\n\
         Aufruf: netzinfo <Befehl> [Optionen]\n",
    );
    for (titel, zeilen) in GRUPPEN {
        s += &format!("\n{titel}:\n");
        for (aufruf, text) in *zeilen {
            s += &format!("  {aufruf:<breite$}  {text}\n");
        }
    }
    s += "\nOptionen:\n";
    for (o, t) in [
        (
            "--erklaer",
            "Erklärt bei jedem Schritt, was im Netz passiert",
        ),
        ("-V, --version", "Version anzeigen"),
        ("-h, --help", "Diese Hilfe"),
    ] {
        s += &format!("  {o:<breite$}  {t}\n");
    }
    s += "\nBeispiele:\n";
    for (a, t) in BEISPIELE {
        s += &format!(
            "  netzinfo {a:<w$}  {t}\n",
            w = BEISPIELE
                .iter()
                .map(|(a, _)| a.chars().count())
                .max()
                .unwrap_or(0)
        );
    }
    s
}

/// Alle Befehlsnamen aus der Hilfe, ohne Doppelte.
fn befehle() -> Vec<&'static str> {
    let mut v: Vec<&str> = GRUPPEN
        .iter()
        .flat_map(|(_, z)| z.iter())
        .filter_map(|(a, _)| a.split_whitespace().next())
        .collect();
    v.dedup();
    v.sort_unstable();
    v.dedup();
    v
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let alt = row[j + 1];
            row[j + 1] = (diag + usize::from(ca != *cb)).min(row[j] + 1).min(alt + 1);
            diag = alt;
        }
    }
    row[b.len()]
}

/// Nächster Befehl, wenn der Abstand klein ist (höchstens 2 und kleiner als die Eingabelänge).
fn aehnlich(eingabe: &str) -> Option<&'static str> {
    let n = eingabe.chars().count();
    befehle()
        .into_iter()
        .map(|b| (levenshtein(&eingabe.to_lowercase(), b), b))
        .filter(|(d, _)| *d <= 2 && *d < n)
        .min_by_key(|(d, _)| *d)
        .map(|(_, b)| b)
}

/// Bedienfehler (Exit 2) erkennt man an der Meldung: die Module liefern nur Text.
// ponytail: Textmarker statt Fehlerart; sauberer wäre ein Fehlertyp mit Art in jedem Modul.
fn ist_bedienfehler(msg: &str) -> bool {
    [
        "Unbekannte",
        "fehlt",
        "Aufruf:",
        "Es passt genau",
        "Nur ein Ziel",
        "ist leer",
        "braucht keine",
    ]
    .iter()
    .any(|m| msg.contains(m))
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let explain_on = take_flag(&mut args, "--erklaer");
    let Some((command, rest)) = args.split_first() else {
        print!("{}", help());
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
        "portfreigaben" => portfreigaben::run(rest, explain_on),
        "last" => last::run(rest, explain_on),
        "hilfe" | "-h" | "--help" => {
            print!("{}", help());
            Ok(())
        }
        "-V" | "--version" => {
            println!("netzinfo {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => {
            let tipp = match aehnlich(other) {
                Some(b) => format!("Meintest du `netzinfo {b}`?"),
                None => "`netzinfo --help` zeigt alle Befehle.".to_string(),
            };
            eprintln!("Unbekannter Befehl „{other}“. {tipp}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::from(if ist_bedienfehler(&e) { 2 } else { 1 })
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
    use super::*;

    #[test]
    fn take_flag_removes_every_occurrence() {
        let mut args = vec!["hier".into(), "--erklaer".into(), "--erklaer".into()];
        assert!(take_flag(&mut args, "--erklaer"));
        assert_eq!(args, vec!["hier".to_string()]);
        assert!(!take_flag(&mut args, "--erklaer"));
    }

    #[test]
    fn tippfehler_findet_naechsten_befehl() {
        assert_eq!(aehnlich("pruefn"), Some("pruefen"));
        assert_eq!(aehnlich("HIER"), Some("hier"));
        assert_eq!(aehnlich("wlann"), Some("wlan"));
        assert_eq!(aehnlich("xyzzy"), None);
        assert_eq!(aehnlich("h"), None);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn jeder_befehl_aus_main_steht_in_der_hilfe() {
        let text = help();
        let quelle = include_str!("main.rs");
        let dispatch = quelle.split("let result = match").nth(1).unwrap_or("");
        let dispatch = dispatch.split("other =>").next().unwrap_or("");
        let mut gefunden = 0;
        for zeile in dispatch.lines() {
            let z = zeile.trim_start();
            let Some(rest) = z.strip_prefix('"') else {
                continue;
            };
            let Some((name, tail)) = rest.split_once('"') else {
                continue;
            };
            if !tail.trim_start().starts_with("=>") || name.starts_with('-') || name == "hilfe" {
                continue;
            }
            gefunden += 1;
            assert!(
                text.contains(&format!("  {name}")) && befehle().contains(&name),
                "{name} fehlt in der Hilfe"
            );
        }
        assert!(gefunden > 30, "Dispatch nicht gelesen: {gefunden}");
        // und umgekehrt: kein Befehl in der Hilfe ohne Dispatch
        for b in befehle() {
            assert!(
                dispatch.contains(&format!("\"{b}\" =>")),
                "{b} ohne Dispatch"
            );
        }
    }

    #[test]
    fn bedienfehler_werden_erkannt() {
        assert!(ist_bedienfehler("Es fehlt das Ziel."));
        assert!(ist_bedienfehler("Unbekannte Option „--x“."));
        assert!(!ist_bedienfehler("Keine Verbindung zum Router."));
    }
}
