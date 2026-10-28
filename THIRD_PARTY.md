# Fremde Daten

## IEEE-Herstellerliste (`data/oui.tsv`)

Quelle: öffentliche Register MA-L, MA-M und MA-S der IEEE Registration Authority (https://standards-oui.ieee.org/). Aus ihnen bleiben nur Präfix und Firmenname übrig (Adressen entfallen, Namen auf 40 Zeichen gekürzt). Die Register sind frei abrufbar; eine ausdrückliche Lizenz für die Weitergabe haben wir nicht gefunden, die Frage ist nicht abschließend geklärt. Netzinfo lädt die Liste zur Laufzeit nie, sie steckt im Programm. Bei Einwänden der IEEE wird die Datei entfernt.

Neu erzeugen: `python3 scripts/oui.py` (nur Standardbibliothek, braucht Netz).
