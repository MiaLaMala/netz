// Fake answers for the view „offen“ in vorschau.html.
// Marks: offen (list), offen-netz (filter „Nur Netz“ on), offen-aussen (nach „Von außen prüfen“), offen-aussenfehler (Prüfung scheitert), offen-leer, offen-unbekannt (Programm ohne Namen), offen-fehler; add ,erklaeren.
// Runde 4 (Robustheit): offen-viele (300 Ports, Renderzeit im Seitentitel), offen-extrem (80 Zeichen, IPv6 mit Zonen-ID, Port 0 und 65535, ohne Namen), offen-kaputt (reichweite oder port fehlen, pid null),
// offen-aussenhaengt (die Prüfung von außen antwortet nie, Zeitlimit greift), offen-danachfehler (Prüfung und Neulesen scheitern, der Knopf bleibt frei).
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
    l('TCP', 3000, 'Code Helper (Plugin) für lange Programmnamen im Beispiel', '*', 'Netz', ''),
    l('TCP', 631, 'Druckdienst', '127.0.0.1', 'Lokal', 'IPP'),
    l('TCP', 5432, 'Datenbank', '::1', 'Lokal', 'PostgreSQL'),
  ];
  // „auch von außen erreichbar“ kommt nur, wenn der Schalter „Von außen prüfen“ an ist.
  if (m('offen-unbekannt')) liste.push(l('TCP', 9100, '', '*', 'Netz', 'Druckdienst (RAW)'));
  if (m('offen-viele')) for (let i = 0; i < 300; i++) liste.push(l(i % 5 ? 'TCP' : 'UDP', 1024 + i, `Programm ${i % 40}`, i % 3 ? '*' : '127.0.0.1', i % 3 ? 'Netz' : 'Lokal', ''));
  if (m('offen-extrem')) liste.push(
    l('TCP', 0, 'Code-Helper-Renderer-Prozess-fuer-eine-sehr-lange-Erweiterung-mit-Namen-ohne-Leerzeichen-xxxxxxxxxxxx'.slice(0, 80), 'fe80::1ff:fe23:4567:890a%en0', 'Netz', ''),
    l('TCP', 65535, 'Google Chrome Helper (Renderer) [Profil 2]', '2001:db8:85a3:8d3:1319:8a2e:370:7348', 'Netz', ''),
    l('UDP', 4000, '', '*', 'Netz', ''));
  if (m('offen-kaputt')) liste.push({ protokoll: 'TCP', port: null, pid: null, programm: null, adresse: null, reichweite: null }, { protokoll: 'UDP', port: 99, pid: 4999, programm: 'Halb', adresse: '*', reichweite: 'Gibtsnicht' });
  let aussenVersucht = false;
  VORSCHAU.offen_zeigen = async ({ aussen }) => {
    if (aussen) aussenVersucht = true;
    if (aussen && m('offen-aussenhaengt')) return new Promise(() => {});
    if (m('offen-danachfehler') && aussenVersucht) throw 'Die Socket-Liste ließ sich nicht lesen, auch `lsof` startet nicht.';
    if (m('offen-fehler')) throw 'Die Socket-Liste ließ sich nicht lesen, auch `lsof` startet nicht.';
    if (aussen && m('offen-aussenfehler')) throw 'Das Netz ist nicht als „meins“ gemerkt, deshalb prüfe ich nicht von außen.';
    return m('offen-leer') ? [] : liste.map(({ aussen: a, ...x }) => ({ ...x, von_aussen: !!(a && aussen) }));
  };

  if (marken.some((s) => s.startsWith('offen'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-offen')?.click();
      if (m('offen-netz')) setTimeout(() => document.querySelector('.werkzeuge button')?.click(), 250);
      if (m('offen-aussen') || m('offen-aussenfehler') || m('offen-aussenhaengt') || m('offen-danachfehler')) setTimeout(() => document.querySelector('.werkzeug .knopf')?.click(), 250);
    }, 150));
  }
})();
