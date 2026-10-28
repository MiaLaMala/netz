// Fake answers for the view „qualitaet“ in vorschau.html.
// Marks: #qualitaet (curve with some losses), ,qualitaet-luecken (20 min pause), ,qualitaet-leer,
// ,qualitaet-fehler, ,qualitaet-unbekannt, optional ,qualitaet-6h and ,erklaeren.
(() => {
  const NAMEN = ['Router (DNS)', '1.1.1.1 (DNS)', '1.1.1.1 (TCP 443)'];
  const jetzt = () => Math.floor(Date.now() / 1000);
  const rausch = (i, k) => Math.abs(Math.sin(i * 12.9898 + k * 78.233) * 43758.5453) % 1;

  // Deterministic fake series: every 30 s over 3 h, spikes now and then, a few losses.
  function reihen() {
    const ende = jetzt();
    return NAMEN.map((ziel, k) => {
      const punkte = [];
      for (let i = 0, t = ende - 3 * 3600; t < ende; i++, t += 30) {
        if (marken.includes('qualitaet-luecken') && t > ende - 2400 && t < ende - 1200) continue;
        const r = rausch(i, k);
        const spitze = i % 97 < 4 ? 25 + k * 20 : 0;
        const verlust = r > 0.985 || (k === 2 && i % 131 < 2);
        punkte.push({ zeit: t, ms: verlust ? null : [3, 14, 18][k] + r * 6 + spitze, verlust });
      }
      return { ziel, punkte };
    });
  }

  function bericht() {
    const r = reihen();
    const alle = r.flatMap((x) => x.punkte);
    const ziele = r.map(({ ziel, punkte }) => {
      const w = punkte.filter((p) => p.ms != null).map((p) => p.ms).sort((a, b) => a - b);
      const v = punkte.filter((p) => p.verlust).length;
      return {
        ziel, messungen: punkte.length, verloren: v, verlust_prozent: (v / punkte.length) * 100,
        median_ms: w[Math.floor(w.length / 2)], p95_ms: w[Math.floor(w.length * 0.95)], max_ms: w[w.length - 1],
        laengster_aussetzer_s: 60,
        schlechteste_stunde: { stunde: Math.floor(punkte[0].zeit / 3600) * 3600, verlust_prozent: 2.4, durchschnitt_ms: w[Math.floor(w.length / 2)] + 2 },
      };
    });
    return { netz: 'WG', von: alle[0].zeit, bis: jetzt(), ziele, reihen: r };
  }

  VORSCHAU.bericht = async () => {
    if (marken.includes('qualitaet-fehler')) throw 'Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.';
    if (marken.includes('qualitaet-unbekannt')) throw 'Dieses Netz ist nicht gemerkt. Merke es zuerst mit `netzinfo merken <Name>`, dann kann ich Messungen darunter speichern.';
    if (marken.includes('qualitaet-leer')) return { netz: 'WG', von: 0, bis: 0, ziele: [], reihen: [] };
    return bericht();
  };
  VORSCHAU.messen_einmal = async () => ({
    zeit: jetzt(),
    werte: NAMEN.map((ziel, k) => ({ ziel, ms: [3, 14, 18][k] + Math.random() * 6, verlust: false })),
  });

  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('qualitaet'))) setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
    if (marken.includes('qualitaet-6h')) setTimeout(() => { const s = document.querySelector('.werkzeuge select'); if (s) { s.value = '6'; s.dispatchEvent(new Event('change')); } }, 400);
  });
})();
