// Fake answers for the view „geraete“ in vorschau.html.
// Marks: geraete (not searched yet, shows the stored cards), geraete-lauf (normal run), geraete-erst (first run),
// geraete-leer, geraete-nichtmeins, geraete-fehler, geraete-suche (stays in progress); add ,erklaeren.
// Port mappings (#45, in the router card): geraete-freigaben (list), geraete-freigaben-aus (UPnP off),
// geraete-freigaben-login (list only after login), geraete-freigaben-nichtmeins.
// Fritz!Box-Geräteliste (#65, in der Router-Karte): ohne Marke „Zugang nötig“ (Zeile mit Knopf),
// geraete-fritz-formular (Formular offen), geraete-fritz (Zugang da, noch nicht gelesen),
// geraete-fritz-liste (gelesen, Karten mit „vom Router“), geraete-fritz-falsch (401), geraete-fritz-aus (TR-064 aus),
// geraete-fritz-weg (nicht erreichbar).
// Robustheit (#176): geraete-viele (250 Karten, 10 davon IPv6, gemischt geliefert: sortiert wird in der Ansicht),
// geraete-gross (Netz ist ein /16: Suche startet nicht, Hinweis), geraete-randfaelle (ohne Namen, ohne Hersteller, Namen mit 63 Zeichen,
// Emoji, Umlaute, doppelte Namen, nur IPv6, Typ „constructor“, fehlende Felder), geraete-haengt (Suche kommt nie an, Grenze nach 1,5 s),
// geraete-fritz-haengt (Router antwortet nie), geraete-fritz-unbekannt (Zustand, den die Ansicht nicht kennt).
// Cards (issue #29): geraete (cards), geraete-gefiltert (filter „Drucker“), geraete-keine (no cards), geraete-name (edit form of the first card).
(() => {
  const m = (s) => marken.includes(s);
  const T = 1793050200;
  const TAG = 86400;
  const d = (ip, name, ports, erst, zuletzt, mac, ip_verlauf = []) => ({ ip, name, ports, first_seen: T - erst, last_seen: T - zuletzt, mac, ip_verlauf });
  const router = d('192.0.2.1', 'fritz.box', [53, 80, 443], 90 * TAG, 0, '00:00:5e:00:53:01');
  const mac = d('192.0.2.20', 'macbook.fritz.box', [22, 445, 7000], 90 * TAG, 0, '00:00:5e:00:53:20', ['192.0.2.25', '192.0.2.24']);
  const drucker = d('192.0.2.9', 'drucker.fritz.box', [80, 445, 631, 9100], 60 * TAG, 0, '00:00:5e:00:53:09');
  const handy = d('192.0.2.10', null, [], 3 * TAG, 0, '00:00:5e:00:53:10', ['192.0.2.44']);
  const neu = d('192.0.2.23', 'wohnzimmer-tv.fritz.box', [8009, 7000, 9999], 0, 0, '00:00:5e:00:53:42');
  const weg = d('192.0.2.31', 'alter-laptop.fritz.box', [22], 40 * TAG, 5 * TAG, '00:00:5e:00:53:31');
  const nichtMeins = '`netzinfo geraete` läuft nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es gibt dafür bewusst keine Option: Wer in Schule, Firma oder fremdem WLAN Geräte durchprobiert, bekommt schnell Ärger. Es wurde nichts gesendet.';
  const warte = (ms) => new Promise((r) => setTimeout(r, ms));

  VORSCHAU.geraete_suchen = async () => {
    await warte(m('geraete-suche') || m('geraete-haengt') ? 600000 : 700);
    if (m('geraete-nichtmeins')) throw { art: 'nicht_meins', text: nichtMeins };
    if (m('geraete-fehler')) throw 'Das Subnetz deines Netzes ist unbekannt. Verbinde dich mit einem Netz und versuche es noch einmal.';
    const r = { bereich: '192.0.2.0/24', adressen: 254, geraete: [router, mac, handy, drucker, neu], neu: [neu], verschwunden: [weg], erster_lauf: false };
    if (m('geraete-leer')) return { ...r, geraete: [], neu: [], verschwunden: [] };
    if (m('geraete-erst')) return { ...r, geraete: [router, mac, drucker, handy], neu: [], verschwunden: [], erster_lauf: true };
    return r;
  };

  // Cards come from the stored file, not from the search.
  const dienst = (typ, instanz, port) => ({ typ, instanz, host: '', port, ipv4: [], ipv6: [], txt: {} });
  const k = (g, typ, symbol, anzeigename, erklaerung, hinweise, extra = {}) => ({ ...g, ipv6: [], ip_verlauf: g.ip_verlauf || [], hersteller: null, mac_zufaellig: false, dienste: [], mdns_name: null, von_mia: null, namen: [], router: null, besuche: 5, typ, symbol, anzeigename, namequelle: QUELLEN[anzeigename] || (/^\d/.test(anzeigename) ? '' : 'DNS (PTR)'), erklaerung, hinweise, ...extra });
  const QUELLEN = { 'Wohnzimmer-TV': 'Bonjour', 'Alter Laptop': 'von dir', 'BEISPIEL-PC': 'NetBIOS' };
  const KARTEN = [
    k(router, 'Router', '🌐', 'fritz.box', 'Netzinfo hält dieses Gerät für einen Router, weil es das Standardgateway deines Netzes ist.', [], { hersteller: 'Beispiel Netzwerktechnik GmbH', besuche: 40, gateway: true }),
    k(neu, 'Medien', '📺', 'wohnzimmer-tv.fritz.box', 'Netzinfo hält dieses Gerät für ein Medien-Gerät (Fernseher oder Lautsprecher), weil es den Bonjour-Dienst AirPlay (_airplay._tcp) anbietet.', ['wurde erst einmal gesehen'], { hersteller: 'Beispiel Elektronik Co.', besuche: 1, dienste: [dienst('_airplay._tcp', 'Wohnzimmer-TV', 7000), dienst('_googlecast._tcp', 'Wohnzimmer-TV', 8009)] }),
    k(mac, 'Computer', '💻', 'macbook.fritz.box', 'Netzinfo hält dieses Gerät für einen Computer, weil es SSH (Port 22) anbietet.', ['hatte früher 2 andere IP-Adresse(n)'], { hersteller: 'Beispiel Computer Inc.', besuche: 31, dienste: [dienst('_ssh._tcp', 'macbook', 22), dienst('_smb._tcp', 'macbook', 445)] }),
    k(drucker, 'Drucker', '🖨️', 'drucker.fritz.box', 'Netzinfo hält dieses Gerät für einen Drucker, weil es den Bonjour-Dienst für Drucken (_ipp._tcp) anbietet.', [], { hersteller: 'Beispiel Druck AG', besuche: 22, dienste: [dienst('_ipp._tcp', 'Flur-Drucker', 631)] }),
    k(handy, 'Unbekannt', '❔', '192.0.2.10', 'Dieses Gerät kennt Netzinfo noch nicht. Du kannst ihm einen Namen geben.', ['nutzt eine zufällige MAC-Adresse', 'hatte früher 1 andere IP-Adresse(n)'], { mac_zufaellig: true, besuche: 3 }),
    k(weg, 'Computer', '💻', 'Alter Laptop', 'Du hast festgelegt, dass dies ein Gerät vom Typ „Computer“ ist. Neue Läufe ändern das nicht.', ['zuletzt vor 5 Tagen gesehen'], { von_mia: { name: 'Alter Laptop', typ: 'Computer' }, besuche: 12 }),
  ].sort((a, b) => b.last_seen - a.last_seen);
  // Router-Liste (#65): drei Geräte kennt nur die Fritz!Box.
  QUELLEN['nas-keller'] = QUELLEN['steckdose-flur'] = QUELLEN['thermostat-bad'] = 'Router';
  const router_ = (verbindung, nur_router = false, aktiv = true) => ({ verbindung, aktiv, zuletzt: T, nur_router });
  const nurRouter = (ip, mac, name, verbindung) => k(d(ip, null, [], 0, 0, mac), 'Unbekannt', '❔', name, 'Dieses Gerät kennt Netzinfo noch nicht. Du kannst ihm einen Namen geben.', [], { besuche: 1, namen: [{ quelle: 'router', name, zuletzt: T }], router: router_(verbindung, true) });
  const MIT_ROUTER = [
    ...KARTEN.map((x) => ({ ...x, router: { 'macbook.fritz.box': router_('WLAN'), 'drucker.fritz.box': router_('LAN'), 'wohnzimmer-tv.fritz.box': router_('WLAN') }[x.anzeigename] ?? null })),
    nurRouter('192.0.2.40', '00:00:5e:00:53:40', 'nas-keller', 'LAN'),
    nurRouter('192.0.2.50', '00:00:5e:00:53:50', 'steckdose-flur', 'WLAN'),
    nurRouter('192.0.2.52', '00:00:5e:00:53:52', 'thermostat-bad', 'WLAN'),
  ].sort((a, b) => b.last_seen - a.last_seen);
  let zugang = m('geraete-fritz-haengt') || m('geraete-fritz-unbekannt') || m('einst-fritz') || m('einst-fritz-entfernen') || m('geraete-fritz') || m('geraete-fritz-liste') || m('geraete-fritz-falsch') || m('geraete-fritz-aus') || m('geraete-fritz-weg') ? 'netzinfo' : null;
  let gelesen = false;
  VORSCHAU.fritzbox_zugang = async () => zugang;
  VORSCHAU.fritzbox_speichern = async ({ benutzer }) => {
    if (m('einst-fritz-fehler')) throw 'Die Fritz!Box lehnt Benutzer oder Passwort ab. Prüfe sie unter System, Fritz!Box-Benutzer.';
    zugang = benutzer.trim() || 'netzinfo';
  };
  VORSCHAU.fritzbox_entfernen = async () => { zugang = null; gelesen = false; };
  VORSCHAU.fritzbox_lesen = async () => {
    if (m('geraete-fritz-haengt')) await warte(600000);
    await warte(600);
    if (m('geraete-fritz-unbekannt')) return { zustand: 'constructor', satz: '', gefunden: 0, neu: 0 };
    const aus = (zustand, satz) => ({ zustand, satz, gefunden: 0, neu: 0 });
    if (m('geraete-fritz-falsch')) return aus('zugang_falsch', 'Die Fritz!Box lehnt Benutzer oder Passwort ab. Prüfe sie unter System, Fritz!Box-Benutzer, oder richte den Zugang neu ein.');
    if (m('geraete-fritz-aus')) return aus('tr064_aus', 'TR-064 ist in der Fritz!Box aus. Schalte es ein unter Heimnetz, Netzwerk, Netzwerkeinstellungen, Heimnetzfreigaben: „Zugriff für Anwendungen zulassen“.');
    if (m('geraete-fritz-weg')) return aus('nicht_erreichbar', 'Die Fritz!Box 192.0.2.1 antwortet nicht innerhalb von 3 s. Prüfe die Verbindung und die Rechte für das lokale Netzwerk.');
    gelesen = true;
    return { zustand: 'ok', satz: '', gefunden: 6, neu: 3 };
  };
  // Robustheit: viele Karten und Randfälle.
  const lang = 'laserjet-etagendrucker-obergeschoss-links-hinten-neben-dem-archiv'.slice(0, 63);
  const RAND = [
    k(d('192.0.2.70', null, [], 3 * TAG, 0, '00:00:5e:00:53:70'), 'Unbekannt', '❔', '192.0.2.70', 'Dieses Gerät kennt Netzinfo noch nicht. Du kannst ihm einen Namen geben.', [], { besuche: 2 }),
    k(d('192.0.2.71', null, [], 0, 0, '00:00:5e:00:53:71'), 'Unbekannt', '❔', '', '', [], { hersteller: 'Beispiel Unterhaltungselektronik und Netzwerktechnik Gesellschaft mit beschränkter Haftung International', hinweise: undefined, dienste: undefined, besuche: 1 }),
    k(d('192.0.2.72', lang, [631], 9 * TAG, 0, '00:00:5e:00:53:72'), 'Drucker', '🖨️', lang, 'Netzinfo hält dieses Gerät für einen Drucker, weil Port 631 offen ist.', [], { hersteller: 'Beispiel Druck AG', namequelle: 'DNS (PTR)' }),
    k(d('192.0.2.73', null, [], 5 * TAG, 0, '00:00:5e:00:53:73'), 'Medien', '📺', 'Müllers Fernseher 📺 Wohnzimmer', 'Du hast festgelegt, dass dies ein Gerät vom Typ „Medien“ ist.', [], { von_mia: { name: 'Müllers Fernseher 📺 Wohnzimmer', typ: 'Medien' }, namequelle: 'von dir' }),
    k(d('192.0.2.74', 'wohnzimmer-tv.example', [], 5 * TAG, 0, '00:00:5e:00:53:74'), 'Medien', '📺', 'Wohnzimmer-TV', 'Netzinfo hält dieses Gerät für ein Medien-Gerät.', [], { namequelle: 'Bonjour' }),
    k(d('192.0.2.75', 'wohnzimmer-tv.example', [], 5 * TAG, 0, '00:00:5e:00:53:75'), 'Medien', '📺', 'Wohnzimmer-TV', 'Netzinfo hält dieses Gerät für ein Medien-Gerät.', [], { namequelle: 'Bonjour' }),
    k(d('192.0.2.76', null, [], 2 * TAG, 0, '02:00:5e:00:53:76'), 'Handy', '📱', 'Bücherregal-Lautsprecher 🔊', 'Netzinfo hält dieses Gerät für ein Handy.', ['nutzt eine zufällige MAC-Adresse'], { mac_zufaellig: true, namequelle: 'Bonjour' }),
    k(d('2001:db8::1a2b', null, [], 1 * TAG, 0, '00:00:5e:00:53:77'), 'Smart Home', '🏠', 'sensor-flur.example', 'Netzinfo hält dieses Gerät für Smart Home.', [], { namequelle: 'DNS (PTR)' }),
    k(d('2001:db8::2', null, [], 1 * TAG, 0, '00:00:5e:00:53:78'), 'Computer', '💻', '2001:db8::2', 'Dieses Gerät kennt Netzinfo noch nicht.', [], {}),
    k(d('192.0.2.79', null, [], 1 * TAG, 0, '00:00:5e:00:53:79'), 'constructor', '❔', 'Typ aus fremder Quelle', 'Dieses Gerät kennt Netzinfo noch nicht.', [], { upnp: { udn: '', server: null, geraetetyp: null, hersteller: null, modell: null, name: null, location_pfad: null } }),
  ];
  const VIELE = Array.from({ length: 250 }, (_, i) => {
    const n = (i * 37) % 250 + 1; // gemischt, jede Adresse einmal
    const v6 = n > 240;
    const ip = v6 ? `2001:db8::${n.toString(16)}` : `192.0.2.${n}`;
    const typ = n === 1 ? 'Router' : ['Computer', 'Handy', 'Medien', 'Smart Home', 'Drucker'][n % 5];
    return k(d(ip, `geraet-${n}.example`, n % 7 ? [] : [22], 20 * TAG, 0, `00:00:5e:00:${(n >> 8).toString(16).padStart(2, '0')}:${(n & 255).toString(16).padStart(2, '0')}`), typ, '❔', `geraet-${n}.example`, `Netzinfo hält dieses Gerät für ${typ}.`, [], { besuche: n % 9 + 1, gateway: n === 1 });
  });
  VORSCHAU.geraete_karten = async () => (m('geraete-viele') ? VIELE : m('geraete-randfaelle') ? RAND : m('geraete-keine') ? [] : m('geraete-fritz-liste') || gelesen ? MIT_ROUTER : KARTEN);
  // „Netz ist ein /16“: nur die Grösse des Netzes ändert sich, der Rest kommt aus der Vorschau von dieses-netz.js.
  const dieses = VORSCHAU.dieses_netz;
  if (m('geraete-gross') && dieses) VORSCHAU.dieses_netz = async (a) => { const r = await dieses(a); return { ...r, info: { ...r.info, subnet: { ...r.info.subnet, network: '192.0.0.0', prefix: 16 } } }; };
  VORSCHAU.geraet_setzen = async () => {};
  // UPnP (issue #42): model and type of the TV, as `netzinfo upnp` stores them.
  KARTEN.find((x) => x.ip === neu.ip).upnp = { udn: 'uuid:0000-example', server: null, geraetetyp: 'urn:schemas-upnp-org:device:MediaRenderer:1', hersteller: 'Beispiel Elektronik Co.', modell: 'Bildschirm 55', name: 'Wohnzimmer-TV', location_pfad: '/desc.xml' };

  // Router-Seite (#67): geraete-router (selbst ausgestellt, Klartext), geraete-router-umleitung, geraete-router-nichtmeins.
  VORSCHAU.router_seite = async () => {
    await warte(500);
    if (m('geraete-router-nichtmeins')) throw { art: 'nicht_meins', text: nichtMeins.replace('geraete', 'router') };
    const umleitung = m('geraete-router-umleitung');
    return {
      gateway: router.ip,
      hinweise: umleitung ? [] : ['Die Router-Seite ist über http ohne Verschlüsselung erreichbar und führt nicht auf https. Wer im Heimnetz mitlesen kann, sieht dort eingegebene Passwörter.'],
      seite: { http: 'offen', https: 'offen', umleitung, server: 'Beispiel-Router', titel: 'Beispiel Box',
        zertifikat: { selbstsigniert: true, abgelaufen: false, gueltig_bis: '2033-05-14', aussteller: 'CN=box.example' } },
    };
  };

  // Portfreigaben (#45): `netzinfo portfreigaben` as data. Example mappings point at 192.0.2.x.
  const frei = (extern, intern_ip, intern_port, beschreibung, aktiv = true) => ({ extern, intern_ip, intern_port, protokoll: 'TCP', beschreibung, aktiv, zuerst_gesehen: T - 2 * TAG });
  VORSCHAU.portfreigaben = async () => {
    await warte(500);
    if (m('geraete-freigaben-nichtmeins')) throw { art: 'nicht_meins', text: nichtMeins.replace('geraete', 'portfreigaben') };
    const a = { gateway: router.ip, upnp: true, verbindung: 'Verbunden', externe_ip_klasse: 'öffentlich', freigaben: [], liste_nur_mit_login: false, listenfehler: null, befunde: [] };
    if (m('geraete-freigaben-aus')) return { ...a, upnp: false, verbindung: '', externe_ip_klasse: '' };
    if (m('geraete-freigaben-login')) return { ...a, liste_nur_mit_login: true };
    return { ...a, freigaben: [frei(3074, '192.0.2.60', 3074, 'Spielkonsole'), frei(22, mac.ip, 22, 'Fernzugriff')],
      befunde: [[true, 'Port 22 (SSH) ist aus dem Internet erreichbar und zeigt auf 192.0.2.20.'], [false, 'Neu seit dem letzten Besuch: 3074 (Spielkonsole).']] };
  };

  // vorschau.html only knows the mark „geraete“; the other marks open the tab here. Search marks press „Suchen“.
  const routermarken = ['router', 'router-umleitung', 'router-nichtmeins'].map((x) => `geraete-${x}`);
  const freimarken = ['freigaben', 'freigaben-aus', 'freigaben-login', 'freigaben-nichtmeins'].map((x) => `geraete-${x}`);
  const fritzmarken = ['liste', 'falsch', 'aus', 'weg', 'haengt', 'unbekannt'].map((x) => `geraete-fritz-${x}`);
  const suchmarken = ['lauf', 'erst', 'leer', 'nichtmeins', 'fehler', 'suche', 'haengt', 'gross'].map((x) => `geraete-${x}`);
  // Die Ansicht sucht nur in „meins“-Netzen und sagt das vorher: Alle Marken außer erster-start spielen in einem Netz, das „meins“ ist.
  if (marken.some((s) => s.startsWith('geraete')) && !marken.includes('erster-start')) {
    const roh = VORSCHAU.dieses_netz;
    VORSCHAU.dieses_netz = async (a) => { const r = await roh(a); return { ...r, netz: { ...(r.netz ?? netze[0]), meins: true } }; };
  }
  if (marken.some((s) => s.startsWith('geraete'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-geraete')?.click();
      Netz.geraeteLimit.suche = Netz.geraeteLimit.router = 1500; // nur die Vorschau wartet nicht drei Minuten
      if (marken.some((s) => suchmarken.includes(s))) setTimeout(() => document.querySelector('.aktion button')?.click(), 250);
      if (marken.some((s) => routermarken.includes(s))) setTimeout(() => [...document.querySelectorAll('.router-knoepfe button')][0]?.click(), 400);
      if (marken.some((s) => freimarken.includes(s))) setTimeout(() => [...document.querySelectorAll('.router-knoepfe button')][1]?.click(), 400);
      if (m('geraete-fritz-formular')) setTimeout(() => document.querySelector('.fritz-einrichten')?.click(), 500);
      if (marken.some((s) => fritzmarken.includes(s))) setTimeout(() => document.querySelector('.fritz-lesen')?.click(), 500);
      if (m('geraete-name')) setTimeout(() => document.querySelector('.karte .knopf.leise')?.click(), 400);
      if (m('geraete-gefiltert')) setTimeout(() => [...document.querySelectorAll('.chip')].find((c) => c.textContent.startsWith('Drucker'))?.click(), 400);
    }, 150));
  }
})();
