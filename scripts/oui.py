#!/usr/bin/env python3
"""Builds data/oui.tsv from the public IEEE registries MA-L, MA-M and MA-S.

Run `python3 scripts/oui.py` (needs network, stdlib only) before a release.
Output lines: `<hex assignment>\t<organisation>`, sorted, names cut to 40 characters.
"""
import csv, io, pathlib, urllib.request

URLS = [f"https://standards-oui.ieee.org/{p}.csv" for p in ("oui/oui", "oui28/mam", "oui36/oui36")]
rows = {}
for url in URLS:
    req = urllib.request.Request(url, headers={"User-Agent": "curl/8.4.0"})
    text = urllib.request.urlopen(req, timeout=120).read().decode("utf-8")
    for r in csv.DictReader(io.StringIO(text)):
        name = " ".join(r["Organization Name"].replace("\u2013", "-").replace("\u2014", "-").split())[:40]
        if name and r["Assignment"]:
            rows[r["Assignment"].upper()] = name
out = pathlib.Path(__file__).resolve().parent.parent / "data" / "oui.tsv"
out.write_text("".join(f"{k}\t{v}\n" for k, v in sorted(rows.items())), encoding="utf-8")
print(f"{len(rows)} Einträge, {out.stat().st_size} Bytes")
