// Fake answers for the view „gemerkte-netze“ in vorschau.html. Add keys to VORSCHAU, e.g.
// VORSCHAU.befehl = async (args) => ({ ... });
// Hash marks: netze-umbenennen (row 2 renamed to a taken name), netze-fehler (row 3 renamed to blanks),
// netze-vergessen (row 2 asks for confirmation), netze-vergessen-hier (row 1, the connected one),
// netze-eins (one network), netze-ladefehler (the list cannot be read).
VORSCHAU.netz_umbenennen = async ({ alt, neu }) => {
  const n = netze.find((x) => x.name === alt);
  const nameNeu = neu.trim();
  if (!nameNeu) throw 'Der neue Name ist leer. Nimm einen Namen wie „WG“.';
  if (nameNeu !== alt && netze.some((x) => x.name === nameNeu)) throw `Der Name „${nameNeu}“ ist schon für ein anderes Netz vergeben. Nimm einen anderen Namen.`;
  n.name = nameNeu;
  return n;
};
VORSCHAU.netz_markieren = async ({ name, meins }) => {
  const n = netze.find((x) => x.name === name);
  n.meins = meins;
  return n;
};
if (marken.includes('netze-eins')) netze = netze.slice(0, 1);
if (marken.includes('netze-ladefehler')) VORSCHAU.gemerkte_netze = async () => { throw 'Die gemerkten Netze ließen sich nicht lesen. Die Datei bleibt dabei unverändert.'; };
// Beispielbereiche statt privater Adressen (WG = Router 192.0.2.1, wie in allen Ansichten).
const beispiel = { WG: ['192.0.2.1', '192.0.2.0/24'], Arbeit: ['198.51.100.1', '198.51.100.0/24'], Hotspot: ['203.0.113.1', '203.0.113.0/28'] };
for (const n of netze) if (beispiel[n.name]) Object.assign(n.fingerprint, { router: beispiel[n.name][0], dhcp_server: beispiel[n.name][0], subnet: beispiel[n.name][1] });
// In den Marken dieser Ansicht ist der Mac gerade im Netz „WG“ (Zeile „Verbunden“).
const diesesNetz = VORSCHAU.dieses_netz;
VORSCHAU.dieses_netz = async (a) => {
  const r = await diesesNetz(a);
  return (marken.includes('netze') || marken.includes('tab-gemerkte-netze')) && !r.netz && !r.erkennung_fehler ? { ...r, netz: netze.find((x) => x.name === 'WG') ?? null } : r;
};
// Gespeicherte DNS-Messung je Netz (#88): WG alles frei, Arbeit nur DoH, Hotspot noch nie gemessen.
const dnsDieses = VORSCHAU.dns_gespeichert;
VORSCHAU.dns_gespeichert = async (a) => (!marken.includes('netze') ? dnsDieses(a) : ({
  WG: { messung: { dot: 'Erreichbar', doh: 'Erreichbar', ddr: null, zeit: t - 3600 * 5 }, frisch: true },
  Arbeit: { messung: { dot: 'Blockiert', doh: 'Erreichbar', ddr: null, zeit: t - 86400 * 3 }, frisch: false },
}[a.name] ?? null));
addEventListener('DOMContentLoaded', () => setTimeout(() => {
  const eingabe = (i, wert) => {
    const zeile = document.querySelector(`.netz-zeile:nth-child(${i})`);
    zeile?.querySelector('.umbenennen-knopf').click();
    const f = zeile?.querySelector('.umbenennen');
    if (!f) return;
    f.value = wert;
    f.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  };
  if (marken.includes('netze-umbenennen')) eingabe(2, 'WG');
  if (marken.includes('netze-fehler')) eingabe(3, '  ');
  if (marken.includes('netze-vergessen-hier')) document.querySelector('.netz-zeile:nth-child(1) .knopf:not(.umbenennen-knopf)')?.click();
  if (marken.includes('netze-vergessen')) document.querySelector('.netz-zeile:nth-child(2) .knopf:not(.umbenennen-knopf)')?.click();
}, 600));
// Start mit der Anmeldung (#9)
let autostartAn = false;
VORSCHAU.autostart_lesen = async () => autostartAn;
VORSCHAU.autostart_setzen = async ({ an }) => (autostartAn = an);
