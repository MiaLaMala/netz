//! netz: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.

mod dns;
mod hier;
mod ports;
mod subnetz;
mod tls;

use std::process::ExitCode;

const HELP: &str = "\
netz: kennt jedes Netz, in dem dein Mac war, und meldet, was anders ist.

Befehle:
  netz hier              Was ist das für ein Netz? IP, Subnetz, Router, DNS
  netz subnetz <CIDR>    Subnetz ausrechnen, z. B. netz subnetz 172.16.0.0/16
  netz dns <Name>        DNS-Antwort deines Netzes mit 1.1.1.1 und Quad9 vergleichen
  netz tls [Host]        Wer hat das Zertifikat unterschrieben? Erkennt SSL-Inspection
  netz ports <Host>      Offene Ports eines eigenen Geräts

Optionen:
  --erklaer              Erklärt bei jedem Schritt, was im Netz passiert
";

/// Prints a German explanation line when `--erklaer` is on.
pub fn explain(on: bool, text: &str) {
    if on {
        println!("  ↳ {text}");
    }
}

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
        "hilfe" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!(
            "Unbekannter Befehl „{other}“. `netz hilfe` zeigt alle Befehle."
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
