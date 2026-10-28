# Mitarbeit

- Ein Issue, ein Zweig, ein Pull Request. Der PR schließt das Issue mit `Closes #N`.
- Befehle, Ausgaben und Doku auf Deutsch, mit echten Umlauten, „deutschen“ Anführungszeichen und ohne Gedankenstriche. Code-Bezeichner und Kommentare auf Englisch.
- Jeder Befehl erklärt mit `--erklaer` (über `crate::explain`), was er im Netz tut. Die Sätze soll jemand in der Ausbildung verstehen.
- Fehler sind `Result<_, String>` mit einem deutschen Satz: was schiefging und was man tun kann. Kein `unwrap()` oder `expect()` auf Daten von außen (Netz, Ausgaben anderer Programme, Dateien).
- Neue Abhängigkeiten nur, wenn die Standardbibliothek wirklich nicht reicht, mit Begründung im PR.
- Vor jedem Commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. Parser und Rechenlogik bekommen Tests mit echten Beispieldaten.
- Gescannt wird nur gegen private Adressen und nur nach Bestätigung (siehe README).
