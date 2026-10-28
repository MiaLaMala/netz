// „Qualität“: misst alle 5 s, solange die Ansicht offen ist, und zeigt Kurve und Kennzahlen.
// Die Kennzahlen folgen `netzinfo bericht` (src/messen.rs).
(() => {
  const { el, zeit, gruppe, zeile, status, meldung, leerZustand } = Netz;
  const NS = 'http://www.w3.org/2000/svg';
  const TAKT = 5000; // ms, never faster: every probe rewrites qualitaet.json
  const LUECKE = 120; // s, same as GAP_SECS in src/messen.rs: a longer pause breaks the line
  // Dash patterns differ per target, so colour is not the only difference.
  const STIL = [
    { farbe: 'var(--accent)', dash: '' },
    { farbe: 'var(--gruen)', dash: '6 3' },
    { farbe: 'var(--orange)', dash: '1.5 3' },
  ];
  const E = {
    median: 'Der Median ist der mittlere Wert: Die Hälfte der Antworten war schneller, die Hälfte langsamer. Einzelne Ausreißer ändern ihn kaum.',
    p95: 'Das 95. Perzentil sagt: 95 von 100 Antworten waren schneller. Es zeigt, wie schlecht es an schlechten Momenten wird.',
    max: 'Das Maximum ist die langsamste Antwort, die je gemessen wurde.',
    verlust: 'Ein Verlust ist eine Anfrage, die nach 2 Sekunden keine Antwort hatte. Im Heimnetz sollten es fast null Prozent sein.',
    aussetzer: 'Der längste Aussetzer ist die längste Zeit, in der hintereinander jede Anfrage verloren ging.',
    stunde: 'Die schlechteste Stunde hatte den höchsten Verlust, danach die höchste Durchschnittszeit.',
  };

  let alt = null; // state of the last rendered view: { wurzel, an, stop }
  const fmt = (n, stellen = 1) => n.toLocaleString('de-DE', { minimumFractionDigits: stellen, maximumFractionDigits: stellen });
  const ms = (v) => (v == null ? '-' : `${fmt(v)} ms`);
  const dauer = (sek) => (sek === 0 ? 'keiner' : sek < 60 ? `${sek} s` : `${Math.floor(sek / 60)} min ${sek % 60} s`);
  const uhr = (sek) => new Date(sek * 1000).toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
  const sv = (tag, attr = {}, text) => {
    const e = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attr)) if (v !== '') e.setAttribute(k, v);
    if (text != null) e.textContent = text;
    return e;
  };
  const stunde = (w) => (w ? `${zeit(w.stunde)}, Verlust ${fmt(w.verlust_prozent)} %, Ø ${ms(w.durchschnitt_ms)}` : null);

  // Same text as `netzinfo bericht --markdown`, times in local time.
  function markdown(b) {
    const kopf = '| Ziel | Messungen | Verlust | Median | 95. Perzentil | Maximum | Längster Aussetzer |\n|---|---|---|---|---|---|---|\n';
    const zeilen = b.ziele.map((z) => `| ${z.ziel} | ${z.messungen} | ${fmt(z.verlust_prozent)} % | ${ms(z.median_ms)} | ${ms(z.p95_ms)} | ${ms(z.max_ms)} | ${dauer(z.laengster_aussetzer_s)} |\n`).join('');
    const schlecht = b.ziele.map((z) => `- ${z.ziel}: ${stunde(z.schlechteste_stunde) ?? '-'}\n`).join('');
    return `# Netzqualität: ${b.netz}\n\nZeitraum: ${zeit(b.von)} bis ${zeit(b.bis)}\n\nGemessen mit \`netzinfo messen\` (DNS-Anfragen und TCP-Verbindungen, kein Root). Ein Verlust ist eine Anfrage ohne Antwort innerhalb von 2 Sekunden.\n\n${kopf}${zeilen}\n## Schlechteste Stunde je Ziel\n\n${schlecht}`;
  }

  // Chart of the last `std` hours. Lines break at losses and pauses; losses sit on a strip below.
  function kurve(reihen, std, jetzt) {
    const B = 600, H = 230, L = 56, R = 10, T = 10, Y1 = 150, STREIFEN = 14;
    const von = jetzt - std * 3600;
    const sichtbar = reihen.map((r) => r.punkte.filter((p) => p.zeit >= von));
    const hoechst = Math.max(0, ...sichtbar.flat().map((p) => p.ms ?? 0));
    const oben = [10, 20, 30, 40, 50, 100, 200, 500, 1000, 2000].find((n) => n >= hoechst) ?? Math.ceil(hoechst / 1000) * 1000;
    const x = (t) => L + ((t - von) / (jetzt - von)) * (B - L - R);
    const y = (v) => Y1 - (v / oben) * (Y1 - T);
    const g = sv('svg', { viewBox: `0 0 ${B} ${H}`, class: 'kurve', role: 'img' });
    const zusammen = reihen.map((r, k) => {
      const p = sichtbar[k];
      const werte = p.filter((q) => q.ms != null).map((q) => q.ms).sort((a, b) => a - b);
      return `${r.ziel}: ${p.length} Messungen, ${p.filter((q) => q.verlust).length} Verluste`
        + (werte.length ? `, Median ${ms(werte[Math.floor(werte.length / 2)])}, Maximum ${ms(werte[werte.length - 1])}` : '');
    });
    g.setAttribute('aria-label', `Antwortzeiten der letzten ${std === 1 ? 'Stunde' : `${std} Stunden`}. ${zusammen.join('. ')}.`);
    for (const f of [0, 0.5, 1]) {
      g.append(sv('line', { x1: L, x2: B - R, y1: y(oben * f), y2: y(oben * f), class: 'gitter' }),
        sv('text', { x: L - 6, y: y(oben * f) + 4, class: 'achse', 'text-anchor': 'end' }, `${fmt(oben * f, 0)} ms`));
    }
    for (let i = 0; i <= 4; i++) {
      const t = von + ((jetzt - von) * i) / 4;
      g.append(sv('text', { x: x(t), y: H - 6, class: 'achse', 'text-anchor': i === 0 ? 'start' : i === 4 ? 'end' : 'middle' }, uhr(t)));
    }
    reihen.forEach((r, k) => {
      let d = '', zuletzt = null;
      for (const p of sichtbar[k]) {
        if (p.ms == null) { zuletzt = null; continue; }
        d += `${zuletzt && p.zeit - zuletzt <= LUECKE ? 'L' : 'M'}${x(p.zeit).toFixed(1)} ${y(p.ms).toFixed(1)}`;
        zuletzt = p.zeit;
      }
      if (d) g.append(sv('path', { d, class: 'linie', stroke: STIL[k].farbe, 'stroke-dasharray': STIL[k].dash }));
      const yv = Y1 + 16 + k * STREIFEN;
      for (const p of sichtbar[k]) {
        if (p.verlust) g.append(sv('line', { x1: x(p.zeit), x2: x(p.zeit), y1: yv - 5, y2: yv + 5, class: 'verlust', stroke: STIL[k].farbe }));
      }
    });
    g.append(sv('text', { x: L - 6, y: Y1 + 20, class: 'achse', 'text-anchor': 'end' }, 'Verluste'));
    return g;
  }

  function legende(namen) {
    return el('ul', { class: 'legende' }, namen.map((n, k) => {
      const l = sv('svg', { viewBox: '0 0 28 8', class: 'muster', 'aria-hidden': 'true' });
      l.append(sv('line', { x1: 1, x2: 27, y1: 4, y2: 4, stroke: STIL[k].farbe, 'stroke-dasharray': STIL[k].dash, 'stroke-width': 2 }));
      return el('li', {}, l, el('span', { text: n }));
    }));
  }

  // the chart shows the chosen range, these numbers cover every stored probe
  const kennzahlen = (b) => b.ziele.map((z) => gruppe(`${z.ziel}, alle Messungen`, [
    zeile('Median', ms(z.median_ms), E.median),
    zeile('95. Perzentil', ms(z.p95_ms), E.p95),
    zeile('Maximum', ms(z.max_ms), E.max),
    zeile('Verlust', `${fmt(z.verlust_prozent)} % (${z.verloren} von ${z.messungen})`, E.verlust),
    zeile('Längster Aussetzer', dauer(z.laengster_aussetzer_s), E.aussetzer),
    zeile('Schlechteste Stunde', stunde(z.schlechteste_stunde), E.stunde),
  ]));

  Netz.registriere({
    id: 'qualitaet',
    titel: 'Qualität',
    icon: Netz.icons.qualitaet,
    async render(wurzel) {
      // Reload of the same view (⌘R) keeps measuring; leaving the view stopped it.
      const weiter = alt?.wurzel.isConnected && alt.an;
      alt?.stop();
      let b;
      try {
        b = await Netz.invoke('bericht', { name: null });
      } catch (e) {
        const t = Netz.text(e);
        const unbekannt = t.includes('nicht gemerkt');
        wurzel.append(
          el('div', { class: 'kopf' }, el('h2', { text: unbekannt ? 'Unbekanntes Netz' : 'Keine Messung möglich' }),
            status(unbekannt ? 'orange' : 'rot', unbekannt ? 'Messungen gehören zu einem gemerkten Netz' : 'Fehler')),
          meldung(t, unbekannt ? 'orange' : 'rot'));
        return;
      }

      let std = 1;
      let timer = null;
      let seit = 0; // probes since the last full report
      const zustand = { wurzel, an: false, stop: () => { zustand.an = false; clearTimeout(timer); document.removeEventListener('visibilitychange', sicht); } };
      const stat = el('div', { 'aria-live': 'polite' });
      const kurvenOrt = el('div', { class: 'karte' });
      const zahlen = el('div');
      const fehler = el('div', { 'aria-live': 'polite' });
      const schalter = el('button', { class: 'erklaeren-schalter', role: 'switch', 'aria-checked': 'false', type: 'button' }, el('span', { class: 'spur' }), el('span', { text: 'Messen' }));
      const zeitraum = el('select', { 'aria-label': 'Zeitraum', onchange: () => { std = Number(zeitraum.value); zeichne(); } },
        el('option', { value: '1', text: '1 Stunde' }), el('option', { value: '6', text: '6 Stunden' }));
      const kopieren = el('button', { class: 'knopf', type: 'button', text: 'Bericht kopieren', onclick: async () => {
        try {
          await navigator.clipboard.writeText(markdown(b));
          kopieren.textContent = 'Kopiert';
          setTimeout(() => { kopieren.textContent = 'Bericht kopieren'; }, 2000);
        } catch {
          fehler.replaceChildren(meldung('Das Kopieren ging nicht. Erlaube der App den Zugriff auf die Zwischenablage und versuche es noch mal.'));
        }
      } });

      function zeichne() {
        const leer = b.ziele.length === 0;
        stat.replaceChildren(leer
          ? status('grau', 'Noch keine Messungen')
          : status(zustand.an ? 'gruen' : 'grau', `${zustand.an ? 'Misst alle 5 s' : 'Misst nicht'} · Messungen von ${zeit(b.von)} bis ${zeit(b.bis)}`));
        kopieren.disabled = leer;
        if (leer && !b.reihen.some((r) => r.punkte.length)) {
          kurvenOrt.replaceChildren(leerZustand(Netz.icons.qualitaet, 'Noch keine Messungen für dieses Netz. Schalte „Messen“ ein, dann entsteht die Kurve.'));
          zahlen.replaceChildren();
          return;
        }
        kurvenOrt.replaceChildren(kurve(b.reihen, std, Math.floor(Date.now() / 1000)), legende(['Router (DNS)', '1.1.1.1 (DNS)', '1.1.1.1 (TCP 443)']));
        zahlen.replaceChildren(...kennzahlen(b));
      }

      async function tick() {
        if (!wurzel.isConnected) return zustand.stop(); // view was left
        try {
          const p = await Netz.invoke('messen_einmal');
          fehler.replaceChildren();
          p.werte.forEach((w, k) => {
            if (w.ms == null && !w.verlust) return; // not measured
            (b.reihen[k] ??= { ziel: w.ziel, punkte: [] }).punkte.push({ zeit: p.zeit, ms: w.ms, verlust: w.verlust });
          });
          // ponytail: Kennzahlen are reread on the first and every 6th probe, the chart grows locally
          if (b.ziele.length === 0 || ++seit % 6 === 0) b = { ...(await Netz.invoke('bericht', { name: null })), reihen: b.reihen };
        } catch (e) {
          fehler.replaceChildren(meldung(Netz.text(e)));
        }
        if (!zustand.an || !wurzel.isConnected) return;
        zeichne();
        timer = setTimeout(tick, TAKT);
      }

      // Window hidden: no probes. Shown again: continue if the switch is on.
      function sicht() {
        clearTimeout(timer);
        if (!document.hidden && zustand.an) tick();
      }

      function schalte(an) {
        zustand.stop();
        zustand.an = an;
        schalter.setAttribute('aria-checked', an);
        if (an) { document.addEventListener('visibilitychange', sicht); tick(); } else zeichne();
      }
      schalter.addEventListener('click', () => schalte(!zustand.an));
      alt = zustand;

      wurzel.append(el('div', { class: 'kopf' }, el('h2', { text: b.netz })), stat,
        el('div', { class: 'werkzeuge' }, schalter, zeitraum, kopieren), fehler, kurvenOrt, zahlen);
      zeichne();
      if (weiter) schalte(true);
    },
  });
})();
