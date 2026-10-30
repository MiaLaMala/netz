// Fake answers for the view „gemerkte-netze“ in vorschau.html. Add keys to VORSCHAU, e.g.
// VORSCHAU.befehl = async (args) => ({ ... });
// Hash marks: netze-umbenennen (row 2 renamed to a taken name), netze-fehler (row 3 renamed to blanks),
// netze-vergessen (row 2 asks for confirmation), netze-vergessen-hier (row 1, the connected one),
// netze-eins (one network), netze-ladefehler (the list cannot be read).
// Einstellungen: einst (der Mac ist in „WG“), einst-fritz (Fritz!Box-Zugang da), einst-fritz-entfernen (Rückfrage),
// einst-fritz-formular (Formular offen), einst-fritz-fehler (Speichern abgelehnt). Zusammen mit tab-gemerkte-netze.
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
  return (marken.includes('netze') || marken.includes('einst') || marken.includes('tab-gemerkte-netze')) && !r.netz && !r.erkennung_fehler ? { ...r, netz: netze.find((x) => x.name === 'WG') ?? null } : r;
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
  if (marken.includes('einst-fritz-entfernen')) document.querySelector('.fritz-entfernen')?.click();
  if (marken.includes('einst-fritz-formular') || marken.includes('einst-fritz-fehler')) document.querySelector('.fritz-einrichten')?.click();
  if (marken.includes('einst-fritz-fehler')) document.querySelector('.fritz-formular')?.requestSubmit();
}, 600));
// Robustheit (#176, Runde 4). Zusammen mit tab-gemerkte-netze:
// netze-viele (100 Netze, lange Namen mit <b> und &amp;), netze-lang (drei Namen mit 60 Zeichen), netze-daten-fehlen (Fingerabdruck
// und Daten null oder Unsinn), netze-dns-fehler (gespeicherte DNS-Messung nicht lesbar), netze-ladefehler-objekt / -error / -leer
// (Fehler als { art, text }, als Error, als nichts), netze-render-wirft (Kern liefert null, die Ansicht wirft beim Zeichnen),
// einst-autostart-fehler, einst-fritz-schluessel (Schlüsselbund nicht lesbar), einst-fritz-entfernen-fehler (Entfernen scheitert
// zweimal, eine Meldung), fang-fehler (unbehandelte Fehler, ein Band), speicher-gesperrt (localStorage wirft).
const lang = (stamm) => `${stamm} <b>fett</b> &amp; ${'x'.repeat(60)}`.slice(0, 60);
if (marken.includes('netze-viele')) {
  netze = [...netze, ...Array.from({ length: 97 }, (_, i) => ({ name: i % 5 ? `Gast ${i + 4}` : lang(`Netz ${i + 4}`), fingerprint: fp2(i), meins: false, first_seen: t - 86400 * (i + 1), last_seen: t - 86400 * i }))];
}
function fp2(i) { return { router: `198.51.100.${(i % 250) + 1}`, dhcp_server: `198.51.100.${(i % 250) + 1}`, domain: null, subnet: `198.51.100.0/24` }; }
if (marken.includes('netze-lang')) netze = netze.map((n, i) => ({ ...n, name: ['<b>fett</b> & &amp; ' + 'W'.repeat(41), 'Büro-Netz-im-zweiten-Stock-links-neben-der-Küche-1234567890', '🏨 Hotel „Seeblick“ ' + 'ä'.repeat(40)][i].slice(0, 60) }));
if (marken.includes('netze-daten-fehlen')) {
  netze = netze.map((n, i) => (i === 1 ? { ...n, fingerprint: null, first_seen: null, last_seen: 1e18 } : i === 2 ? { ...n, fingerprint: {}, first_seen: 0, last_seen: undefined } : n));
}
if (marken.includes('netze-dns-fehler')) VORSCHAU.dns_gespeichert = async () => { throw 'Die gespeicherte DNS-Messung ließ sich nicht lesen.'; };
const ladefehler = { 'netze-ladefehler-objekt': { art: 'sonstiges', text: 'Die gemerkten Netze ließen sich nicht lesen. Die Datei bleibt dabei unverändert.' },
  'netze-ladefehler-error': new Error('Failed to fetch'), 'netze-ladefehler-leer': undefined };
for (const [m, f] of Object.entries(ladefehler)) if (marken.includes(m)) VORSCHAU.gemerkte_netze = async () => { throw f; };
if (marken.includes('netze-render-wirft')) VORSCHAU.gemerkte_netze = async () => null;
if (marken.includes('speicher-gesperrt')) for (const f of ['getItem', 'setItem']) Storage.prototype[f] = () => { throw new DOMException('Speicher voll', 'QuotaExceededError'); };
// Geräte-Vorschau definiert die Fritz!Box-Befehle erst später: erst nach dem Laden überschreiben.
addEventListener('DOMContentLoaded', () => {
  if (marken.includes('einst-autostart-fehler')) VORSCHAU.autostart_lesen = async () => { throw 'Der Autostart ließ sich nicht lesen. Prüfe in den Systemeinstellungen unter Allgemein, Anmeldeobjekte.'; };
  if (marken.includes('einst-fritz-schluessel')) VORSCHAU.fritzbox_zugang = async () => { throw 'Der Schlüsselbund ist gesperrt. Entsperre ihn in der App „Schlüsselbund“ und lade die Einstellungen neu.'; };
  if (marken.includes('einst-fritz-entfernen-fehler')) VORSCHAU.fritzbox_entfernen = async () => { throw 'Der Schlüsselbund ließ das Entfernen nicht zu. Entferne den Eintrag „Netzinfo Fritz!Box“ selbst in der App „Schlüsselbund“.'; };
});
addEventListener('DOMContentLoaded', () => setTimeout(() => {
  if (marken.includes('einst-fritz-entfernen-fehler')) {
    document.querySelector('.fritz-entfernen')?.click();
    for (let i = 0; i < 2; i++) document.querySelector('.bestaetigung .knopf.gefahr')?.click();
  }
  if (marken.includes('fang-fehler')) {
    Promise.reject({ art: 'sonstiges', text: 'Die Fritz!Box antwortet nicht. Prüfe, ob sie eingeschaltet ist.' });
    for (let i = 0; i < 3; i++) Promise.reject(new Error('boom'));
    setTimeout(() => { throw new TypeError('x ist undefined'); });
  }
}, 900));
// Start mit der Anmeldung (#9)
let autostartAn = false;
VORSCHAU.autostart_lesen = async () => autostartAn;
VORSCHAU.autostart_setzen = async ({ an }) => (autostartAn = an);
