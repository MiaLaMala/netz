#!/usr/bin/env python3
"""Baut die winzigen Testdateien testdata/ipdb/land.mmdb und asn.mmdb (nur Standardbibliothek, kein Netz).

Inhalt: nur die öffentlichen Resolver 1.1.1.1, 9.9.9.9 und 8.8.8.8. Die Werte sind erfunden genug
für Tests (8.8.8.0/24 hat ein Land, aber keinen Anbieter).
"""
import ipaddress, struct, sys
from pathlib import Path


def zahl(typ, n):  # uint16 (5), uint32 (6): Bytes ohne führende Nullen
    b = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return bytes([(typ << 5) | len(b)]) + b


def text(s):
    b = s.encode()
    assert 29 <= len(b) < 29 + 256 or len(b) < 29
    kopf = bytes([0x40 | len(b)]) if len(b) < 29 else bytes([0x40 | 29, len(b) - 29])
    return kopf + b


def karte(d):
    return bytes([0xE0 | len(d)]) + b"".join(text(k) + v for k, v in d.items())


def wert(v):
    return text(v) if isinstance(v, str) else zahl(6, v)


def baum(eintraege, art, epoche):
    daten, ort = b"", {}
    wurzel = {}
    for cidr, satz in eintraege:
        netz = ipaddress.ip_network(cidr)
        laenge = netz.prefixlen + (96 if netz.version == 4 else 0)
        schluessel = int(netz.network_address)  # IPv4 liegt in ::/96, also gleich der Zahl
        rec = karte({k: wert(v) for k, v in satz.items()})
        if rec not in ort:
            ort[rec] = len(daten)
            daten += rec
        knoten = wurzel
        for i in range(laenge):
            b = (schluessel >> (127 - i)) & 1
            if i == laenge - 1:
                knoten[b] = ("d", ort[rec])
            else:
                knoten = knoten.setdefault(b, {})
    # Knoten nummerieren (Breitensuche)
    reihe, nummer = [wurzel], {id(wurzel): 0}
    for k in reihe:
        for b in (0, 1):
            if isinstance(k.get(b), dict):
                nummer[id(k[b])] = len(reihe)
                reihe.append(k[b])
    n = len(reihe)

    def rec24(x):
        if x is None:
            return n
        if isinstance(x, dict):
            return nummer[id(x)]
        return n + 16 + x[1]

    out = b""
    for k in reihe:
        for b in (0, 1):
            out += rec24(k.get(b)).to_bytes(3, "big")
    meta = karte({
        "node_count": zahl(6, n), "record_size": zahl(5, 24), "ip_version": zahl(5, 6),
        "database_type": text(art), "binary_format_major_version": zahl(5, 2),
        "binary_format_minor_version": zahl(5, 0),
        "build_epoch": bytes([0x08, 0x02]) + epoche.to_bytes(8, "big"),
        "languages": bytes([0x00, 0x04]), "description": bytes([0xE0]),
    })
    return out + bytes(16) + daten + b"\xab\xcd\xefMaxMind.com" + meta


if __name__ == "__main__":
    ziel = Path(sys.argv[1] if len(sys.argv) > 1 else "testdata/ipdb")
    ziel.mkdir(parents=True, exist_ok=True)
    epoche = 1780000000  # 2026-05-28
    land = [
        ("1.1.1.0/24", {"country_code": "AU"}),
        ("8.8.8.0/24", {"country_code": "US"}),
        ("9.9.9.0/24", {"country_code": "US"}),
        ("2606:4700:4700::/48", {"country_code": "CA"}),
    ]
    cf = {"autonomous_system_number": 13335, "autonomous_system_organization": "CLOUDFLARENET"}
    asn = [
        ("1.1.1.0/24", cf),
        ("9.9.9.0/24", {"autonomous_system_number": 19281, "autonomous_system_organization": "QUAD9-AS-1"}),
        ("2606:4700:4700::/48", cf),
    ]
    (ziel / "land.mmdb").write_bytes(baum(land, "country", epoche))
    (ziel / "asn.mmdb").write_bytes(baum(asn, "asn", epoche + 3600))
