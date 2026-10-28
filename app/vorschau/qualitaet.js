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

// „Wo hakt es?“ (issue #31). Marks: ,diagnose-ok (default), ,diagnose-router, ,diagnose-dns (see below), ,diagnose-laeuft,
// plus ,qualitaet; the button is clicked for you. All numbers are made up.
(() => {
  const z = (name, ms, v = 0, status = 'ok') => ({ name, gesendet: 20, beantwortet: ms == null ? 0 : Math.round(20 * (1 - v / 100)), verlust_prozent: v, median_ms: ms, p95_ms: ms && ms * 1.8, jitter_ms: ms && 3, status, gruende: [] });
  const gut = [z('Router', 3.2), z('Erster Rechner des Anbieters', 9.8), z('1.1.1.1', 12.1), z('9.9.9.9', 13.4)];
  VORSCHAU.diagnose = () => new Promise((ok) => setTimeout(() => ok(
    marken.includes('diagnose-router')
      ? { ziele: [z('Router', 41.0, 15, 'schlecht'), ...gut.slice(1)], urteil: 'wlan_oder_router', satz: 'Das Problem liegt wahrscheinlich im WLAN oder am Router.', begruendung: ['Router verliert 15,0 % der Pakete.', 'Router: Im schlechten Moment (95. Perzentil) 73,8 ms statt sonst 6,1 ms, mehr als das Dreifache. Im Mittel (Median) antwortet Router hier sonst in 3,0 ms, jetzt in 41,0 ms.'], tcp_443: null, mit_basis: true, zeitpunkt: 0 }
      : marken.includes('diagnose-dns')
        ? { ziele: [gut[0], z('Erster Rechner des Anbieters', 62, 20, 'schlecht'), z('1.1.1.1', 180, 10, 'schlecht'), z('9.9.9.9', 190, 25, 'schlecht')], urteil: 'anbieter', satz: 'Das Problem liegt wahrscheinlich beim Anbieter.', begruendung: ['Der Router antwortet in Ordnung.', 'Erster Rechner des Anbieters verliert 20,0 % der Pakete.', '1.1.1.1: Median 180,0 ms, über der festen Grenze von 150,0 ms.'], tcp_443: null, mit_basis: false, zeitpunkt: 0 }
        : { ziele: gut, urteil: 'in_ordnung', satz: 'Keine Auffälligkeit gemessen.', begruendung: ['Router und beide Internet-Ziele antworten ohne Auffälligkeit.'], tcp_443: null, mit_basis: true, zeitpunkt: 0 },
  ), marken.includes('diagnose-laeuft') ? 60000 : 300));
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('diagnose'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.diagnose button')?.click(), 500);
    }
  });
})();
