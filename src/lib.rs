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
pub mod engpass;
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
        println!(
            "{}",
            umbruch(text, ERKLAERUNG, &" ".repeat(ERKLAERUNG.chars().count()))
        );
    }
}

/// Beginn jeder Erklärzeile. Ein Wort statt eines Pfeils: Screenreader lesen Symbole als „Pfeil“.
pub(crate) const ERKLAERUNG: &str = "  Erklärung: ";

/// Gibt Fließtext umgebrochen aus (ein Satz oder mehrere, ohne Einrückung).
pub fn satz(text: &str) {
    println!("{}", umbruch(text, "", ""));
}

/// Maximale Zeilenbreite der Terminal-Ausgabe, gezählt in Zeichen.
pub const ZEILE_MAX: usize = 100;

/// Bricht `text` an Wortgrenzen auf `ZEILE_MAX` Zeichen um. Die erste Zeile beginnt mit `erste`,
/// jede weitere mit `folge` (gleich breit, damit der Text bündig bleibt). Zählt `chars()`, nicht
/// Bytes, damit Umlaute die Breite nicht verfälschen. Ein einzelnes Wort bleibt ungekürzt.
pub fn umbruch(text: &str, erste: &str, folge: &str) -> String {
    let mut aus = String::new();
    let mut zeile = erste.to_string();
    let mut leer = true;
    for wort in text.split_whitespace() {
        let neu = zeile.chars().count() + 1 + wort.chars().count();
        if !leer && neu > ZEILE_MAX {
            aus += &zeile;
            aus.push('\n');
            zeile = folge.to_string();
            leer = true;
        }
        if !leer {
            zeile.push(' ');
        }
        zeile += wort;
        leer = false;
    }
    aus + &zeile
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

/// Läuft `prog` höchstens `grenze` und gibt stdout , stderr und Status zurück. Danach wird es beendet und
/// es kommt `TimedOut`. stdin ist zu. Ein Befehl, der hängt, hängt so nie Netzinfo.
pub(crate) fn befehl_status(
    prog: &str,
    args: &[&str],
    grenze: std::time::Duration,
) -> std::io::Result<(std::process::ExitStatus, String, String)> {
    use std::io::Read;
    use std::process::Stdio;
    let mut kind = std::process::Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    fn lesen(mut o: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            // ein Lesefehler lässt den Rest der Ausgabe weg; der Status sagt, ob der Lauf gelang
            let _ = o.read_to_end(&mut b);
            b
        })
    }
    let leser = kind.stdout.take().map(lesen);
    let fehler = kind.stderr.take().map(lesen);
    let start = std::time::Instant::now();
    let status = loop {
        match kind.try_wait()? {
            Some(s) => break s,
            None if start.elapsed() < grenze => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            None => {
                // der Prozess kann gerade selbst geendet sein, dann ist kill/wait egal
                let _ = kind.kill();
                let _ = kind.wait();
                return Err(std::io::ErrorKind::TimedOut.into());
            }
        }
    };
    let text = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
        String::from_utf8_lossy(&h.and_then(|h| h.join().ok()).unwrap_or_default()).into_owned()
    };
    Ok((status, text(leser), text(fehler)))
}

/// Wie `befehl_status`, aber ein Status ungleich 0 ist ein Fehler.
pub(crate) fn befehl(
    prog: &str,
    args: &[&str],
    grenze: std::time::Duration,
) -> std::io::Result<String> {
    let (status, out, _) = befehl_status(prog, args, grenze)?;
    if status.success() {
        Ok(out)
    } else {
        Err(std::io::Error::other(format!("{prog} meldete {status}")))
    }
}

/// Sperrt `<datei>.lock` für alle Prozesse (App, Menüleisten-Wache, Kommandozeile), solange der
/// Rückgabewert lebt. Wer lesen, ändern und schreiben will, hält sie darum herum, sonst überschreibt
/// der zweite Schreiber die Änderung des ersten. Wartet höchstens 5 Sekunden.
pub(crate) fn sperre(datei: &std::path::Path) -> Result<std::fs::File, String> {
    let fehler = |e: std::io::Error| {
        format!(
            "Die Datei {} ließ sich nicht sperren ({}). Prüfe den Ordner und versuche es noch mal.",
            datei.display(),
            io_satz(&e)
        )
    };
    if let Some(dir) = datei.parent() {
        std::fs::create_dir_all(dir).map_err(fehler)?;
    }
    let mut name = datei.as_os_str().to_owned();
    name.push(".lock");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(name)
        .map_err(fehler)?;
    let start = std::time::Instant::now();
    loop {
        match f.try_lock() {
            Ok(()) => return Ok(f),
            Err(std::fs::TryLockError::WouldBlock) if start.elapsed().as_secs() < 5 => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "Die Datei {} wird gerade von einem anderen Netzinfo beschrieben. Warte kurz und versuche es noch mal.",
                    datei.display()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(fehler(e)),
        }
    }
}

/// Zeitgrenze für Systembefehle, die sofort antworten sollten.
pub(crate) const BEFEHL_GRENZE: std::time::Duration = std::time::Duration::from_secs(10);

/// Pseudozufällige Bytes (xorshift, fester Startwert) für Parser-Tests mit kaputten Paketen.
#[cfg(test)]
pub(crate) fn kaputte_pakete(echt: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut zufall = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..3000).map(move |i| {
        let mut p = echt.to_vec();
        match i % 3 {
            0 => p.truncate((zufall() as usize) % (p.len() + 1)),
            1 => {
                for _ in 0..1 + zufall() % 4 {
                    if !p.is_empty() {
                        let n = p.len();
                        p[(zufall() as usize) % n] = zufall() as u8;
                    }
                }
            }
            _ => p = (0..zufall() % 600).map(|_| zufall() as u8).collect(),
        }
        p
    })
}

#[cfg(test)]
mod befehl_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn befehl_bricht_nach_der_grenze_ab() {
        let t = Instant::now();
        let e = befehl("sleep", &["30"], Duration::from_millis(200)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(io_satz(&e), "keine Antwort in der Zeit");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn sperre_haelt_zweiten_schreiber_fern() {
        let d = std::env::temp_dir().join(format!("netz-sperre-{}", std::process::id()));
        let p = d.join("x.json");
        let a = sperre(&p).unwrap();
        // zweiter Griff auf dieselbe Datei kommt nicht durch, solange `a` lebt
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(d.join("x.json.lock"))
            .unwrap();
        assert!(matches!(
            f.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(a);
        // ein parallel gestarteter Kindprozess erbt den Griff kurz bis zu seinem exec
        let frei = (0..100).any(|_| {
            f.try_lock().is_ok() || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                false
            }
        });
        assert!(frei);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn befehl_liefert_ausgabe_und_meldet_fehlstatus() {
        assert_eq!(
            befehl("echo", &["hallo"], BEFEHL_GRENZE).unwrap(),
            "hallo\n"
        );
        assert!(befehl("false", &[], BEFEHL_GRENZE).is_err());
        let (s, o, _) = befehl_status("false", &[], BEFEHL_GRENZE).unwrap();
        assert!(!s.success() && o.is_empty());
        let e = befehl("gibt-es-nicht-xyz", &[], BEFEHL_GRENZE).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }
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

    #[test]
    fn umbruch_zaehlt_zeichen_und_rueckt_ein() {
        let wort = "Größe ";
        let text = wort.repeat(40);
        let u = crate::umbruch(&text, "  Erklärung: ", "              ");
        let zeilen: Vec<&str> = u.lines().collect();
        assert!(zeilen.len() > 1);
        assert!(zeilen.iter().all(|z| z.chars().count() <= crate::ZEILE_MAX));
        assert!(
            zeilen[0].starts_with("  Erklärung: Größe")
                && zeilen[1].starts_with("              Größe")
        );
        assert_eq!(crate::umbruch("kurz", "- ", "  "), "- kurz");
        let lang = "x".repeat(150);
        assert_eq!(crate::umbruch(&lang, "", ""), lang);
    }
}
