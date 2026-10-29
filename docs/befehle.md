# Befehle im Detail

Ergänzt die Übersicht im [README](../README.md). Jeder Befehl erklärt mit `--erklaer` selbst, was er im Netz tut.

## Wie Netzinfo ein Netz wiedererkennt

`netzinfo hier` sagt oben, ob das Netz bekannt ist („Bekanntes Netz: WG, zuletzt am …“) oder neu. Erkannt wird es an Router, DHCP-Server, Domain und Subnetz, nicht am WLAN-Namen. Die Liste liegt in `~/Library/Application Support/netzinfo/netze.json` (anderer Pfad: Umgebungsvariable `NETZ_DATEN`). Zwei Netze mit demselben Werksrouter (z. B. zwei FRITZ!Boxen) sehen gleich aus.

## Was `pruefen` meldet

`netzinfo pruefen` sortiert jede Abweichung ein. Harmlos ist etwa eine neue öffentliche IP. Auffällig sind andere DNS-Server. Alarm gibt es, wenn jemand die HTTPS-Verschlüsselung aufbricht oder das Netz für Namen, die es nicht gibt, Adressen erfindet. Messungen, Prüfungen und Gerätelisten liegen neben `netze.json`, jeweils nach Netz getrennt. `pruefen`, `messen` und `geraete` laufen nur in gemerkten Netzen, `geraete` zusätzlich nur in Netzen, die als „meins“ markiert sind.

## Hersteller und zufällige Adressen

`netzinfo geraete --nachbarn` zeigt zu jeder MAC den Hersteller aus der eingebauten IEEE-Liste (`data/oui.tsv`, Quelle und Lizenzhinweis in [THIRD_PARTY.md](THIRD_PARTY.md)). Ist die Adresse zufällig (Handys mit „privater WLAN-Adresse“), steht dort „zufällige Adresse“: Sie verrät keinen Hersteller, bleibt aber pro Netz gleich und gilt deshalb beim nächsten Besuch als bekannt. `netzinfo pruefen` meldet ein neues Gerät mit zufälliger Adresse als „auffällig“, nie als Alarm. Die Liste aktualisiert nur ein Release: `python3 scripts/oui.py` holt die Register neu und schreibt `data/oui.tsv`.

## IPv6-Router

`netzinfo hier` zeigt auch, welche IPv6-Router das Netz ankündigt (Mac: `ndp`, Linux: `ip`, ohne Root und ohne Mitschnitt). Grenzen: Gesehen wird nur, was das System gelernt hat; ein Router, den das Gerät ignoriert, bleibt unsichtbar. Mehrere Router sind in Mesh- und Firmennetzen normal, deshalb entsteht aus diesem Signal allein nie ein Alarm, sondern höchstens „auffällig“. Alarm gibt es nur zusammen mit einem DHCP-Server, der nicht der Router ist.

## Nur im eigenen Netz scannen

`netzinfo ports` und `netzinfo geraete` laufen nur gegen private Adressen und nur, wenn das Netz als deins markiert ist (`netzinfo merken <Name> --meins`) oder du `--meins` angibst. Auf dem eigenen Rechner (Loopback) geht es immer. In fremden Netzen (Schule, Arbeit, Hotspot) macht `netzinfo` nur, was jeder Laptop beim Verbinden ohnehin tut.

## Ausgehende Ports

`netzinfo ports --raus` verbindet sich mit deinem eigenen Testserver auf 16 TCP-Ports (22, 25, 53, 80, 123, 143, 443, 465, 587, 853, 993, 1194, 3389, 5222, 8080, 8443), in zufälliger Reihenfolge, höchstens vier gleichzeitig. „offen“: Verbindung steht. „abgelehnt“: sofortige Ablehnung. „verschluckt“: keine Antwort in 3 Sekunden. Es sendet nur an diesen einen Server und braucht kein „meins“. Nur TCP, und eine Verbindung heißt nicht, dass Inhalte ungefiltert bleiben.

Testserver aufstellen (ein Linux-Rechner oder Server mit fester Adresse, Firewall für die Ports offen):

```sh
for p in 22 25 53 80 123 143 443 465 587 853 993 1194 3389 5222 8080 8443; do
  socat TCP-LISTEN:$p,fork,reuseaddr /dev/null &
done
```

Port 22 braucht dort einen anderen SSH-Port oder den echten `sshd`; Ports unter 1024 brauchen Root oder `setcap`. Einmal eintragen: `netzinfo ports --raus --ziel <Server>`. Danach vergleicht `netzinfo pruefen` in gemerkten Netzen mit der letzten Messung und meldet nur neu gesperrte Ports, nie als Alarm. Ohne Ziel sendet Netzinfo nichts.
