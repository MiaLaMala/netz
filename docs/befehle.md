# Befehle im Detail

Ergänzt die Übersicht im [README](../README.md). Jeder Befehl erklärt mit `--erklaer` selbst, was er im Netz tut.

## Hilfe, Fehler und Exit-Codes

`netzinfo` ohne Angabe und `netzinfo --help` nennen oben die ersten Schritte (`hier`, `merken`, `pruefen`) und listen dann alle Befehle nach Thema. `netzinfo <Befehl> --help` (auch `netzinfo hilfe <Befehl>`) zeigt nur die Zeilen dieses Befehls, ein Beispiel und was der Befehl ins Netz sendet. `netzinfo --version` nennt Version und Ziel, etwa `netzinfo 0.1.0 (aarch64-apple-darwin)`. Die Hilfe bricht bei 80 Zeichen um. Bei einem Tippfehler in Befehl oder Option kommt „Meintest du …?“. Was es nur auf dem Mac gibt, steht in der Hilfe als „(nur Mac)“ oder unter „Dieser Mac“; unter Linux antwortet der Befehl mit einem Satz, warum er dort nicht geht. Exit-Code 0: geklappt. 1: Der Befehl lief, aber etwas ging schief (kein Netz, Zeitüberschreitung, Gerät nicht erreichbar). 2: falscher Aufruf (unbekannter Befehl oder unbekannte Option, fehlendes Argument). `pruefen` meldet Abweichungen im Text, nicht im Exit-Code.

## Ausgabe lesen (auch mit Screenreader)

Jede Ausgabe ist reiner Text, eine Zeile nach der anderen. Kopfzeilen haben die Form `Name: Wert` (`Netz: WG`, `Router: 192.0.2.1`, `Ziel: 203.0.113.5`), Abschnitte stehen als Zeile mit Doppelpunkt (`Ereignisse:`), die Zeilen darunter sind eingerückt. Erklärungen von `--erklaer` beginnen mit dem Wort „Erklärung:“. Langer Text bricht bei 100 Zeichen um (die Hilfe bei 80). Es gibt keine Rahmen und keine Farbe, also tut `NO_COLOR` nichts, und Zeichen sind immer von einem Wort begleitet. `netzinfo live` malt im Terminal eine Blockkurve und überschreibt seine Zeilen; mit `TERM=dumb` oder Umleitung bleibt nur Text.

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

## Fritz!Box-Geräteliste

`netzinfo fritzbox` liest die Geräteliste einer Fritz!Box über TR-064, ihren Fernzugriff im Heimnetz, und mischt sie über die MAC-Adresse in die eigene Liste (Quelle „Router“, dazu Verbindung LAN oder WLAN und aktiv). Der Router kennt auch Geräte, die auf keinen Scan antworten; ein Gerät, das nur er kennt, heißt in der App „vom Router“. Es läuft nur im Netz, das als „meins“ markiert ist, nur gegen das Gateway und nur lesend: `Hosts:1` mit `GetHostNumberOfEntries` und `GetGenericHostEntry`, eine feste Erlaubnisliste im Code. TR-064 muss in der Fritz!Box eingeschaltet sein (Heimnetz, Netzwerk, Netzwerkeinstellungen, Heimnetzfreigaben: „Zugriff für Anwendungen zulassen“), und es braucht einen eigenen Benutzer mit dem Recht „FRITZ!Box Einstellungen“. `netzinfo fritzbox --zugang <Benutzer>` fragt das Passwort im Terminal ab und legt beides im macOS-Schlüsselbund ab, `--entfernen` löscht den Eintrag. In der App geht das über die Router-Karte in der Ansicht „Geräte“. Die Anmeldung ist HTTP Digest (MD5, nach AVMs „TR-064 - First Steps“); das Passwort geht nicht im Klartext über das Netz und steht in keiner Datei. Unter Linux und Windows gibt es das noch nicht (kein Klartext-Ersatz).


## Engpass: Leitung, WLAN und Mac

`netzinfo engpass` legt drei Zahlen nebeneinander und nennt den Engpass in einem Satz. **Leitung**: die Sync-Rate laut Router, nicht der Vertrag. Bei einer Fritz!Box mit Zugang (siehe oben) per TR-064 `GetCommonLinkProperties` und `X_AVM-DE_GetAddonInfos` (Dienst `WANCommonInterfaceConfig`, Quelle: AVM „TR-064 Support WANCommonInterfaceConfig“, Version 22). Den Vertragswert (`TariffDataRate`) schicken nur manche Anbieter; „0“ heißt: kein Wert, dann steht in der Zeile kein „gebucht“. Ohne Zugang und bei anderen Routern fragt Netzinfo per UPnP-IGD ohne Anmeldung (bei der Fritz!Box nur mit „Statusinformationen über UPnP übertragen“). Geht beides nicht, sagt die Zeile, was fehlt. Es werden nur diese Lese-Aktionen gesendet, nur an das Gateway und nur in einem Netz, das als „meins“ markiert ist. **WLAN**: die Senderate des Macs (CoreWLAN), umgerechnet mit dem festen Faktor 0,6 auf den üblichen Nutzdurchsatz; eine grobe Faustregel (50 bis 70 Prozent je nach Standard und Umfeld). Am Kabel entfällt die Zeile. **Am Mac**: ohne Option der letzte gespeicherte Lauf von `netzinfo last` in diesem Netz, mit `--start` eine neue Messung mit Apples `networkQuality` (belastet die Leitung etwa 30 Sekunden; im Mobilfunk-Hotspot nur mit `--trotzdem`). Die Bewertung nimmt als Sollwert den Vertrag, sonst die Leitung: Die Leitung bremst, wenn sie unter 90 Prozent des Vertrags liegt. Am Mac kommen mindestens 80 Prozent des Sollwerts an: dann ist alles gut (ohne Vertragswert bleibt offen, ob das reicht). Sonst bremst das WLAN, wenn sein Nutzdurchsatz unter 90 Prozent des Sollwerts liegt, und sonst etwas zwischen Router und Mac. Die Breitbandmessung der Bundesnetzagentur (breitbandmessung.de) ersetzt das nicht; sie ist für eine Minderung nötig.
