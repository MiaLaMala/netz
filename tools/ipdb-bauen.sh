#!/bin/sh
# Lädt die zwei Offline-Datenbanken (zusammen etwa 18 MB) nach ${1:-ipdb}/ als land.mmdb und asn.mmdb.
# Nur zum Bauen, Netzinfo selbst lädt nie etwas. Quellen und Lizenzen: tools/QUELLEN.
# Die Dateien sind fertige .mmdb aus dem Paket ip-location-db (npm, über jsDelivr), auf feste Versionen
# und Prüfsummen festgelegt. Aktualisieren: Version und Prüfsumme unten ändern, Stand in QUELLEN nachziehen.
set -eu
ziel=${1:-ipdb}
mkdir -p "$ziel"
holen() { # Paket Version Datei Name Prüfsumme
  curl -fsSL --max-filesize 12000000 \
    "https://cdn.jsdelivr.net/npm/@ip-location-db/$1@$2/$3" -o "$ziel/$4"
  echo "$5  $ziel/$4" | shasum -a 256 -c -
}
holen dbip-country-mmdb 2.3.2026060120 dbip-country.mmdb land.mmdb \
  4e7f53dd9c6ebe0e7244d5dd4cc03639bc9dbe0a5a83eca90db50eeaeea72024
holen iptoasn-asn-mmdb 2.3.2026061719 iptoasn-asn.mmdb asn.mmdb \
  572ea9963fc9db6d8ac1eba8268ba66774fb83281e4f3a316c5e6abcb5f50cb9
