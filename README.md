# Netzinfo

## English in short

Netzinfo remembers every network your Mac has been on and tells you when one behaves differently than before (new router, changed DNS answers, TLS interception, new devices). It is a Rust CLI (`netzinfo hier`, `dns`, `tls`, `pruefen`, `messen`, `geraete`, `pfad`, `portal`, `offen`) plus a Tauri Mac app. Output and docs are in German; `--erklaer` explains in plain German what each command does on the wire.
Scanning only targets private addresses on networks you marked as yours (`--meins`). Install: see the German "Installieren" section below. Status: macOS is the main platform, the CLI core also builds on Linux (CI), Windows is untested. License: MIT. Security reports: see [SECURITY.md](SECURITY.md).

Kennt jedes Netz, in dem dein Mac war, und meldet sich, wenn eins sich anders verhält als sonst.

Andere Netzwerk-Tools schauen auf ein Netz, und zwar jetzt. `netzinfo` merkt sich jedes Netz (WG, Arbeit, Schule, Hotspot) und vergleicht beim nächsten Besuch: Ist es derselbe Router? Antwortet DNS wie immer? Hat das Zertifikat von github.com plötzlich jemand anderes unterschrieben? Mit `--erklaer` zeigt jeder Befehl auf Deutsch, was im Netz gerade passiert, passend zum Stoff der IHK-Prüfung.

## Befehle

| Befehl | Was er macht | Issue |
|---|---|---|
| `netzinfo hier` | IP, Subnetz, Router und DNS des aktuellen Netzes | #1 |
| `netzinfo subnetz 172.16.0.0/16` | Netz, Broadcast und Hostbereich ausrechnen | #1 |
| `netzinfo dns <Name>` | Antwort deines Netzes gegen 1.1.1.1 und Quad9 | #2 |
| `netzinfo tls [Host]` | Zertifikatsaussteller prüfen, SSL-Inspection erkennen | #3 |
| `netzinfo ports <Host>` | offene Ports eines eigenen Geräts | #4 |
| `netzinfo ports --raus [--ziel <Server>]` | welche ausgehenden TCP-Ports das Netz durchlässt (siehe „Ausgehende Ports“) | #54 |
| `netzinfo merken <Name> [--meins]` | aktuelles Netz speichern, optional als „meins“ markieren | #5 |
| `netzinfo netze` | gemerkte Netze auflisten | #5 |
| `netzinfo vergessen <Name>` | ein gemerktes Netz löschen | #5 |
| `netzinfo pruefen` | was ist anders als beim letzten Besuch? Router, DNS, Umleitungen, Zertifikate, öffentliche IP | #6 |
| `netzinfo messen [--minuten N]` | Antwortzeiten und Aussetzer sammeln | #7 |
| `netzinfo bericht [Netz] [--markdown Datei]` | Median, Ausreißer, Verluste und längster Aussetzer, auch als Datei für den IT-Support | #7 |
| `netzinfo geraete [CIDR] [--nachbarn]` | Geräte im eigenen Netz, neu und verschwunden, mit `--nachbarn` auch MAC-Adressen und Geräte ohne offenen Port | #8, #26 |
| `netzinfo pfad [Ziel]` | Weg zum Ziel Hop für Hop mit Verlust und Laufzeiten, ohne sudo | #30 |
| `netzinfo portal [--json]` | Hängt das Netz hinter einer Anmeldeseite (Captive Portal)? | #32 |
| `netzinfo offen [--json]` | Welche Programme auf diesem Rechner lauschen, nur lokal oder fürs ganze Netz | #38 |

`netzinfo hier` sagt oben, ob das Netz bekannt ist („Bekanntes Netz: WG, zuletzt am …“) oder neu. Erkannt wird es an Router, DHCP-Server, Domain und Subnetz, nicht am WLAN-Namen. Die Liste liegt in `~/Library/Application Support/netzinfo/netze.json` (anderer Pfad: Umgebungsvariable `NETZ_DATEN`). Zwei Netze mit demselben Werksrouter (z. B. zwei FRITZ!Boxen) sehen gleich aus.

`netzinfo pruefen` sortiert jede Abweichung ein. Harmlos ist etwa eine neue öffentliche IP. Auffällig sind andere DNS-Server. Alarm gibt es, wenn jemand die HTTPS-Verschlüsselung aufbricht oder das Netz für Namen, die es nicht gibt, Adressen erfindet. Messungen, Prüfungen und Gerätelisten liegen neben `netze.json`, jeweils nach Netz getrennt. `pruefen`, `messen` und `geraete` laufen nur in gemerkten Netzen, `geraete` zusätzlich nur in Netzen, die als „meins“ markiert sind.

`netzinfo geraete --nachbarn` zeigt zu jeder MAC den Hersteller aus der eingebauten IEEE-Liste (`data/oui.tsv`, Quelle und Lizenzhinweis in [THIRD_PARTY.md](THIRD_PARTY.md)). Ist die Adresse zufällig (Handys mit „privater WLAN-Adresse“), steht dort „zufällige Adresse“: Sie verrät keinen Hersteller, bleibt aber pro Netz gleich und gilt deshalb beim nächsten Besuch als bekannt. `netzinfo pruefen` meldet ein neues Gerät mit zufälliger Adresse als „auffällig“, nie als Alarm. Die Liste aktualisiert nur ein Release: `python3 scripts/oui.py` holt die Register neu und schreibt `data/oui.tsv`.

`netzinfo hier` zeigt auch, welche IPv6-Router das Netz ankündigt (Mac: `ndp`, Linux: `ip`, ohne Root und ohne Mitschnitt). Grenzen: Gesehen wird nur, was das System gelernt hat; ein Router, den das Gerät ignoriert, bleibt unsichtbar. Mehrere Router sind in Mesh- und Firmennetzen normal, deshalb entsteht aus diesem Signal allein nie ein Alarm, sondern höchstens „auffällig“. Alarm gibt es nur zusammen mit einem DHCP-Server, der nicht der Router ist.

## Installieren

Unter [Releases](https://github.com/MiaLaMala/netzinfo/releases) liegen die App als `.dmg`, die Kommandozeile als `netzinfo-<tag>-aarch64-apple-darwin.tar.gz` und `SHA256SUMS` (`shasum -a 256 -c SHA256SUMS`). Nur Apple Silicon, macOS 13 oder neuer.

1. `.dmg` öffnen und `Netzinfo` in den Ordner „Programme“ ziehen.
2. Netzinfo starten. macOS meldet, dass es die App nicht prüfen konnte: Das Programm ist nur ad hoc signiert, nicht notarisiert. Auf aktuellem macOS hilft Rechtsklick und „Öffnen“ nicht mehr.
3. „Fertig“ wählen, dann Systemeinstellungen, Datenschutz & Sicherheit, ganz nach unten scrollen, bei Netzinfo „Trotzdem öffnen“ und mit Passwort oder Touch ID bestätigen.
4. Beim ersten Scan fragt macOS nach „Lokales Netzwerk“. Erlauben, sonst findet Netzinfo keine Geräte.

Im Terminal geht Schritt 2 und 3 auch so: `xattr -dr com.apple.quarantine /Applications/Netzinfo.app`. Dasselbe gilt für das entpackte `netzinfo` aus dem Archiv. Nach einem Update kann macOS die Berechtigung „Lokales Netzwerk“ erneut abfragen, weil die Ad-hoc-Signatur bei jedem Build anders ist.

## Bauen

    cargo build --release
    ./target/release/netzinfo hier --erklaer

Läuft auf macOS und braucht keine Root-Rechte.

## App

Die Mac-App liegt in `app/` (Tauri 2, Oberfläche in schlichtem HTML, CSS und JavaScript ohne npm). Sie braucht nur Rust und die Xcode Command Line Tools:

    cargo build -p netzinfo-app
    ./target/debug/netzinfo-app

Sie liest dieselbe Liste wie die Kommandozeile (`NETZ_DATEN` gilt auch hier). Die CI baut nur den Kern (`netzinfo`), nicht die App. `app/vorschau.html` zeigt die Oberfläche mit erfundenen Daten in jedem Browser, ohne die App zu starten.

## Nur im eigenen Netz scannen

`netzinfo ports` und `netzinfo geraete` laufen nur gegen private Adressen und nur, wenn das Netz als deins markiert ist (`netzinfo merken <Name> --meins`) oder du `--meins` angibst. Auf dem eigenen Rechner (Loopback) geht es immer. In fremden Netzen (Schule, Arbeit, Hotspot) macht `netzinfo` nur, was jeder Laptop beim Verbinden ohnehin tut.

## Lizenz

MIT, siehe [LICENSE](LICENSE).

## Ausgehende Ports

`netzinfo ports --raus` verbindet sich mit deinem eigenen Testserver auf 16 TCP-Ports (22, 25, 53, 80, 123, 143, 443, 465, 587, 853, 993, 1194, 3389, 5222, 8080, 8443), in zufälliger Reihenfolge, höchstens vier gleichzeitig. „offen“: Verbindung steht. „abgelehnt“: sofortige Ablehnung. „verschluckt“: keine Antwort in 3 Sekunden. Es sendet nur an diesen einen Server und braucht kein „meins“. Nur TCP, und eine Verbindung heißt nicht, dass Inhalte ungefiltert bleiben.

Testserver aufstellen (ein Linux-Rechner oder Server mit fester Adresse, Firewall für die Ports offen):

```sh
for p in 22 25 53 80 123 143 443 465 587 853 993 1194 3389 5222 8080 8443; do
  socat TCP-LISTEN:$p,fork,reuseaddr /dev/null &
done
```

Port 22 braucht dort einen anderen SSH-Port oder den echten `sshd`; Ports unter 1024 brauchen Root oder `setcap`. Einmal eintragen: `netzinfo ports --raus --ziel <Server>`. Danach vergleicht `netzinfo pruefen` in gemerkten Netzen mit der letzten Messung und meldet nur neu gesperrte Ports, nie als Alarm. Ohne Ziel sendet Netzinfo nichts.
