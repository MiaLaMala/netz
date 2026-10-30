# Mitarbeit

## Fehler melden (zwei Minuten)

1. Version holen: `netzinfo --version`, in der App der Eintrag „About Netzinfo“ im Menü „Netzinfo“.
2. Den Befehl noch einmal mit `--erklaer` starten und die Ausgabe kopieren. Bei der App: Ansicht, Text oder Screenshot.
3. Echte IP-Adressen, MACs, Hostnamen und WLAN-Namen ersetzen durch `192.0.2.x`, `198.51.100.x`, `203.0.113.x`, `2001:db8::` und `aa:bb:cc:dd:ee:ff`. Das gilt auch für Screenshots.
4. [Fehler melden](https://github.com/MiaLaMala/netzinfo/issues/new?template=fehler.yml) und die Felder ausfüllen. Eine Sicherheitslücke bitte nicht hier, sondern wie in [SECURITY.md](SECURITY.md).

## Code beitragen

- Ein Issue, ein Zweig, ein Pull Request. Der PR schließt das Issue mit `Closes #N`.
- Befehle, Ausgaben und Doku auf Deutsch, mit echten Umlauten, „deutschen“ Anführungszeichen und ohne Gedankenstriche. Code-Bezeichner und Kommentare auf Englisch.
- Jeder Befehl erklärt mit `--erklaer` (über `crate::explain`), was er im Netz tut. Die Sätze soll jemand in der Ausbildung verstehen.
- Ein neuer Befehl braucht eine Zeile in `GRUPPEN` in `src/main.rs` (Hilfe, `<Befehl> --help` und „Meintest du“ lesen von dort; ein Test prüft Hilfe und Dispatch gegeneinander) und eine Zeile in README und `docs/befehle.md`. Geht er nur auf dem Mac, steht „(nur Mac)“ dahinter.
- Fehler sind `Result<_, String>` mit einem deutschen Satz: was schiefging und was man tun kann. Kein `unwrap()` oder `expect()` auf Daten von außen (Netz, Ausgaben anderer Programme, Dateien).
- Neue Abhängigkeiten nur, wenn die Standardbibliothek wirklich nicht reicht, mit Begründung im PR.
- Vor jedem Commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. Parser und Rechenlogik bekommen Tests mit echten Beispieldaten.
- Die CI läuft auf Linux und auf dem Mac (Clippy, Tests und Übersetzen der App); was nur unter macOS geht, gehört in `src/plattform/macos.rs` oder hinter `cfg(target_os = "macos")`.
- Gescannt wird nur gegen private Adressen und nur nach Bestätigung (siehe README).
- Beiträge stehen unter derselben Lizenz wie das Projekt (MIT, siehe `LICENSE`).
