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
    Err(match erlaubt {
        [] => format!("Unbekannte Option „{falsch}“. `netzinfo {befehl}` kennt keine Optionen."),
        [eine] => {
            format!("Unbekannte Option „{falsch}“ für `netzinfo {befehl}`. Erlaubt ist: {eine}.")
        }
        _ => format!(
            "Unbekannte Option „{falsch}“ für `netzinfo {befehl}`. Erlaubt sind: {}.",
            erlaubt.join(", ")
        ),
    })
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
