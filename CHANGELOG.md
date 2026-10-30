# Änderungen

Alle wichtigen Änderungen an Netzinfo stehen hier. Das Format folgt [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), die Versionen folgen [Semantic Versioning](https://semver.org/lang/de/).

## [0.1.0]

Die erste Veröffentlichung. Netzinfo merkt sich jedes Netz, in dem dein Mac war, und meldet sich, wenn eins sich anders verhält als sonst.

### Hinzugefügt

- Netzinfo vergleicht ein Netz mit dem letzten Besuch (Router, DNS, Zertifikate, öffentliche IP, Portfreigaben, Weg ins Internet) und sagt in einem Satz, ob alles wie immer ist.
- Die Engpass-Karte stellt Leitung laut Router, WLAN und Mac nebeneinander und nennt den Bremser und was hilft.
- Das Störungsprotokoll hält fest, wann das Internet weg war und woran es lag, und schreibt daraus einen sachlichen Text für die Hotline des Anbieters.
- Geräte im Netz sucht Netzinfo nur in Netzen, die du als „meins“ markierst, auf Wunsch mit den Namen aus der Fritz!Box.
- Verbindungen zeigt, welche Programme auf dem Mac mit wem verbunden sind und welche lauschen, mit Land und Anbieter des Ziels.
- Die Wache in der Menüleiste prüft nach jedem Netzwechsel und schickt eine Mitteilung, wenn etwas auffällt.
- Die Kommandozeile `netzinfo` läuft auf dem Mac und unter Linux. `--erklaer` sagt auf Deutsch, was im Netz passiert.
- Kein Server, kein Konto, keine Telemetrie. Das Fritz!Box-Passwort liegt nur im Schlüsselbund.

### Installieren

1. `Netzinfo_0.1.0_aarch64.dmg` laden, öffnen und Netzinfo in den Ordner „Programme“ ziehen.
2. Starten. macOS warnt, weil die App nicht notarisiert ist. Warnung mit „Fertig“ schließen, dann Systemeinstellungen, Datenschutz & Sicherheit, ganz nach unten, „Trotzdem öffnen“.
3. Für die Kommandozeile gibt es je ein Archiv für den Mac, für Linux x86_64 und für Linux arm64 (Raspberry Pi). Prüfsummen stehen in `SHA256SUMS`.

Die ganze Anleitung steht im [README](https://github.com/MiaLaMala/netzinfo#installieren).

### Bekannt fehlt

- Die App gibt es nur für den Mac mit Apple Silicon und macOS 13 oder neuer. Unter Linux gibt es nur die Kommandozeile, Windows gibt es nicht.
- Die App ist nur ad hoc signiert, nicht notarisiert. Darum der Umweg über „Trotzdem öffnen“. Nach einem Update kann macOS die Freigabe für „Lokales Netzwerk“ noch einmal abfragen.
- Die Linux-Fassung lief bisher nur in der CI, nicht von Hand auf einem Server oder Raspberry Pi.

### Dank

Herstellerliste aus dem IEEE-Register, Länder von [DB-IP](https://db-ip.com) (CC BY 4.0), Anbieter von IPtoASN (PDDL 1.0). Die Strichbilder der leeren Zustände sind mit Higgsfield (Recraft) erzeugt.
