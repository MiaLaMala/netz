# Fremde Daten

## IEEE-Herstellerliste (`data/oui.tsv`)

Quelle: öffentliche Register MA-L, MA-M und MA-S der IEEE Registration Authority (https://standards-oui.ieee.org/). Aus ihnen bleiben nur Präfix und Firmenname übrig (Adressen entfallen, Namen auf 40 Zeichen gekürzt). Die Register sind frei abrufbar; eine ausdrückliche Lizenz für die Weitergabe haben wir nicht gefunden, die Frage ist nicht abschließend geklärt. Netzinfo lädt die Liste zur Laufzeit nie, sie steckt im Programm. Bei Einwänden der IEEE wird die Datei entfernt.

Neu erzeugen: `python3 scripts/oui.py` (nur Standardbibliothek, braucht Netz).

## Land und Anbieter (`ipdb/`, nicht im Git)

Zwei Offline-Datenbanken, die mit der App kommen: Land aus DB-IP "IP to Country Lite" (CC BY 4.0, Namensnennung: IP Geolocation by DB-IP, https://db-ip.com) und Anbieter aus IPtoASN (PDDL 1.0). Stand, Versionen und Links: `tools/QUELLEN`. Bauen: `tools/ipdb-bauen.sh`.
