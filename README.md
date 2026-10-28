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

Geplant: Netze wiedererkennen (#5), Vergleich mit dem letzten Besuch (#6), Qualität über Zeit (#7), Geräte im eigenen Netz (#8), Wache im Hintergrund (#9).

## Bauen

    cargo build --release
    ./target/release/netz hier --erklaer

Läuft auf macOS und braucht keine Root-Rechte.

## Nur im eigenen Netz scannen

`netz ports` und später `netz geraete` laufen nur gegen private Adressen und nur, wenn du bestätigst, dass das Netz deins ist. In fremden Netzen (Schule, Arbeit, Hotspot) macht `netz` nur, was jeder Laptop beim Verbinden ohnehin tut.
