// Fake answers for the view „verbindungen“ in vorschau.html.
// Marks: verbindungen (grouped list), verbindungen-traffic / verbindungen-trafficfehler (Messung läuft / scheitert), verbindungen-neu (filter „nur neue“ on), verbindungen-leer, verbindungen-ohnedb (no database), verbindungen-keinnetz (Netz nicht gemerkt), verbindungen-fehler; add ,erklaeren.
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
  VORSCHAU.verbindungen_zeigen = async () => {
    if (m('verbindungen-fehler')) throw 'Die Liste der Verbindungen ließ sich nicht lesen, auch `lsof` startet nicht.';
    if (m('verbindungen-keinnetz')) return { gruppen: gruppen.map((x) => ({ ...x, neu: 0, verbindungen: x.verbindungen.map((y) => ({ ...y, neu: false })) })), netz_name: null, erster_lauf: true, gespeichert: false, datenbank_stand: '2026-06' };
    const stand = m('verbindungen-ohnedb') ? null : '2026-06';
    if (m('verbindungen-leer')) return { gruppen: [], netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: stand };
    if (stand) return { gruppen, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: stand };
    const ohne = gruppen.map((g) => ({ ...g, verbindungen: g.verbindungen.map((x) => ({ ...x, land: null, asn: null, anbieter: null })) }));
    return { gruppen: ohne, netz_name: 'WG', erster_lauf: false, gespeichert: true, datenbank_stand: null };
  };
  VORSCHAU.verbindungen_loeschen = async () => {};

  if (marken.some((s) => s.startsWith('verbindungen'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-verbindungen')?.click();
      if (m('verbindungen-neu') || m('verbindungen-keinnetz-neu')) setTimeout(() => document.querySelector('.werkzeuge button')?.click(), 250);
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
  VORSCHAU.traffic_messen = async () => marken.includes('verbindungen-trafficfehler') ? Promise.reject('Die Messung ging nicht: Netzinfo darf die Zähler nicht lesen.') : [
    p('Backup', 2_000_000, 24_000_000), p('Safari', 9_500_000, 600_000), p('Mail', 120_000, 40_000), p('Systemeinstellungen', 0, 0),
  ];
  if (marken.includes('verbindungen-traffic') || marken.includes('verbindungen-trafficfehler')) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => document.querySelector('.traffic .knopf')?.click(), 600));
  }
})();
