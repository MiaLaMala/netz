// „Live“: Antwortzeit zu ein bis vier Zielen als Kurve der letzten 5 Minuten, mit Jitter, Verlust und
// drei Eignungs-Ampeln (Kern: src/live.rs). Gemessen wird nur, solange die Ansicht sichtbar ist:
// Der Takt endet, sobald die Wurzel nicht mehr im Dokument hängt oder das Fenster verborgen ist.
(() => {
  const { el, status, meldung, gruppe, zeile, invoke } = Netz;
  Netz.icons.live = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1.5 11.5 4 8l2.5 2.5L9.5 4l2 5 3-2"/></svg>';

  const E = {
    jitter: 'Jitter ist das Zittern der Antwortzeit. Bei Videoanrufen führt hoher Jitter zu abgehackter Stimme, auch wenn der Durchschnitt gut aussieht. Netzinfo rechnet die mittlere Differenz aufeinanderfolgender Antwortzeiten.',
    verlust: 'Verlust sind Pakete, die nach 2 Sekunden keine Antwort hatten. Wenn der Mac schläft oder die Messung pausiert, wird nichts gesendet und also auch nichts als Verlust gezählt.',
    ampel: 'Die Ampeln sind Faustwerte der letzten 60 Sekunden. Videoanruf: gut bei P95 unter 100 ms, Jitter unter 30 ms, Verlust unter 1 %. Spiele: gut bei Median unter 50 ms, Jitter unter 15 ms, Verlust unter 0,5 %. Streaming: gut bei Verlust unter 2 %. „Mit Einschränkung“ gilt bis zum Dreifachen, darüber „schlecht“.',
    ping: 'Gemessen wird mit kleinen Ping-Paketen (ICMP Echo), ohne sudo. Router und Ziele beantworten sie manchmal langsamer als echten Verkehr; die Zeit kann höher wirken als in einer App.',
  };
  const FARBE = { gut: 'gruen', eingeschraenkt: 'orange', schlecht: 'rot' };
  const WORT = { gut: 'gut', eingeschraenkt: 'mit Einschränkung', schlecht: 'schlecht' };
  const FENSTER_MS = 300000;
  const NS = 'http://www.w3.org/2000/svg';
  const ms = (v) => (v == null ? '–' : `${v.toFixed(v < 10 ? 1 : 0)} ms`);

  const sv = (tag, attr) => {
    const e = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attr)) e.setAttribute(k, v);
    return e;
  };

  // Kurve, Band P5 bis P95 (nur über das Kennzahlenfenster der letzten 60 s) und rote Striche für Verlust.
  function kurve(a) {
    const B = 600, H = 150, L = 36, U = 18;
    const jetzt = a.punkte.length ? a.punkte[a.punkte.length - 1].zeit : 0;
    const werte = a.punkte.map((p) => p.ms).filter((v) => v != null);
    const max = Math.max(50, ...werte) * 1.1;
    const x = (t) => L + ((t - (jetzt - FENSTER_MS)) / FENSTER_MS) * (B - L - 4);
    const y = (v) => H - U - (v / max) * (H - U - 6);
    const s = sv('svg', { viewBox: `0 0 ${B} ${H}`, class: 'kurve', role: 'img', 'aria-label': `Antwortzeit zu ${a.ziel}, letzte 5 Minuten` });
    for (const t of [0, 0.5, 1]) {
      const v = max * t;
      s.append(sv('line', { x1: L, x2: B - 4, y1: y(v), y2: y(v), class: 'gitter' }));
      const z = sv('text', { x: L - 6, y: y(v) + 4, 'text-anchor': 'end', class: 'achse' });
      z.textContent = Math.round(v);
      s.append(z);
    }
    const k = a.kennzahlen;
    if (k && k.p5_ms != null) {
      s.append(sv('rect', { x: x(jetzt - 60000), y: y(k.p95_ms), width: x(jetzt) - x(jetzt - 60000), height: Math.max(2, y(k.p5_ms) - y(k.p95_ms)), class: 'band' }));
    }
    // Eine Linie je Strecke ohne Verlust; ein Verlust unterbricht sie.
    const teile = [[]];
    for (const p of a.punkte) {
      if (p.ms == null) teile.push([]);
      else teile[teile.length - 1].push(`${x(p.zeit).toFixed(1)} ${y(p.ms).toFixed(1)}`);
    }
    for (const t of teile.filter((t) => t.length)) s.append(sv('path', { d: `M${t.join(' L')}`, class: 'linie' }));
    for (const p of a.punkte) {
      if (p.ms == null) s.append(sv('line', { x1: x(p.zeit), x2: x(p.zeit), y1: H - U, y2: H - U + 12, class: 'verlust' }));
    }
    return s;
  }

  function karte(a) {
    const k = a.kennzahlen;
    const ampeln = a.eignung
      ? [['Videoanruf', a.eignung.anruf], ['Spiele', a.eignung.spiel], ['Streaming', a.eignung.streaming]]
      : [];
    const gesendet = k ? `${k.gesendet} Pakete` : 'noch keine Antwort';
    return el('section', { class: 'gruppe live-ziel', 'aria-label': a.ziel },
      el('h3', { text: a.ziel === a.ziel_ip ? a.ziel : `${a.ziel} · ${a.ziel_ip}` }),
      el('div', { class: 'karte' }, kurve(a),
        el('ul', { class: 'legende' },
          el('li', {}, el('span', { class: 'muster linie-muster' }), 'Antwortzeit in ms'),
          el('li', {}, el('span', { class: 'muster band-muster' }), 'P5 bis P95 (60 s)'),
          el('li', {}, el('span', { class: 'muster verlust-muster' }), 'Verlust'))),
      el('dl', { class: 'liste' },
        zeile('Median', k && ms(k.median_ms)),
        zeile('P95', k && ms(k.p95_ms)),
        zeile('Jitter', k && ms(k.jitter_ms), E.jitter),
        zeile('Verlust', k && `${k.verlust_prozent.toFixed(1)} %`, E.verlust),
        zeile('Maximum', k && ms(k.max_ms))),
      el('p', { class: 'fuss', text: `Kennzahlen der letzten 60 s, ${gesendet}.` }),
      el('div', { class: 'ampeln' }, ampeln.map(([n, st]) => el('div', { class: 'ampel' },
        el('span', { class: 'fein', text: n }), status(FARBE[st], WORT[st])))));
  }

  Netz.registriere({
    id: 'live',
    titel: 'Live',
    icon: Netz.icons.live,
    async render(wurzel) {
      let laeuft = false, nr = 0;
      const feld = el('input', { type: 'text', class: 'ziel-feld', value: '1.1.1.1', 'aria-label': 'Ziele', placeholder: '1.1.1.1 9.9.9.9', autocapitalize: 'off', spellcheck: 'false' });
      const knopf = el('button', { class: 'knopf', type: 'button', text: 'Start' });
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'off' });
      const ziele = () => feld.value.split(/[\s,]+/).filter(Boolean).slice(0, 4);

      const takt = async (neu, lauf) => {
        if (lauf !== nr) return; // gestoppt oder neu gestartet
        // Nur messen, solange die Ansicht im Dokument hängt; verborgenes Fenster setzt nur aus.
        if (!wurzel.isConnected) return stopp();
        const t0 = Date.now();
        if (!document.hidden) {
          try {
            const antworten = await Promise.all(ziele().map((z) => invoke('live_sonde', { ziel: z, neu })));
            if (lauf !== nr) return;
            ort.replaceChildren(...antworten.map(karte));
          } catch (f) {
            if (lauf !== nr) return;
            stopp();
            return ort.replaceChildren(meldung(Netz.text(f), 'rot'));
          }
        }
        setTimeout(() => takt(false, lauf), Math.max(0, 1000 - (Date.now() - t0)));
      };
      const stopp = () => { laeuft = false; nr++; knopf.textContent = 'Start'; feld.disabled = false; };
      const start = () => {
        if (!ziele().length) return;
        laeuft = true; feld.disabled = true; knopf.textContent = 'Stopp';
        takt(true, ++nr);
      };
      knopf.addEventListener('click', () => (laeuft ? stopp() : start()));
      feld.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !laeuft) start(); });

      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Live' }),
          el('p', { class: 'status' }, el('span', { text: 'Antwortzeit, Jitter und Verlust als Kurve. Gemessen wird nur, solange diese Ansicht offen ist.' }))),
        el('div', { class: 'werkzeuge' }, feld, knopf),
        ort,
        Object.assign(gruppe('So wird gemessen', [
          zeile('Messung', 'Ping (ICMP Echo), jede Sekunde', E.ping),
          zeile('Eignung', 'Faustwerte, keine Norm', E.ampel),
        ]), { className: 'gruppe erklaer-gruppe' }));
      // Beim Verlassen stoppt der Takt von selbst: die Wurzel hängt dann nicht mehr im Dokument.
    },
  });
})();
