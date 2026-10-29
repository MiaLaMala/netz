// Fake answers for the view „geraete“ in vorschau.html.
// Marks: geraete (not searched yet, shows the stored cards), geraete-lauf (normal run), geraete-erst (first run),
// geraete-leer, geraete-nichtmeins, geraete-fehler, geraete-suche (stays in progress); add ,erklaeren.
// Cards (issue #29): geraete (cards), geraete-gefiltert (filter „Drucker“), geraete-keine (no cards).
(() => {
  const m = (s) => marken.includes(s);
  const T = 1793050200;
  const TAG = 86400;
  const d = (ip, name, ports, erst, zuletzt, mac, ip_verlauf = []) => ({ ip, name, ports, first_seen: T - erst, last_seen: T - zuletzt, mac, ip_verlauf });
  const router = d('192.168.178.1', 'fritz.box', [53, 80, 443], 90 * TAG, 0, '00:00:5e:00:53:01');
  const mac = d('192.168.178.20', 'mias-macbook.fritz.box', [22, 445, 7000], 90 * TAG, 0, '00:00:5e:00:53:20', ['192.168.178.25', '192.168.178.23']);
  const drucker = d('192.168.178.9', 'drucker.fritz.box', [80, 631, 9100], 60 * TAG, 0, '00:00:5e:00:53:09');
  const handy = d('192.168.178.10', null, [], 3 * TAG, 0, '00:00:5e:00:53:10', ['192.168.178.44']);
  const neu = d('192.168.178.42', 'wohnzimmer-tv.fritz.box', [8009, 7000, 9999], 0, 0, '00:00:5e:00:53:42');
  const weg = d('192.168.178.31', 'alter-laptop.fritz.box', [22], 40 * TAG, 5 * TAG, '00:00:5e:00:53:31');
  const nichtMeins = '`netzinfo geraete` läuft nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es gibt dafür bewusst keine Option: Wer in Schule, Firma oder fremdem WLAN Geräte durchprobiert, bekommt schnell Ärger. Es wurde nichts gesendet.';
  const warte = (ms) => new Promise((r) => setTimeout(r, ms));

  VORSCHAU.geraete_suchen = async () => {
    await warte(m('geraete-suche') ? 600000 : 700);
    if (m('geraete-nichtmeins')) throw { art: 'nicht_meins', text: nichtMeins };
    if (m('geraete-fehler')) throw 'Das aktuelle Subnetz ist unbekannt.';
    const r = { bereich: '192.168.178.0/24', adressen: 254, geraete: [router, mac, handy, drucker, neu], neu: [neu], verschwunden: [weg], erster_lauf: false };
    if (m('geraete-leer')) return { ...r, geraete: [], neu: [], verschwunden: [] };
    if (m('geraete-erst')) return { ...r, geraete: [router, mac, drucker, handy], neu: [], verschwunden: [], erster_lauf: true };
    return r;
  };

  // Cards come from the stored file, not from the search.
  const dienst = (typ, instanz, port) => ({ typ, instanz, host: '', port, ipv4: [], ipv6: [], txt: {} });
  const k = (g, typ, symbol, anzeigename, erklaerung, hinweise, extra = {}) => ({ ...g, ipv6: [], ip_verlauf: g.ip_verlauf || [], hersteller: null, mac_zufaellig: false, dienste: [], mdns_name: null, von_mia: null, namen: [], besuche: 5, typ, symbol, anzeigename, namequelle: QUELLEN[anzeigename] || (/^\d/.test(anzeigename) ? '' : 'DNS (PTR)'), erklaerung, hinweise, ...extra });
  const QUELLEN = { 'Wohnzimmer-TV': 'Bonjour', 'Alter Laptop': 'von dir', 'MIAS-PC': 'NetBIOS' };
  const KARTEN = [
    k(router, 'Router', '🌐', 'fritz.box', 'Netzinfo hält dieses Gerät für einen Router, weil es das Standardgateway deines Netzes ist.', [], { hersteller: 'Beispiel Netzwerktechnik GmbH', besuche: 40 }),
    k(neu, 'Medien', '📺', 'wohnzimmer-tv.fritz.box', 'Netzinfo hält dieses Gerät für ein Medien-Gerät (Fernseher oder Lautsprecher), weil es den Bonjour-Dienst AirPlay (_airplay._tcp) anbietet.', ['wurde erst einmal gesehen'], { hersteller: 'Beispiel Elektronik Co.', besuche: 1, dienste: [dienst('_airplay._tcp', 'Wohnzimmer', 7000), dienst('_googlecast._tcp', 'Wohnzimmer-TV', 8009)] }),
    k(mac, 'Computer', '💻', 'mias-macbook.fritz.box', 'Netzinfo hält dieses Gerät für einen Computer, weil es SSH (Port 22) anbietet.', ['hatte früher 2 andere IP-Adresse(n)'], { hersteller: 'Beispiel Computer Inc.', besuche: 31, dienste: [dienst('_ssh._tcp', 'mias-macbook', 22), dienst('_smb._tcp', 'mias-macbook', 445)] }),
    k(drucker, 'Drucker', '🖨️', 'drucker.fritz.box', 'Netzinfo hält dieses Gerät für einen Drucker, weil es den Bonjour-Dienst für Drucken (_ipp._tcp) anbietet.', [], { hersteller: 'Beispiel Druck AG', besuche: 22, dienste: [dienst('_ipp._tcp', 'Flur-Drucker', 631)] }),
    k(handy, 'Unbekannt', '❔', '192.168.178.10', 'Dieses Gerät kennt Netzinfo noch nicht. Du kannst ihm einen Namen geben.', ['nutzt eine zufällige MAC-Adresse', 'hatte früher 1 andere IP-Adresse(n)'], { mac_zufaellig: true, besuche: 3 }),
    k(weg, 'Computer', '💻', 'Alter Laptop', 'Du hast festgelegt, dass dies ein Gerät vom Typ „Computer“ ist. Neue Läufe ändern das nicht.', ['zuletzt vor 5 Tagen gesehen'], { von_mia: { name: 'Alter Laptop', typ: 'Computer' }, besuche: 12 }),
  ].sort((a, b) => b.last_seen - a.last_seen);
  VORSCHAU.geraete_karten = async () => (m('geraete-keine') ? [] : KARTEN);
  VORSCHAU.geraet_setzen = async () => {};
  // UPnP (issue #42): model and type of the TV, as `netzinfo upnp` stores them.
  KARTEN.find((x) => x.ip === neu.ip).upnp = { udn: 'uuid:0000-example', server: null, geraetetyp: 'urn:schemas-upnp-org:device:MediaRenderer:1', hersteller: 'Beispiel Elektronik Co.', modell: 'Bildschirm 55', name: 'Wohnzimmer-TV', location_pfad: '/desc.xml' };

  // vorschau.html only knows the mark „geraete“; the other marks open the tab here. Search marks press „Suchen“.
  const suchmarken = ['lauf', 'erst', 'leer', 'nichtmeins', 'fehler', 'suche'].map((x) => `geraete-${x}`);
  if (marken.some((s) => s.startsWith('geraete'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-geraete')?.click();
      if (marken.some((s) => suchmarken.includes(s))) setTimeout(() => document.querySelector('.aktion button')?.click(), 250);
      if (m('geraete-gefiltert')) setTimeout(() => [...document.querySelectorAll('.chip')].find((c) => c.textContent.startsWith('Drucker'))?.click(), 400);
    }, 150));
  }
})();
