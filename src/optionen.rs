//! Gemeinsame Prüfung der Optionen: Jeder Befehl ruft `nur` als Erstes auf.

/// Lehnt jede Option ab, die nicht in `erlaubt` steht. Werte hinter Optionen und
/// Positionsargumente (Host, Name, Zahl) sind keine Optionen und bleiben erlaubt.
/// Die Meldung beginnt mit „Unbekannte“, daran erkennt `main` den Bedienfehler (Exit 2).
pub fn nur(args: &[String], erlaubt: &[&str], befehl: &str) -> Result<(), String> {
    let Some(falsch) = args
        .iter()
        .find(|a| ist_option(a) && !erlaubt.contains(&a.as_str()))
    else {
        return Ok(());
    };
    // `--erklaer` und `--help` nimmt `main` vorher heraus, sie stehen nie in `erlaubt`.
    let tipp = match aehnlich(falsch, &[erlaubt, &["--erklaer", "--help"]].concat()) {
        Some(o) => format!(" Meintest du `{o}`?"),
        None => String::new(),
    };
    Err(match erlaubt {
        [] => {
            format!("Unbekannte Option „{falsch}“.{tipp} `netzinfo {befehl}` kennt keine Optionen.")
        }
        [eine] => format!(
            "Unbekannte Option „{falsch}“ für `netzinfo {befehl}`.{tipp} Erlaubt ist: {eine}."
        ),
        _ => format!(
            "Unbekannte Option „{falsch}“ für `netzinfo {befehl}`.{tipp} Erlaubt sind: {}.",
            erlaubt.join(", ")
        ),
    })
}

/// Die Option aus `kandidaten`, die `eingabe` am nächsten kommt (Abstand höchstens 2 und kleiner
/// als die Länge der Eingabe), sonst `None`.
pub fn aehnlich<'a>(eingabe: &str, kandidaten: &[&'a str]) -> Option<&'a str> {
    let n = eingabe.chars().count();
    kandidaten
        .iter()
        .map(|k| (levenshtein(eingabe, k), *k))
        .filter(|(d, _)| *d <= 2 && *d < n)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
}

/// Editierabstand zweier Wörter (für „Meintest du“).
pub fn levenshtein(a: &str, b: &str) -> usize {
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

/// „-x“ und „--x“ sind Optionen, „-5“ und „-“ nicht.
fn ist_option(a: &str) -> bool {
    let mut z = a.chars();
    z.next() == Some('-') && z.next().is_some_and(|c| !c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn unbekannt_wird_abgelehnt() {
        let e = nur(&a(&["--x"]), &["--tempo", "--verschluesselt"], "dns").unwrap_err();
        assert_eq!(
            e,
            "Unbekannte Option „--x“ für `netzinfo dns`. Erlaubt sind: --tempo, --verschluesselt."
        );
        assert!(
            nur(&a(&["-x"]), &[], "hier")
                .unwrap_err()
                .contains("keine Optionen")
        );
        assert!(
            nur(&a(&["--j"]), &["--json"], "zeit")
                .unwrap_err()
                .contains("Erlaubt ist: --json.")
        );
    }

    #[test]
    fn tippfehler_in_option_bekommt_meintest_du() {
        let e = nur(&a(&["--nachbar"]), &["--nachbarn", "--karten"], "geraete").unwrap_err();
        assert!(e.contains("Meintest du `--nachbarn`?"), "{e}");
        let e = nur(&a(&["--hlep"]), &["--json"], "zeit").unwrap_err();
        assert!(e.contains("Meintest du `--help`?"), "{e}");
        let e = nur(&a(&["--erklar"]), &[], "hier").unwrap_err();
        assert!(e.contains("Meintest du `--erklaer`?"), "{e}");
    }

    #[test]
    fn erlaubtes_werte_und_positionen_gehen_durch() {
        assert!(nur(&a(&["--tempo"]), &["--tempo"], "dns").is_ok());
        assert!(
            nur(
                &a(&["--ziel", "example.org", "--raus"]),
                &["--raus", "--ziel"],
                "ports"
            )
            .is_ok()
        );
        assert!(nur(&a(&["192.0.2.0/24", "-5", "-"]), &[], "subnetz").is_ok());
        assert!(nur(&[], &[], "hier").is_ok());
    }
}
