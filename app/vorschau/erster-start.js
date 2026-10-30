// Marke erster-start: Fremder öffnet die App zum ersten Mal. Keine gemerkten Netze, keine Messung, keine Karten, WLAN ohne Namen,
// Standort und „Lokales Netzwerk“ unentschieden (ort-erlaubt, ort-abgelehnt, ort-eingeschraenkt ändern nur den Standort).
// Weitere Zustände: erster-start-gemerkt (Netz „WG“ gemerkt, aber nicht „meins“), erster-start-meins (gemerkt und „meins“),
// dazu geraete-lauf (Suche klickt, vier Geräte, Erstlauf) oder geraete-leer (Suche findet nichts: Hilfe bei verweigertem „Lokalen Netzwerk“).
// Marken je Ansicht: tab-uebersicht,erster-start  tab-tempo,erster-start  tab-geraete,geraete,erster-start  tab-verbindungen,verbindungen,erster-start
// tab-gemerkte-netze,einst,erster-start. Fürs Video: „Dieses Netz merken …“ im Formular (mit Haken „meins“) macht das Netz bekannt,
// danach darf die Gerätesuche laufen; ohne Haken lehnt sie ab wie der Kern. Lädt nach den anderen Vorschau-Dateien und baut auf ihnen auf.
(() => {
  if (!marken.includes('erster-start')) return;
  // Erst jetzt, nach allen anderen Dateien: Deren Klicks beim Laden (geraete*, wlan-*, verbindungen*) sollen nicht auslösen, die Antworten schon.
  marken.push('wlan-gut', 'engpass-ungemessen', 'verbindungen-keinnetz');
  // Gerade gemerkt heißt: noch nichts zum Vergleichen. Ohne eigene pruefung-Marke ist die Prüfung der erste Besuch.
  if (!marken.some((m) => m.startsWith('pruefung-'))) marken.push('pruefung-erst');
  let name = marken.includes('erster-start-gemerkt') || marken.includes('erster-start-meins') ? 'WG' : null;
  let meins = marken.includes('erster-start-meins');
  const merken = VORSCHAU.netz_merken;
  VORSCHAU.netz_merken = async (a) => { await merken(a); name = a.name.trim(); meins = !!a.meins; };
  VORSCHAU.gemerkte_netze = async () => (name ? [{ name, fingerprint: { router: '192.0.2.1', dhcp_server: '192.0.2.1', domain: 'fritz.box', subnet: '192.0.2.0/24' }, meins, first_seen: t, last_seen: t }] : []);
  const dieses = VORSCHAU.dieses_netz;
  VORSCHAU.dieses_netz = async (a) => {
    const r = await dieses(a);
    return r.netz || name ? { ...r, netz: { ...(r.netz ?? { name, first_seen: t, last_seen: t }), meins } } : r;
  };
  // Karten gibt es erst nach der ersten Suche, und die ist der Erstlauf (nichts „neu“). geraete-leer bleibt, wie es ist.
  let gesucht = false;
  const suchen = VORSCHAU.geraete_suchen;
  VORSCHAU.geraete_suchen = async (a) => {
    if (!meins) throw { art: 'nicht_meins', text: 'Das Netz ist nicht „meins“.' };
    if (!marken.includes('geraete-leer')) marken.push('geraete-erst');
    const r = await suchen(a);
    gesucht = r.geraete.length > 0;
    return r;
  };
  const karten = VORSCHAU.geraete_karten;
  // Nach dem Erstlauf kennt Netzinfo nur die vier gefundenen Geräte, und jedes zum ersten Mal.
  const gefunden = ['192.0.2.1', '192.0.2.20', '192.0.2.9', '192.0.2.10'];
  VORSCHAU.geraete_karten = async (a) => (gesucht ? (await karten(a)).filter((k) => gefunden.includes(k.ip))
    .map((k) => ({ ...k, besuche: 1, first_seen: k.last_seen, ip_verlauf: [], hinweise: [] })) : []);
  const bericht = VORSCHAU.bericht;
  VORSCHAU.bericht = async (a) => { if (!name) throw { art: 'nicht_gemerkt', text: 'Dieses Netz ist nicht gemerkt.' }; return bericht(a); };
})();
