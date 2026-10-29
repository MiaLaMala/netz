# Sicherheit

## Lücke melden

Bitte nicht als öffentliches Issue. Nutze auf GitHub den Reiter „Security“ und dort „Report a vulnerability“ (Private vulnerability reporting). Ich antworte innerhalb von 7 Tagen, ohne Garantie: Netzinfo wird von einer Person gepflegt.

## Was Netzinfo tut

- Es scannt nur private Adressen und nur im Netz, das du mit `--meins` als deins markiert hast. Der eigene Rechner (Loopback) geht immer.
- In fremden Netzen macht es nur, was jeder Laptop beim Verbinden ohnehin tut.
- Es liest und schreibt nur die eigene Datei `netze.json` (Pfad über `NETZ_DATEN` änderbar) und die Dateien daneben.

## Das gilt als Sicherheitslücke

- Ein Scan oder Paketversand außerhalb privater Bereiche oder ohne die Schutzregel „meins“.
- Netzinfo liest oder schreibt Dateien außerhalb seines Datenordners (z. B. über einen manipulierten Netznamen).
- Eine unsichere Update-Prüfung, falls es je eine gibt (ungeprüfte Downloads).
- Eine Eingabe aus dem Netz (DNS-Antwort, Zertifikat, Antwortseite), die Netzinfo abstürzen lässt oder Befehle ausführt.

Nicht gemeint sind Hinweise auf Abhängigkeiten, die schon `cargo deny` in der CI meldet; dafür reicht ein normales Issue.
