// Beispielbereiche statt privater Adressen (gilt für alle Ansichten gleich): Router 192.0.2.1, Mac 192.0.2.20, VPN 198.51.100.7.
// Randfall-Marken (Runde 4): nur-ipv6 (kein IPv4, kein IPv4-Router), lang (Netzname mit 60 Zeichen, SSID mit 32 Byte samt Emoji, lange DNS-Namen und
// Zonen-ID), ssid-leer, ssid-rot (macOS liefert „<redacted>“), leer (Listen null, Felder leer), ipv6-haengt und proxy-haengt (Befehl antwortet nie),
// ruhezustand (Antwort der Prüfung ist Stunden alt, wenn man in die Übersicht zurückkehrt; body[data-pruefungen] zählt die Messungen).
// Die Grunddaten stehen in vorschau.html (Gerüst), hier werden sie nur umgeschrieben.
(() => {
  const grund = VORSCHAU.dieses_netz;
  // Nach „Merken“ ist das Netz in der Vorschau bekannt (zeigt die neue Überschrift samt Fokus).
  const merken = VORSCHAU.netz_merken;
  let gemerkt = null;
  VORSCHAU.netz_merken = async (a) => { await merken(a); gemerkt = a.name.trim(); };
  VORSCHAU.dieses_netz = async () => JSON.parse(JSON.stringify(await grund())
    .replaceAll('192.168.178.', '192.0.2.').replaceAll('100.64.0.', '198.51.100.').replaceAll('fd7a:115c:a1e0::53', '2001:db8::53'));
  const offen = VORSCHAU.dieses_netz;
  VORSCHAU.dieses_netz = async () => {
    const d = await offen();
    return gemerkt && !d.netz ? { ...d, netz: { name: gemerkt, meins: false, last_seen: t } } : d;
  };
  // Marke hinweis: die Liste gemerkter Netze ist nicht lesbar (Hinweis oben, Netz bleibt unbekannt).
  if (marken.includes('hinweis')) {
    const roh = VORSCHAU.dieses_netz;
    VORSCHAU.dieses_netz = async () => ({ ...(await roh()), hinweis: 'Die Liste der gemerkten Netze ließ sich nicht lesen. Dieses Netz kann gerade nicht wiedererkannt werden.' });
  }
})();
// Fake answers for the WLAN card of the view „dieses-netz“ in vorschau.html.
// Marks: wlan-gut, wlan-schwach; without a mark (cable) the card stays hidden.
(() => {
  const gut = { standard: 'Wi-Fi 6 (802.11ax)', band: '5 GHz', kanal: 36, breite_mhz: 80, rssi_dbm: -52, rauschen_dbm: -92, snr_db: 40, rate_mbit: 1201, sicherheit: 'Wpa3', land: 'DE' };
  const schwach = { standard: 'Wi-Fi 4 (802.11n)', band: '2,4 GHz', kanal: 6, breite_mhz: 20, rssi_dbm: -83, rauschen_dbm: null, snr_db: null, rate_mbit: 24, sicherheit: 'Wpa', land: 'DE' };
  // Standort: ort-erlaubt (mit SSID), ort-abgelehnt, ort-eingeschraenkt; ohne Marke „nicht gefragt“.
  const mitName = { ...gut, ssid: 'Beispiel-WLAN', bssid: '00:00:5e:00:53:01' };
  VORSCHAU.wlan_lesen = async () => (marken.includes('ort-erlaubt') ? mitName : marken.includes('wlan-gut') ? gut : marken.includes('wlan-schwach') ? schwach : null);
  VORSCHAU.ort_status = async () => (marken.includes('ort-erlaubt') ? 'Erlaubt' : marken.includes('ort-abgelehnt') ? 'Abgelehnt' : marken.includes('ort-eingeschraenkt') ? 'Eingeschraenkt' : 'NichtGefragt');
  VORSCHAU.ort_anfragen = async () => 'NichtGefragt';
  VORSCHAU.ort_einstellungen_oeffnen = async () => null;
})();
// Verschlüsseltes DNS (#88). Marken: dns-frisch (gespeicherte Messung von heute), dns-alt (drei Tage alt,
// wird neu gemessen); ohne Marke wie bisher. Der Zähler der Messungen steht in body[data-messungen].
(() => {
  addEventListener('DOMContentLoaded', () => { document.body.dataset.messungen ??= '0'; });
  let messungen = 0;
  const messen = VORSCHAU.dns_verschluesselt;
  VORSCHAU.dns_verschluesselt = async (a) => {
    document.body.setAttribute('data-messungen', String(++messungen));
    // Die Neumessung braucht in der Vorschau etwas Zeit, damit der Zwischenzustand sichtbar bleibt.
    if (marken.includes('dns-alt-laeuft')) await new Promise(() => {});
    return messen(a);
  };
  const gespeichert = { dot: 'Erreichbar', doh: 'Erreichbar', ddr: null };
  VORSCHAU.dns_gespeichert = async () => (marken.includes('dns-frisch')
    ? { messung: { ...gespeichert, zeit: t - 3600 * 5 }, frisch: true }
    : marken.includes('dns-alt') || marken.includes('dns-alt-laeuft')
      ? { messung: { ...gespeichert, zeit: t - 86400 * 3 }, frisch: false }
      : null);
})();
// Proxy und WPAD (#53). Marken: proxy-pac (PAC-Datei und WPAD), proxy-pac-offen (Hostnamen schon geladen);
// ohne Marke „Kein Proxy“.
(() => {
  const pac = marken.includes('proxy-pac') || marken.includes('proxy-pac-offen');
  VORSCHAU.proxy_lesen = async () => (pac
    ? { manuell: null, pac_url: 'http://wpad.example.net/proxy.pac', wpad_dhcp: 'http://wpad.example.net/wpad.dat', wpad_dns: true, auto_suche: true }
    : { manuell: null, pac_url: null, wpad_dhcp: null, wpad_dns: false, auto_suche: false });
  VORSCHAU.proxy_pac_ansehen = async () => ['proxy1.example.net', 'proxy2.example.net'];
  if (marken.includes('proxy-pac-offen')) addEventListener('load', () => setTimeout(() => document.querySelector('[data-ansicht=\'dieses-netz\'] button[data-pac]')?.click(), 300));
})();

// IPv6-Router (#68). Marken: v6-keiner, v6-mehrere, v6-fremd; ohne Marke ein Router, der zum Gateway passt.
(() => {
  VORSCHAU.ipv6_router = async () => (marken.includes('v6-keiner')
    ? { anzahl: 0, passt_zu_gateway: null, text: 'kein IPv6-Router, dieses Netz nutzt wohl kein IPv6' }
    : marken.includes('v6-fremd')
      ? { anzahl: 1, passt_zu_gateway: false, text: 'ein Router, passt nicht zum Gateway' }
      : marken.includes('v6-mehrere')
      ? { anzahl: 2, passt_zu_gateway: false, text: '2 Router, passt nicht zum Gateway' }
      : { anzahl: 1, passt_zu_gateway: true, text: 'ein Router, passt zum Gateway' });
})();

// Übersicht (#208). Marken: tab-uebersicht plus eine Lage: pruefung-ok, pruefung-alarm, pruefung-erst, pruefung-laeuft, pruefung-fehler,
// portal, sni-filter (ohne Marke: Netz bekannt mit Unterschieden nur mit `bekannt`), vpn, ohne wlan-Marke Kabel, ohne `bekannt` ein neues Netz.
// `details` klappt „Details“ auf, `erklaeren` schaltet den Erklärmodus ein.
(() => {
  if (marken.includes('details')) addEventListener('load', () => setTimeout(() => { const d = document.querySelector('[data-ansicht=\'uebersicht\'] details.details'); if (d) d.open = true; }, 1800));
})();
// Bei pruefung-alarm sieht der Mac dieselben Werte wie die Prüfung: anderer DHCP-Server, anderer DNS.
if (marken.includes('pruefung-alarm')) {
  const roh = VORSCHAU.dieses_netz;
  VORSCHAU.dieses_netz = async () => {
    const d = await roh();
    return { ...d, info: { ...d.info, dhcp_server: '192.0.2.77', dhcp_dns: ['198.51.100.53'], system_dns: ['198.51.100.53', '2001:db8::53'] } };
  };
}

// Randfälle (Runde 4): die Namen und Felder, an denen eine Ansicht bricht.
(() => {
  const NETZNAME = 'Wohnheim-Mitte-Haus-3-Etage-2-Zimmer-17-Gaeste-Besucher'; // 55 Zeichen ohne Leerstelle
  const SSID = 'Seeblick Gäste 🏨🌊📶 WLAN'; // 33 Byte in UTF-8
  const aendere = (f) => { const vor = VORSCHAU.dieses_netz; VORSCHAU.dieses_netz = async () => f(await vor()); };
  if (marken.includes('nur-ipv6')) aendere((d) => ({ ...d, info: { ...d.info, ip: null, subnet: null, router: null, dhcp_server: null, dhcp_dns: [], domain: null, system_dns: ['2001:db8::53'] } }));
  if (marken.includes('lang')) {
    aendere((d) => ({ ...d, netz: d.netz && { ...d.netz, name: NETZNAME },
      info: { ...d.info, system_dns: ['fe80::1%en0', 'resolver-mit-sehr-langem-namen-fuer-den-test.dns.example', '2001:db8::53'] } }));
    const wlan = VORSCHAU.wlan_lesen;
    VORSCHAU.wlan_lesen = async () => { const w = await wlan(); return w && { ...w, ssid: SSID, bssid: '00:00:5e:00:53:01' }; };
  }
  for (const [marke, ssid] of [['ssid-leer', ''], ['ssid-rot', '<redacted>']]) {
    if (!marken.includes(marke)) continue;
    const wlan = VORSCHAU.wlan_lesen;
    VORSCHAU.wlan_lesen = async () => { const w = await wlan(); return w && { ...w, ssid, bssid: ssid }; };
  }
  if (marken.includes('leer')) {
    aendere((d) => ({ ...d, netz: d.netz && { ...d.netz, name: '' }, info: { ...d.info, interface: null, dhcp_dns: null, system_dns: null, domain: '' } }));
    const wlan = VORSCHAU.wlan_lesen;
    VORSCHAU.wlan_lesen = async () => { const w = await wlan(); return w && { ...w, rssi_dbm: null, sicherheit: null, standard: '' }; };
    VORSCHAU.dns_gespeichert = async () => ({ messung: { dot: null, doh: null, ddr: null, zeit: null }, frisch: true });
  }
  if (marken.includes('ipv6-haengt')) VORSCHAU.ipv6_router = () => new Promise(() => {});
  if (marken.includes('proxy-haengt')) VORSCHAU.proxy_lesen = () => new Promise(() => {});
})();
