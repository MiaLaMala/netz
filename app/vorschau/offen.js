// Fake answers for the view „offen“ in vorschau.html.
// Marks: offen (list), offen-netz (filter „Nur Netz“ on), offen-aussen (nach „Von außen prüfen“), offen-leer; add ,erklaeren.
(() => {
  const m = (s) => marken.includes(s);
  const l = (protokoll, port, programm, adresse, reichweite, dienst, extra = {}) => ({
    protokoll, port, pid: 4000 + port % 1000, programm, nutzer: 'mia', dienst, adresse, reichweite,
    apple_signiert: reichweite === 'Netz' ? true : null, ...extra,
  });
  const liste = [
    l('TCP', 7000, 'AirPlay-Empfang', '*', 'Netz', 'AirPlay'),
    l('TCP', 8080, 'Entwicklungsserver', '*', 'Netz', 'HTTP (alternativ)', { apple_signiert: false, aussen: true }),
    l('UDP', 5353, 'Namensdienst', '*', 'Netz', 'mDNS'),
    l('TCP', 8384, 'Sync-Programm', '198.51.100.7', 'Vpn', ''),
    l('UDP', 53211, 'Namensauflösung', '*', 'UdpClient', ''),
    l('TCP', 631, 'Druckdienst', '127.0.0.1', 'Lokal', 'IPP'),
    l('TCP', 5432, 'Datenbank', '::1', 'Lokal', 'PostgreSQL'),
  ];
  // „auch von außen erreichbar“ kommt nur, wenn der Schalter „Von außen prüfen“ an ist.
  VORSCHAU.offen_zeigen = async ({ aussen }) =>
    (m('offen-leer') ? [] : liste.map(({ aussen: a, ...x }) => ({ ...x, von_aussen: !!(a && aussen) })));

  if (marken.some((s) => s.startsWith('offen'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-offen')?.click();
      if (m('offen-netz')) setTimeout(() => document.querySelector('.werkzeuge button')?.click(), 250);
      if (m('offen-aussen')) setTimeout(() => document.querySelector('.werkzeug .knopf')?.click(), 250);
    }, 150));
  }
})();
