// Fake answers for the view „geraete“ in vorschau.html.
// Marks: geraete (not searched yet), geraete-lauf (normal run), geraete-erst (first run),
// geraete-leer, geraete-nichtmeins, geraete-fehler, geraete-suche (stays in progress); add ,erklaeren.
(() => {
  const m = (s) => marken.includes(s);
  const T = 1793050200;
  const TAG = 86400;
  const d = (ip, name, ports, erst, zuletzt) => ({ ip, name, ports, first_seen: T - erst, last_seen: T - zuletzt });
  const router = d('192.168.178.1', 'fritz.box', [53, 80, 443], 90 * TAG, 0);
  const mac = d('192.168.178.20', 'mias-macbook.fritz.box', [22, 445, 7000], 90 * TAG, 0);
  const drucker = d('192.168.178.9', 'drucker.fritz.box', [80, 631, 9100], 60 * TAG, 0);
  const handy = d('192.168.178.10', null, [], 3 * TAG, 0);
  const neu = d('192.168.178.42', 'wohnzimmer-tv.fritz.box', [8009, 7000, 9999], 0, 0);
  const weg = d('192.168.178.31', 'alter-laptop.fritz.box', [22], 40 * TAG, 5 * TAG);
  const nichtMeins = '`netzinfo geraete` läuft nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Es gibt dafür bewusst keine Option: Wer in Schule, Firma oder fremdem WLAN Geräte durchprobiert, bekommt schnell Ärger. Es wurde nichts gesendet.';
  const warte = (ms) => new Promise((r) => setTimeout(r, ms));

  VORSCHAU.geraete_suchen = async () => {
    await warte(m('geraete-suche') ? 600000 : 700);
    if (m('geraete-nichtmeins')) throw nichtMeins;
    if (m('geraete-fehler')) throw 'Das aktuelle Subnetz ist unbekannt.';
    const r = { bereich: '192.168.178.0/24', adressen: 254, geraete: [router, mac, handy, drucker, neu], neu: [neu], verschwunden: [weg], erster_lauf: false };
    if (m('geraete-leer')) return { ...r, geraete: [], neu: [], verschwunden: [] };
    if (m('geraete-erst')) return { ...r, geraete: [router, mac, drucker, handy], neu: [], verschwunden: [], erster_lauf: true };
    return r;
  };

  // vorschau.html only knows the mark „geraete“; the other marks open the tab and press „Suchen“ here.
  if (marken.some((s) => s.startsWith('geraete-'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-geraete')?.click();
      setTimeout(() => document.querySelector('.aktion button')?.click(), 250);
    }, 150));
  }
})();
