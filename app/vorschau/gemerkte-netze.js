// Fake answers for the view „gemerkte-netze“ in vorschau.html. Add keys to VORSCHAU, e.g.
// VORSCHAU.befehl = async (args) => ({ ... });
// Hash marks: netze-umbenennen (row 2 renamed to a taken name), netze-fehler (row 3 renamed to blanks).
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
}, 600));
