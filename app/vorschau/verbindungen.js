// Fake answers for the view „verbindungen“ in vorschau.html.
// Marks: verbindungen (grouped list), verbindungen-neu (filter „nur neue“ on), verbindungen-leer; add ,erklaeren.
(() => {
  const m = (s) => marken.includes(s);
  const v = (ziel, ziel_port, dienst, art, name, quelle, neu) => ({
    protokoll: 'TCP', lokal: '192.168.178.20:50123', ziel, ziel_port, dienst, zustand: 'ESTABLISHED', pid: 4242,
    programm: '', pfad: null, art, name, name_quelle: quelle, neu,
  });
  const g = (programm, vs) => ({ programm, neu: vs.filter((x) => x.neu).length, verbindungen: vs.map((x) => ({ ...x, programm })) });
  const gruppen = [
    g('Safari', [
      v('203.0.113.10', 443, 'HTTPS', 'internet', 'server-1.cdn.example', 'PTR', false),
      v('203.0.113.11', 443, 'HTTPS', 'internet', null, null, true),
      v('2001:db8::9', 443, 'HTTPS', 'internet', 'www.beispiel.example', 'PTR', false),
    ]),
    g('Mail', [
      v('203.0.113.40', 993, 'IMAPS', 'internet', 'imap.beispiel.example', 'PTR', false),
      v('203.0.113.41', 5223, 'Apple Push', 'internet', null, null, true),
    ]),
    g('Finder', [v('192.168.178.9', 445, 'SMB', 'netz', 'drucker.fritz.box', 'Gerät', false)]),
  ];
  VORSCHAU.verbindungen_zeigen = async () => {
    if (m('verbindungen-leer')) return { gruppen: [], netz_name: 'WG', erster_lauf: false, gespeichert: true };
    return { gruppen, netz_name: 'WG', erster_lauf: false, gespeichert: true };
  };
  VORSCHAU.verbindungen_loeschen = async () => {};

  if (marken.some((s) => s.startsWith('verbindungen'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-verbindungen')?.click();
      if (m('verbindungen-neu')) setTimeout(() => document.querySelector('.werkzeuge button')?.click(), 250);
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
