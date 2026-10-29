// Fake answers for the view „live“ in vorschau.html.
// Marks: #live (stopped, nothing measured yet), ,live-gut (starts, good line), ,live-schlecht (jitter and losses),
// ,live-zwei (two targets), ,live-fehler (name not found), ,live-recht (no ICMP socket), ,live-aus (target never answers);
// add ,erklaeren.
(() => {
  const m = (s) => marken.includes(s);
  const rausch = (i, k) => Math.abs(Math.sin(i * 12.9898 + k * 78.233) * 43758.5453) % 1;
  const GRENZ = { gut: 1, mittel: 3 }; // wie im Kern: unter Grenze gut, bis zum Dreifachen mit Einschränkung
  const stufe = (paare) => paare.map(([w, g]) => (w == null ? 'schlecht' : w < g ? 'gut' : w < g * GRENZ.mittel ? 'eingeschraenkt' : 'schlecht'))
    .reduce((a, b) => (['gut', 'eingeschraenkt', 'schlecht'].indexOf(b) > ['gut', 'eingeschraenkt', 'schlecht'].indexOf(a) ? b : a));
  const rang = (s, p) => s[Math.max(1, Math.ceil((s.length * p) / 100)) - 1];

  // Deterministic: one probe per second over the last 5 minutes.
  function ansicht(ziel, k) {
    const schlecht = m('live-schlecht') && k === 0;
    const aus = m('live-aus') && k === 0;
    const jetzt = Date.now();
    const punkte = [];
    for (let i = 0; i < 300; i++) {
      const r = rausch(i, k);
      const verlust = aus || (schlecht ? i % 23 === 7 : i === 140 && k === 1);
      const spitze = schlecht && i % 11 < 3 ? 60 + r * 90 : 0;
      punkte.push({ zeit: jetzt - (299 - i) * 1000, ms: verlust ? null : (k ? 18 : 11) + r * (schlecht ? 25 : 3) + spitze });
    }
    const f = punkte.slice(-60);
    const ms = f.map((p) => p.ms).filter((v) => v != null);
    const s = [...ms].sort((a, b) => a - b);
    if (!s.length) return { ziel, ziel_ip: /^[\d.]+$/.test(ziel) ? ziel : '203.0.113.7', punkte, kennzahlen: { gesendet: f.length, verlust_prozent: 100, min_ms: null, median_ms: null, p5_ms: null, p95_ms: null, max_ms: null, jitter_ms: null }, eignung: { anruf: 'schlecht', spiel: 'schlecht', streaming: 'schlecht' } };
    const diffs = f.slice(1).map((p, i) => (p.ms != null && f[i].ms != null ? Math.abs(p.ms - f[i].ms) : null)).filter((d) => d != null);
    const jitter = diffs.reduce((a, b) => a + b, 0) / diffs.length;
    const verlust = (100 * (f.length - ms.length)) / f.length;
    const med = s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
    const kennzahlen = { gesendet: f.length, verlust_prozent: verlust, min_ms: s[0], median_ms: med, p5_ms: rang(s, 5), p95_ms: rang(s, 95), max_ms: s[s.length - 1], jitter_ms: jitter };
    const eignung = {
      anruf: stufe([[kennzahlen.p95_ms, 100], [jitter, 30], [verlust, 1]]),
      spiel: stufe([[med, 50], [jitter, 15], [verlust, 0.5]]),
      streaming: stufe([[verlust, 2]]),
    };
    return { ziel, ziel_ip: /^[\d.]+$/.test(ziel) ? ziel : '203.0.113.7', punkte, kennzahlen, eignung };
  }

  VORSCHAU.live_sonde = async ({ ziel }) => {
    if (m('live-fehler')) throw `Für „${ziel}“ gibt es keine IP-Adresse. Prüfe den Namen.`;
    if (m('live-recht')) throw 'Kann keinen ICMP-Socket öffnen (keine Berechtigung). Auf dem Mac geht das ohne sudo; prüfe, ob eine Firewall oder ein Profil das blockt.';
    return ansicht(ziel, ['1.1.1.1', '9.9.9.9'].indexOf(ziel) === 1 ? 1 : 0);
  };

  if (marken.some((s) => s.startsWith('live'))) {
    addEventListener('DOMContentLoaded', () => setTimeout(() => {
      document.querySelector('#tab-live')?.click();
      if (m('live') && !marken.some((s) => s.startsWith('live-'))) return;
      setTimeout(() => {
        const feld = document.querySelector('.ziel-feld');
        if (m('live-zwei')) feld.value = '1.1.1.1 9.9.9.9';
        if (m('live-fehler')) feld.value = 'gibt-es-nicht.example';
        document.querySelector('[data-ansicht=live] .werkzeuge button')?.click();
      }, 200);
    }, 150));
  }
})();
