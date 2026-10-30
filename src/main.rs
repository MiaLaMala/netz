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
                "dns <Name> [--typ A|AAAA|MX|TXT]",
                "DNS-Antwort deines Netzes mit 1.1.1.1 und Quad9 vergleichen",
            ),
            ("dns --meine-ip", "Öffentliche IP per DNS-Abfrage ermitteln"),
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
                "Proxy, PAC und WPAD in diesem Netz erkennen (nur Mac)",
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
            (
                "bericht [Netz] [--markdown <Datei>]",
                "Qualität eines Netzes über die Zeit, auch als Datei für den IT-Support",
            ),
            (
                "live <Ziel>... [--sekunden N]",
                "Ping-Kurve mit Jitter, Verlust und Eignung für Anrufe und Spiele",
            ),
            (
                "engpass [--start]",
                "Leitung, WLAN und Mac nebeneinander: wo bremst es? (--start misst am Mac)",
            ),
            (
                "last --start",
                "Antwortzeit unter Last (Bufferbloat), 30 s volle Leitung (nur Mac)",
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
                "Scan: Netze je Kanal und Band, Rat für einen leereren Kanal (nur Mac)",
            ),
        ],
    ),
    (
        "Geräte im Netz",
        &[
            (
                "geraete [--nachbarn]",
                "Geräte im eigenen Netz, neu und verschwunden, mit MAC (--nachbarn nur Mac)",
            ),
            (
                "geraete --karten",
                "Gerätekarten mit Typ und Hinweisen aus dem Gemerkten",
            ),
            (
                "geraete <Bereich> --gross",
                "Bereich größer als /22 absuchen (bis /16), dauert lange",
            ),
            (
                "fritzbox [--zugang <Benutzer>|--entfernen]",
                "Geräteliste der Fritz!Box (TR-064), Zugang im Schlüsselbund (nur Mac)",
            ),
            ("ports <Host>", "Offene Ports eines eigenen Geräts"),
            (
                "ports --raus [--ziel <Server>]",
                "Welche ausgehenden Ports lässt dieses Netz durch?",
            ),
            (
                "bonjour [--txt]",
                "Dienste im Netz, die sich per Bonjour melden",
            ),
            (
                "upnp",
                "Geräte, die sich per UPnP/SSDP vorstellen, mit Modell",
            ),
        ],
    ),
    (
        "Dieser Mac (unter Linux nicht verfügbar)",
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
                "verbindungen [--programm Name]",
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
                "Datenmenge je Programm ohne Root",
            ),
        ],
    ),
];

/// Hilfe bricht bei 80 Zeichen um: das ist die Standardbreite des Terminals auf dem Mac.
const BREITE: usize = 80;

/// Linke Spalte der Gesamthilfe höchstens so breit; längere Aufrufe stehen über ihrem Text.
const SPALTE_MAX: usize = 30;

/// Die ersten Schritte, ganz oben in der Hilfe.
const BEISPIELE: &[(&str, &str)] = &[
    ("hier", "Wo bin ich gerade?"),
    ("merken Zuhause --meins", "Dieses Netz als „meins“ merken"),
    ("pruefen", "Was ist seit dem letzten Besuch anders?"),
    (
        "dns example.org --erklaer",
        "DNS vergleichen, jeder Schritt erklärt",
    ),
];

/// Je Befehl: (Name, Beispielaufruf, was er ins Netz sendet). Steht in `netzinfo <Befehl> --help`.
const DETAILS: &[(&str, &str, &str)] = &[
    (
        "hier",
        "hier",
        "Liest nur Einstellungen dieses Rechners, sendet nichts.",
    ),
    (
        "subnetz",
        "subnetz 192.0.2.77/26 --erklaer",
        "Rechnet nur, sendet nichts.",
    ),
    (
        "dns",
        "dns example.org --typ MX",
        "Fragt DNS-Server: deinen, 1.1.1.1 und Quad9 (--tempo auch 8.8.8.8, 9.9.9.9).",
    ),
    (
        "sni",
        "sni",
        "Verbindet sich mit einem Testserver, erst mit neutralem Namen, dann mit Beispielnamen.",
    ),
    (
        "tls",
        "tls github.com",
        "Baut eine HTTPS-Verbindung zum Host auf, ohne Anmeldung.",
    ),
    (
        "portal",
        "portal",
        "Ruft drei Prüfadressen von Apple, Google und Mozilla per HTTP ab.",
    ),
    (
        "proxy",
        "proxy",
        "Liest nur Einstellungen, sendet nichts. Die PAC-Datei lädt nur --ansehen.",
    ),
    (
        "ipv6",
        "ipv6",
        "Löst Namen auf und verbindet sich fünfmal je Protokoll mit Port 443, ohne Daten.",
    ),
    (
        "zeit",
        "zeit",
        "Fragt öffentliche Zeitserver (NTP) und liest das Datum einer HTTPS-Antwort. Stellt die Uhr nie.",
    ),
    (
        "router",
        "router",
        "Eine HTTP-Anfrage und ein TLS-Handshake an den Router, ohne Anmeldung. Nur in „meins“-Netzen.",
    ),
    (
        "portfreigaben",
        "portfreigaben",
        "Fragt den Router per UPnP, nur lesend.",
    ),
    (
        "merken",
        "merken Zuhause --meins",
        "Sendet nichts, speichert nur auf diesem Rechner.",
    ),
    (
        "netze",
        "netze",
        "Sendet nichts, liest nur die gemerkten Netze.",
    ),
    (
        "vergessen",
        "vergessen Zuhause",
        "Sendet nichts, löscht nur den Eintrag auf diesem Rechner.",
    ),
    (
        "pruefen",
        "pruefen",
        "Misst das Netz neu (DNS, Zertifikate, Router, Weg ins Internet) und vergleicht mit dem Gemerkten.",
    ),
    (
        "messen",
        "messen --minuten 5",
        "Fragt DNS beim Router und bei 1.1.1.1, baut TCP-Verbindungen zu 1.1.1.1 auf und speichert die Zeiten je Netz.",
    ),
    (
        "bericht",
        "bericht Zuhause --markdown bericht.md",
        "Sendet nichts, liest nur gespeicherte Messungen.",
    ),
    (
        "live",
        "live 1.1.1.1 --sekunden 30",
        "Schickt Pings (ICMP) an die Ziele, die du angibst.",
    ),
    (
        "engpass",
        "engpass",
        "Fragt den Router lesend (TR-064 oder UPnP). --start belastet die Leitung etwa 30 Sekunden.",
    ),
    (
        "last",
        "last --start",
        "Belastet die Leitung 30 Sekunden voll (Apples networkQuality), nur mit --start.",
    ),
    (
        "pfad",
        "pfad example.org",
        "Schickt Pings mit steigender Lebensdauer (TTL) an das Ziel.",
    ),
    (
        "mtu",
        "mtu example.org",
        "Schickt Pings wachsender Größe an das Ziel.",
    ),
    (
        "diagnose",
        "diagnose",
        "Schickt Pings an Router, Anbieter und zwei Ziele im Internet.",
    ),
    (
        "wlan",
        "wlan --verlauf --minuten 5",
        "Liest nur das Funkmodul dieses Rechners (--umfeld scannt per Funk), sendet nichts ins Netz.",
    ),
    (
        "geraete",
        "geraete --karten",
        "Probiert an jeder Adresse 12 TCP-Ports (mit --nachbarn auch einen Ping) und fragt Bonjour, UPnP und DNS. Nur private Adressen, nur in „meins“-Netzen.",
    ),
    (
        "fritzbox",
        "fritzbox",
        "Fragt die Fritz!Box lesend (TR-064). Nur der Router, nur in „meins“-Netzen.",
    ),
    (
        "ports",
        "ports 192.0.2.20",
        "Verbindet sich mit Ports des Geräts, nur eigene Geräte. --raus geht nur zu deinem --ziel.",
    ),
    (
        "bonjour",
        "bonjour --txt",
        "Fragt per Multicast (mDNS) im eigenen Netz.",
    ),
    (
        "upnp",
        "upnp",
        "Fragt per Multicast (SSDP) im eigenen Netz und lädt die Beschreibung vom Gerät selbst.",
    ),
    (
        "offen",
        "offen --json",
        "Liest nur diesen Rechner, sendet nichts. --aussen probiert die Ports an der Netzadresse dieses Macs.",
    ),
    (
        "verbindungen",
        "verbindungen --programm Safari",
        "Liest nur diesen Rechner, sendet nichts.",
    ),
    (
        "woher",
        "woher 8080",
        "Liest nur diesen Rechner, sendet nichts.",
    ),
    (
        "traffic",
        "traffic --sekunden 10",
        "Liest nur Zähler dieses Rechners, sendet nichts.",
    ),
];

/// Bricht `text` bei `BREITE` Zeichen um (zählt Zeichen, nicht Bytes). Ohne Zeilenumbruch am Ende.
fn umbrechen(text: &str, erste: &str, folge: &str) -> String {
    let (mut aus, mut zeile, mut leer) = (String::new(), erste.to_string(), true);
    for wort in text.split_whitespace() {
        if !leer && zeile.chars().count() + 1 + wort.chars().count() > BREITE {
            aus += &zeile;
            aus.push('\n');
            (zeile, leer) = (folge.to_string(), true);
        }
        if !leer {
            zeile.push(' ');
        }
        zeile += wort;
        leer = false;
    }
    aus + &zeile
}

fn help() -> String {
    let breite = GRUPPEN
        .iter()
        .flat_map(|(_, z)| z.iter())
        .map(|(a, _)| a.chars().count())
        .max()
        .unwrap_or(0)
        .min(SPALTE_MAX);
    let mut s = String::from(
        "Netzinfo: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.\n\n\
         Aufruf: netzinfo <Befehl> [Optionen]\n\nFang mit `netzinfo hier` an:\n",
    );
    let w = BEISPIELE
        .iter()
        .map(|(a, _)| a.chars().count())
        .max()
        .unwrap_or(0);
    for (a, t) in BEISPIELE {
        s += &spalten("  netzinfo ", a, w, t);
    }
    for (titel, zeilen) in GRUPPEN {
        s += &format!("\n{titel}:\n");
        for (aufruf, text) in *zeilen {
            s += &spalten("  ", aufruf, breite, text);
        }
    }
    s += "\nOptionen:\n";
    for (o, t) in [
        (
            "--erklaer",
            "Erklärt bei jedem Schritt, was im Netz passiert",
        ),
        ("-V, --version", "Version anzeigen"),
        (
            "-h, --help",
            "Diese Hilfe; `netzinfo <Befehl> --help` zeigt nur den Befehl",
        ),
    ] {
        s += &spalten("  ", o, breite, t);
    }
    s += "\nExit-Code:\n";
    for (c, t) in [
        ("0", "geklappt"),
        ("1", "Fehler beim Ausführen"),
        ("2", "falscher Aufruf"),
    ] {
        s += &spalten("  ", c, 1, t);
    }
    s
}

/// Eine Tabellenzeile `vor`, linke Spalte auf `breite` Zeichen, rechte Spalte umgebrochen und
/// unter sich eingerückt. Ist `links` breiter, steht der Text eingerückt darunter. Endet mit
/// Zeilenumbruch.
fn spalten(vor: &str, links: &str, breite: usize, text: &str) -> String {
    if links.chars().count() > breite {
        return gestapelt(vor, links, text);
    }
    let erste = format!("{vor}{links:<breite$}  ");
    let folge = " ".repeat(erste.chars().count());
    umbrechen(text, &erste, &folge) + "\n"
}

/// `links` allein in einer Zeile, `text` darunter, vier Zeichen weiter eingerückt.
fn gestapelt(vor: &str, links: &str, text: &str) -> String {
    format!("{vor}{links}\n{}\n", umbrechen(text, "      ", "      "))
}

/// Hilfe für einen Befehl: seine Zeilen aus `GRUPPEN`, dazu Beispiel und Netzwirkung, sonst `None`.
fn befehlshilfe(name: &str) -> Option<String> {
    let zeilen: Vec<_> = GRUPPEN
        .iter()
        .flat_map(|(_, z)| z.iter())
        .filter(|(a, _)| a.split_whitespace().next() == Some(name))
        .collect();
    if zeilen.is_empty() {
        return None;
    }
    let mut s = String::from("Aufruf:\n");
    for (a, t) in zeilen {
        s += &gestapelt("  netzinfo ", a, t);
    }
    if let Some((_, beispiel, netz)) = DETAILS.iter().find(|(b, _, _)| *b == name) {
        s += &format!("\nBeispiel:\n  netzinfo {beispiel}\n\nIm Netz:\n");
        s += &umbrechen(netz, "  ", "  ");
        s += "\n";
    }
    s += "\n";
    s += &umbrechen(
        "`--erklaer` erklärt bei jedem Schritt, was im Netz passiert. `netzinfo --help` zeigt alle Befehle.",
        "",
        "",
    );
    s += "\n";
    Some(s)
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

/// Nächster Befehl, wenn der Abstand klein ist (höchstens 2 und kleiner als die Eingabelänge).
fn aehnlich(eingabe: &str) -> Option<&'static str> {
    let n = eingabe.chars().count();
    befehle()
        .into_iter()
        .map(|b| {
            (
                netzinfo::optionen::levenshtein(&eingabe.to_lowercase(), b),
                b,
            )
        })
        .filter(|(d, _)| *d <= 2 && *d < n)
        .min_by_key(|(d, _)| *d)
        .map(|(_, b)| b)
}

/// Bedienfehler (Exit 2) erkennt man an der Meldung: die Module liefern nur Text.
// ponytail: Textmarker statt Fehlerart; sauberer wäre ein Fehlertyp mit Art in jedem Modul.
fn ist_bedienfehler(msg: &str) -> bool {
    // „fehlt“ nur als fehlende Angabe („Es fehlt das Ziel“, „Nach `--x` fehlt der Name“), nicht
    // als fehlendes Programm oder fehlende Datei: das ist ein Fehler beim Ausführen (Exit 1).
    if msg.starts_with("Nach ") && msg.contains(" fehlt ") {
        return true;
    }
    [
        "Unbekannte",
        "Es fehlt",
        "Aufruf:",
        "Es passt genau",
        "gehört eine ganze Zahl",
        "Gib ein bis vier",
        "Nur ein Ziel",
        "ist leer",
        "braucht keine",
    ]
    .iter()
    .any(|m| msg.contains(m))
}

/// `netzinfo <befehl> | head -1` soll leise enden wie jedes Unix-Werkzeug. Rust stellt SIGPIPE
/// auf „ignorieren“, dann bricht `println!` an der geschlossenen Pipe mit einer Panik ab.
fn sigpipe_wie_unix() {
    #[cfg(unix)]
    // SAFETY: setzt nur die Standardreaktion des Signals, vor dem ersten Thread.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> ExitCode {
    sigpipe_wie_unix();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let explain_on = take_flag(&mut args, "--erklaer");
    let Some((command, rest)) = args.split_first() else {
        print!("{}", help());
        return ExitCode::SUCCESS;
    };
    if rest.iter().any(|a| a == "--help" || a == "-h")
        && let Some(h) = befehlshilfe(command)
    {
        print!("{h}");
        return ExitCode::SUCCESS;
    }
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
        "fritzbox" => netzinfo::fritzbox::run(rest, explain_on),
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
        "engpass" => netzinfo::engpass::run(rest, explain_on),
        "hilfe" | "-h" | "--help" => {
            print!(
                "{}",
                rest.first()
                    .and_then(|b| befehlshilfe(b))
                    .unwrap_or_else(help)
            );
            Ok(())
        }
        "-V" | "--version" => {
            println!("netzinfo {} ({})", env!("CARGO_PKG_VERSION"), ziel());
            Ok(())
        }
        other if other.starts_with('-') => {
            let tipp =
                match netzinfo::optionen::aehnlich(other, &["--erklaer", "--version", "--help"]) {
                    Some(o) => format!("Meintest du `{o}`? "),
                    None => String::new(),
                };
            eprintln!(
                "Unbekannte Option „{other}“. {tipp}`netzinfo --help` zeigt alle Befehle und Optionen."
            );
            return ExitCode::from(2);
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
            let bedienfehler = ist_bedienfehler(&e);
            if bedienfehler && befehlshilfe(command).is_some() {
                eprintln!("Mehr dazu: `netzinfo {command} --help`");
            }
            ExitCode::from(if bedienfehler { 2 } else { 1 })
        }
    }
}

/// Das Ziel dieses Baus, so wie die Release-Dateien heißen (`aarch64-apple-darwin`).
fn ziel() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin".to_string(),
        "linux" if cfg!(target_env = "musl") => "unknown-linux-musl".to_string(),
        "linux" => "unknown-linux-gnu".to_string(),
        andere => andere.to_string(),
    };
    format!("{}-{os}", std::env::consts::ARCH)
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
        assert_eq!(netzinfo::optionen::levenshtein("kitten", "sitting"), 3);
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

    /// Erlaubte Optionen je Befehl (Varianten wie `wlan --verlauf` haben eigene Listen).
    fn optionen_von(befehl: &str) -> Vec<&'static [&'static str]> {
        match befehl {
            "dns" => vec![dns::OPTIONEN],
            "ports" => vec![ports::OPTIONEN],
            "merken" => vec![netze::OPTIONEN_MERKEN],
            "pruefen" => vec![pruefen::OPTIONEN],
            "messen" => vec![messen::OPTIONEN],
            "bericht" => vec![messen::OPTIONEN_BERICHT],
            "last" => vec![last::OPTIONEN],
            "engpass" => vec![netzinfo::engpass::OPTIONEN],
            "geraete" => vec![geraete::OPTIONEN],
            "fritzbox" => vec![netzinfo::fritzbox::OPTIONEN],
            "pfad" => vec![pfad::OPTIONEN],
            "portal" => vec![portal::OPTIONEN],
            "offen" => vec![offen::OPTIONEN],
            "bonjour" => vec![bonjour::OPTIONEN],
            "diagnose" => vec![diagnose::OPTIONEN],
            "wlan" => vec![
                wlan::OPTIONEN,
                netzinfo::wlanverlauf::OPTIONEN,
                netzinfo::kanal::OPTIONEN,
            ],
            "proxy" => vec![proxy::OPTIONEN],
            "verbindungen" => vec![verbindungen::OPTIONEN],
            "woher" => vec![woher::OPTIONEN],
            "live" => vec![live::OPTIONEN],
            "ipv6" => vec![ipv6::OPTIONEN],
            "zeit" => vec![netzinfo::zeit::OPTIONEN],
            "traffic" => vec![traffic::OPTIONEN],
            "router" => vec![router::OPTIONEN],
            _ => vec![],
        }
    }

    #[test]
    fn optionen_aus_der_hilfe_werden_nicht_abgelehnt() {
        let mut geprueft = 0;
        for (aufruf, _) in GRUPPEN.iter().flat_map(|(_, z)| z.iter()) {
            let befehl = aufruf.split_whitespace().next().unwrap_or("");
            let listen = optionen_von(befehl);
            for opt in aufruf
                .split(|c: char| !(c == '-' || c.is_alphanumeric()))
                .filter(|w| w.starts_with("--"))
            {
                geprueft += 1;
                let args = vec![opt.to_string()];
                assert!(
                    listen
                        .iter()
                        .any(|l| netzinfo::optionen::nur(&args, l, befehl).is_ok()),
                    "{befehl} lehnt {opt} ab, steht aber in der Hilfe"
                );
            }
        }
        assert!(geprueft > 20, "Hilfe nicht gelesen: {geprueft}");
    }

    #[test]
    fn hilfe_bleibt_unter_100_zeichen() {
        let mut alle = vec![help()];
        alle.extend(befehle().into_iter().filter_map(befehlshilfe));
        for z in alle.iter().flat_map(|h| h.lines()) {
            assert!(z.chars().count() <= netzinfo::ZEILE_MAX, "{z}");
        }
    }

    #[test]
    fn jeder_befehl_hat_beispiel_und_netzangabe() {
        for b in befehle() {
            let d = DETAILS.iter().find(|(n, _, _)| *n == b);
            let (_, beispiel, netz) = d.unwrap_or_else(|| panic!("{b} ohne DETAILS"));
            assert_eq!(beispiel.split_whitespace().next(), Some(b), "{beispiel}");
            assert!(!netz.is_empty());
        }
        assert_eq!(DETAILS.len(), befehle().len(), "DETAILS mit totem Befehl");
    }

    #[test]
    fn hilfe_passt_in_80_spalten() {
        let mut alle = vec![help()];
        alle.extend(befehle().into_iter().filter_map(befehlshilfe));
        for z in alle.iter().flat_map(|h| h.lines()) {
            assert!(z.chars().count() <= BREITE, "{z}");
        }
        let h = help();
        let einstieg = h.find("Fang mit `netzinfo hier` an").unwrap();
        assert!(einstieg < h.find("Dieses Netz:").unwrap());
    }

    #[test]
    fn ziel_nennt_architektur_und_system() {
        let z = ziel();
        assert!(
            z.starts_with(std::env::consts::ARCH) && z.matches('-').count() >= 2,
            "{z}"
        );
    }

    #[test]
    fn hilfe_je_befehl() {
        let h = befehlshilfe("wlan").unwrap();
        assert!(
            h.contains("wlan --umfeld") && !h.contains("netzinfo dns"),
            "{h}"
        );
        assert!(befehlshilfe("xyzzy").is_none());
    }

    #[test]
    fn unbekannte_option_ist_bedienfehler() {
        let e = netzinfo::optionen::nur(&["--x".to_string()], &[], "hier").unwrap_err();
        assert!(ist_bedienfehler(&e), "{e}");
    }

    #[test]
    fn bedienfehler_werden_erkannt() {
        assert!(ist_bedienfehler("Es fehlt das Ziel."));
        assert!(ist_bedienfehler("Unbekannte Option „--x“."));
        assert!(!ist_bedienfehler("Keine Verbindung zum Router."));
        assert!(ist_bedienfehler("Nach `--programm` fehlt der Name."));
        assert!(ist_bedienfehler(
            "Nach `--sekunden` gehört eine ganze Zahl von 1 bis 3600."
        ));
        assert!(ist_bedienfehler(
            "Gib ein bis vier Ziele an: `netzinfo live 1.1.1.1`."
        ));
        assert!(!ist_bedienfehler(
            "Das Programm `iw` fehlt. Installiere es."
        ));
        assert!(!ist_bedienfehler("Der Ordner ist unbekannt (HOME fehlt)."));
        assert!(!ist_bedienfehler(
            "networkQuality fehlt. Gibt es ab macOS 12."
        ));
    }
}
