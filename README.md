# netz

Kennt jedes Netz, in dem dein Mac war, und meldet sich, wenn eins sich anders verhält als sonst.

Andere Netzwerk-Tools schauen auf ein Netz, und zwar jetzt. `netz` merkt sich jedes Netz (WG, Arbeit, Schule, Hotspot) und vergleicht beim nächsten Besuch: Ist es derselbe Router? Antwortet DNS wie immer? Hat das Zertifikat von github.com plötzlich jemand anderes unterschrieben? Mit `--erklaer` zeigt jeder Befehl auf Deutsch, was im Netz gerade passiert, passend zum Stoff der IHK-Prüfung.

## Befehle

| Befehl | Was er macht | Issue |
|---|---|---|
| `netz hier` | IP, Subnetz, Router und DNS des aktuellen Netzes | #1 |
| `netz subnetz 172.16.0.0/16` | Netz, Broadcast und Hostbereich ausrechnen | #1 |
| `netz dns <Name>` | Antwort deines Netzes gegen 1.1.1.1 und Quad9 | #2 |
| `netz tls [Host]` | Zertifikatsaussteller prüfen, SSL-Inspection erkennen | #3 |
| `netz ports <Host>` | offene Ports eines eigenen Geräts | #4 |
| `netz merken <Name> [--meins]` | aktuelles Netz speichern, optional als „meins“ markieren | #5 |
| `netz netze` | gemerkte Netze auflisten | #5 |
| `netz vergessen <Name>` | ein gemerktes Netz löschen | #5 |
| `netz pruefen` | was ist anders als beim letzten Besuch? Router, DNS, Umleitungen, Zertifikate, öffentliche IP | #6 |
| `netz messen [--minuten N]` | Antwortzeiten und Aussetzer sammeln | #7 |
| `netz bericht [Netz] [--markdown Datei]` | Median, Ausreißer, Verluste und längster Aussetzer, auch als Datei für den IT-Support | #7 |
| `netz geraete [CIDR]` | Geräte im eigenen Netz, neu und verschwunden | #8 |

`netz hier` sagt oben, ob das Netz bekannt ist („Bekanntes Netz: WG, zuletzt am …“) oder neu. Erkannt wird es an Router, DHCP-Server, Domain und Subnetz, nicht am WLAN-Namen. Die Liste liegt in `~/Library/Application Support/netz/netze.json` (anderer Pfad: Umgebungsvariable `NETZ_DATEN`). Zwei Netze mit demselben Werksrouter (z. B. zwei FRITZ!Boxen) sehen gleich aus.

`netz pruefen` sortiert jede Abweichung ein. Harmlos ist etwa eine neue öffentliche IP. Auffällig sind andere DNS-Server. Alarm gibt es, wenn jemand die HTTPS-Verschlüsselung aufbricht oder das Netz für Namen, die es nicht gibt, Adressen erfindet. Messungen, Prüfungen und Gerätelisten liegen neben `netze.json`, jeweils nach Netz getrennt. `pruefen`, `messen` und `geraete` laufen nur in gemerkten Netzen, `geraete` zusätzlich nur in Netzen, die als „meins“ markiert sind.

## Bauen

    cargo build --release
    ./target/release/netz hier --erklaer

Läuft auf macOS und braucht keine Root-Rechte.

## App

Die Mac-App liegt in `app/` (Tauri 2, Oberfläche in schlichtem HTML, CSS und JavaScript ohne npm). Sie braucht nur Rust und die Xcode Command Line Tools:

    cargo build -p netz-app
    ./target/debug/netz-app

Sie liest dieselbe Liste wie die Kommandozeile (`NETZ_DATEN` gilt auch hier). Die CI baut nur den Kern (`netz`), nicht die App. `app/vorschau.html` zeigt die Oberfläche mit erfundenen Daten in jedem Browser, ohne die App zu starten.

## Nur im eigenen Netz scannen

`netz ports` und später `netz geraete` laufen nur gegen private Adressen und nur, wenn das Netz als deins markiert ist (`netz merken <Name> --meins`) oder du `--meins` angibst. Auf dem eigenen Rechner (Loopback) geht es immer. In fremden Netzen (Schule, Arbeit, Hotspot) macht `netz` nur, was jeder Laptop beim Verbinden ohnehin tut.
