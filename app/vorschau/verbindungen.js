// Fake answers for the view „verbindungen“ in vorschau.html.
// Marks: verbindungen (grouped list), verbindungen-traffic / verbindungen-trafficfehler (Messung läuft / scheitert), verbindungen-neu (filter „nur neue“ on), verbindungen-leer, verbindungen-ohnedb (no database), verbindungen-keinnetz (Netz nicht gemerkt), verbindungen-fehler; lauschend (Segment „Lauschend“ gewählt, die Liste dort kommt aus vorschau/offen.js, dessen Marken offen-netz, offen-aussen usw. dazu passen); add ,erklaeren.
// Runde 4 (Robustheit): verbindungen-viele (2000 Verbindungen, Renderzeit im Seitentitel), verbindungen-viele-mehr (nach „Weitere zeigen“), verbindungen-extrem (80 Zeichen, Klammern, ohne Namen, IPv6 mit Zonen-ID, Port 0 und 65535, 10 GB/s), verbindungen-kaputt (Felder fehlen oder null),
// verbindungen-segment-kaputt (gemerkter Wert ist Unsinn), verbindungen-speicher-wirft (localStorage wirft), verbindungen-traffichaengt (Messung antwortet nie, Zeitlimit greift); alle auch mit lauschend, dann zeigt offen.js sein Gegenstück.
(() => {
  const m = (s) => marken.includes(s);
  const v = (ziel, ziel_port, dienst, art, name, quelle, neu, land = null, anbieter = null) => ({
    protokoll: 'TCP', lokal: '192.0.2.20:50123', ziel, ziel_port, dienst, zustand: 'ESTABLISHED', pid: 4242,
    programm: '', pfad: null, art, name, name_quelle: quelle, neu,
    land, asn: anbieter ? 64496 : null, anbieter,
  });
  const g = (programm, vs) => ({ programm, neu: vs.filter((x) => x.neu).length, verbindungen: vs.map((x) => ({ ...x, programm })) });
  const gruppen = [
    g('Safari', [
      v('203.0.113.10', 443, 'HTTPS', 'internet', 'server-1.cdn.example', 'PTR', false, 'Irland', 'BEISPIEL-CLOUD'),
      v('203.0.113.11', 443, 'HTTPS', 'internet', null, null, true, 'Irland', 'BEISPIEL-CLOUD'),
      v('2001:db8::9', 443, 'HTTPS', 'internet', 'www.beispiel.example', 'PTR', false, 'Deutschland', 'BEISPIEL-HOSTING'),
    ]),
    g('Mail', [
      v('203.0.113.40', 993, 'IMAPS', 'internet', 'imap.beispiel.example', 'PTR', false, 'Deutschland', 'BEISPIEL-HOSTING'),
      v('203.0.113.41', 5223, 'Apple Push', 'internet', null, null, true, 'USA', 'BEISPIEL-NETZ'),
    ]),
    g('Code Helper (Renderer)', [v('203.0.113.77', 443, 'HTTPS', 'internet', 'ec2-203-0-113-77.eu-west-1.compute.beispiel-cloud.example', 'PTR', false, 'Irland', 'BEISPIEL-CLOUD')]),
    g('Backup', [v('198.51.100.40', 443, 'HTTPS', 'internet', 'speicher.beispiel.example', 'PTR', false, 'USA', 'BEISPIEL-CLOUD')]),
    g('Systemeinstellungen', [v('192.0.2.9', 631, 'IPP', 'netz', 'drucker.fritz.box', 'Gerät', false)]),
  ];
  window.langerName = 'Code-Helper-Renderer-Prozess-fuer-eine-sehr-lange-Erweiterung-mit-Namen-ohne-Leerzeichen-xxxxxxxxxxxx'.slice(0, 80);
  const extrem = [
    g(langerName, [v('203.0.113.77', 443, 'HTTPS', 'internet', 'ec2-203-0-113-77.eu-west-1.compute.beispiel-cloud.example.fuer-einen-sehr-langen-hostnamen.example', 'PTR', true, 'Irland', 'BEISPIEL-CLOUD')]),
    g('Google Chrome Helper (Renderer) [Profil 2]', [
      v('fe80::1ff:fe23:4567:890a%en0', 0, '', 'netz', null, null, false),
      v('2001:db8:85a3:8d3:1319:8a2e:370:7348', 65535, '', 'internet', null, null, true, 'Deutschland', 'BEISPIEL-HOSTING'),
    ]),
    // Prozess schon beendet: kein Name mehr.
    { programm: '', neu: 0, verbindungen: [{ ...v('203.0.113.99', 443, 'HTTPS', 'internet', null, null, false, 'USA', 'BEISPIEL-NETZ'), pid: 5555, programm: '' }] },
  ];
  // Kaputte Eingaben: Felder fehlen, sind null oder haben den falschen Typ.
  const kaputt = [
    { programm: null, verbindungen: [{ ziel: null, ziel_port: null, pid: null, name: null, neu: true }, { ziel: '203.0.113.5' }] },
    { programm: 'Halb', verbindungen: [{ ziel: '203.0.113.6', ziel_port: '443', pid: 4242, programm: 'Halb' }] },
  ];
  const viele = Array.from({ length: 40 }, (_, i) => g(`Programm ${String(i + 1).padStart(2, '0')}`, Array.from({ length: 50 }, (_, j) =>
    v(`203.0.113.${(i * 7 + j) % 250 + 1}`, 443, 'HTTPS', 'internet', j % 3 ? `host-${i}-${j}.beispiel.example` : null, j % 3 ? 'PTR' : null, j % 17 === 0, 'Irland', 'BEISPIEL-CLOUD'))));
  VORSCHAU.verbindungen_zeigen = async () => {
    if (m('verbindungen-extrem')) return { gruppen: extrem, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: '2026-06' };
    if (m('verbindungen-kaputt')) return { gruppen: kaputt, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: 'kaputt' };
    if (m('verbindungen-viele') || m('verbindungen-viele-mehr')) return { gruppen: viele, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: '2026-06' };
    if (m('verbindungen-fehler')) throw 'Die Liste der Verbindungen ließ sich nicht lesen, auch `lsof` startet nicht.';
    if (m('verbindungen-keinnetz')) return { gruppen: gruppen.map((x) => ({ ...x, neu: 0, verbindungen: x.verbindungen.map((y) => ({ ...y, neu: false })) })), netz_name: null, erster_lauf: true, gespeichert: false, datenbank_stand: '2026-06' };
    const stand = m('verbindungen-ohnedb') ? null : '2026-06';
    if (m('verbindungen-leer')) return { gruppen: [], netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: stand };
    if (stand) return { gruppen, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: stand };
    const ohne = gruppen.map((g) => ({ ...g, verbindungen: g.verbindungen.map((x) => ({ ...x, land: null, asn: null, anbieter: null })) }));
    return { gruppen: ohne, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: null };
  };
  VORSCHAU.verbindungen_loeschen = async () => {};
  // Das gewählte Segment steht in localStorage: die Marke setzt es, ohne sie beginnt jede Vorschau bei „Ausgehend“.
  try { localStorage.setItem('netzinfo.verbindungen.segment', m('verbindungen-segment-kaputt') ? 'Unsinn{' : m('lauschend') ? 'lauschend' : 'ausgehend'); } catch { /* ohne Speicher */ }
  if (m('verbindungen-speicher-wirft')) for (const f of ['getItem', 'setItem']) Storage.prototype[f] = () => { throw new DOMException('voll', 'QuotaExceededError'); };
  if (m('verbindungen-traffichaengt') || m('offen-aussenhaengt')) window.NETZ_ZEITLIMIT_MS = 300;

  // Renderzeit der langen Listen: vom Klick auf den Tab bis die erste Zeile steht, im Seitentitel.
  if (marken.some((s) => s.endsWith('-viele') || s === 'verbindungen-viele-mehr')) {
    let t0;
    addEventListener('DOMContentLoaded', () => new MutationObserver((_, o) => {
      if (!document.querySelector('.verbindung, .offen-tabelle tbody tr')) return;
      o.disconnect();
      const n = document.querySelectorAll('.verbindung, .offen-tabelle tbody tr').length;
      document.title = `Renderzeit ${Math.round(performance.now() - (t0 ?? 0))} ms, ${n} Zeilen im DOM`;
    }).observe(document.body, { childList: true, subtree: true }));
    addEventListener('DOMContentLoaded', () => setTimeout(() => { t0 = performance.now(); }, 140));
  }

  if (marken.some((s) => s.startsWith('verbindungen') || s === 'lauschend')) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-verbindungen')?.click();
      if (m('verbindungen-neu') || m('verbindungen-keinnetz-neu')) setTimeout(() => document.querySelector('.werkzeuge button')?.click(), 400);
      if (m('verbindungen-viele-mehr')) setTimeout(() => document.querySelector('.mehr .knopf')?.click(), 400);
    }, 150));
  }
})();

// „Woher?“ (Issue #56): same Safari (PID 4242) as in the list above.
VORSCHAU.woher_zeigen = async () => ({
  kette: [
    { pid: 4242, ppid: 4100, name: 'Safari', pfad: '/Applications/Safari.app/Contents/MacOS/Safari', start: 1000, nutzer: 'mia' },
    { pid: 1, ppid: 0, name: 'launchd', pfad: '/sbin/launchd', start: 1, nutzer: 'root' },
  ],
  quelle: 'Unbekannt', eintrag: null, abgeschnitten: false,
  satz: 'launchd hat das Programm gestartet, vor 3 Stunden, aber ich finde keinen Eintrag dafür. Wahrscheinlich ein App-Start per Doppelklick, oder der ursprüngliche Elternprozess ist beendet und launchd hat übernommen.',
});
// #58: one measurement, identical numbers every time. Rates = bytes / 5 s; sorted by total rate.
(() => {
  const p = (programm, ein, aus) => ({ programm, ein, aus, rate_ein: ein / 5, rate_aus: aus / 5 });
  const nullen = [p('Backup', 0, 0), p(null, 5, 0), { programm: 'Leer', ein: 1, aus: 1, rate_ein: 0, rate_aus: 0 }];
  const extremTraffic = [{ programm: langerName, ein: 6.25e9, aus: 1e8, rate_ein: 1.25e9, rate_aus: 2e7 }, p('Programm ohne Rate', 0, 0)];
  VORSCHAU.traffic_messen = async () => marken.includes('verbindungen-trafficfehler') ? Promise.reject('Die Messung ging nicht: Netzinfo darf die Zähler nicht lesen.')
    : marken.includes('verbindungen-traffichaengt') ? new Promise(() => {})
    : marken.includes('verbindungen-kaputt') ? nullen
    : marken.includes('verbindungen-extrem') ? extremTraffic : [
    p('Backup', 2_000_000, 24_000_000), p('Safari', 9_500_000, 600_000), p('Mail', 120_000, 40_000), p('Systemeinstellungen', 0, 0),
  ];
  if (['verbindungen-traffic', 'verbindungen-trafficfehler', 'verbindungen-traffichaengt', 'verbindungen-kaputt', 'verbindungen-extrem'].some((x) => marken.includes(x))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => document.querySelector('.traffic .knopf')?.click(), 600));
  }
})();
