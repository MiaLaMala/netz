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

  // „Wo hakt es?“ (issue #31): own block, `netzinfo diagnose` in the app.
  const AMPEL = { ok: 'gruen', schlecht: 'orange', stumm: 'rot', fehlt: 'grau' };
  const WORT = { ok: 'in Ordnung', schlecht: 'auffällig', stumm: 'stumm', fehlt: 'nicht gemessen' };
  const URTEIL = { funk: 'rot', in_ordnung: 'gruen', weit_weg: 'orange', wlan_oder_router: 'rot', anbieter: 'rot', kein_internet: 'rot' };
  // Funk (issue #37): bottom row; no Wi-Fi = row says so, never an error.
  const FUNK = { gut: ['gruen', 'in Ordnung'], schwach: ['orange', 'schwach'], nicht_geprueft: ['grau', 'nicht geprüft'], kabel: ['grau', 'kein WLAN (Kabel), entfällt'] };
  function funkZeile(f) {
    if (!f) return null;
    const [ampel, wort] = FUNK[f.status] || FUNK.nicht_geprueft;
    const zahlen = [f.rssi_dbm != null && `Signal ${f.rssi_dbm} dBm`, f.snr_db != null && `Abstand zum Rauschen ${f.snr_db} dB`].filter(Boolean).join(', ');
    return el('li', {}, el('span', { class: `punkt ${ampel}` }), el('span', { class: 'diagnose-name', text: 'Funk' }),
      el('span', { class: 'diagnose-wert', text: zahlen ? `${zahlen} · ${wort}` : wort }));
  }
  function diagnoseKarte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Wo hakt es?' });
    knopf.addEventListener('click', async () => {
      knopf.disabled = true;
      ort.replaceChildren(status('grau', 'Misst etwa 5 Sekunden an vier Stellen'));
      try {
        const r = await Netz.invoke('diagnose');
        ort.replaceChildren(
          el('ul', { class: 'diagnose-ziele' }, funkZeile(r.funk), r.ziele.map((z) => el('li', {},
            el('span', { class: `punkt ${AMPEL[z.status]}` }),
            el('span', { class: 'diagnose-name', text: z.name }),
            el('span', { class: 'diagnose-wert', text: z.status === 'fehlt' ? 'nicht gemessen'
              : `${z.beantwortet === 0 ? 'keine Antwort' : `${ms(z.median_ms)}, Verlust ${fmt(z.verlust_prozent)} %`} · ${WORT[z.status]}` })))),
          el('p', { class: 'diagnose-urteil' }, el('span', { class: `punkt ${URTEIL[r.urteil]}` }), el('strong', { text: r.satz })),
          el('ul', { class: 'diagnose-gruende' }, r.begruendung.map((t) => el('li', { text: t }))),
          r.verlauf ? el('p', { class: 'diagnose-verlauf', text: r.verlauf }) : '',
          el('p', { class: 'diagnose-hinweis', text: `${r.mit_basis ? 'Verglichen mit den gespeicherten Werten dieses Netzes.' : 'Für dieses Netz gibt es noch keine gespeicherten Werte, es gelten feste Grenzen.'} Das Urteil ist eine Vermutung, kein Beweis.` }));
      } catch (e) {
        ort.replaceChildren(meldung(Netz.text(e)));
      }
      knopf.disabled = false;
    });
    return el('section', { class: 'karte diagnose', 'aria-label': 'Wo hakt es?' }, knopf, ort);
  }

  // ---- Karte „Weg zum Ziel“ (`netzinfo pfad`, src/pfad.rs) ----
  const EW = {
    karte: 'Jedes Paket hat eine Lebensdauer (TTL), die an jedem Router um eins sinkt. Ich schicke Pakete mit TTL 1, 2, 3 …: Der Router, bei dem sie ablaufen, meldet sich. So sehe ich Station für Station, wo die Zeit vergeht.',
    balken: 'Der Balken zeigt den Median der Antwortzeit an dieser Station, relativ zur langsamsten.',
    verlust: 'Verlust nur an einer Station mitten im Weg ist meist harmlos: Viele Router beantworten diese Anfragen absichtlich nur selten. Verlust am Ende des Wegs (rot) ist ein echtes Zeichen, dass Pakete das Ziel nicht erreichen.',
  };
  // Running measurement and last answer live outside the view: ⌘R or a tab change must neither
  // start a second run nor lose the answer. The backend cannot be cancelled, a late answer is only stored.
  let wegLaeuft = null; // { ziel, p: Promise }
  let wegLetzt = null; // { erg } | { fehler }

  function hopZeile(h, max, ende) {
    const adr = h.adressen.length ? (h.name ?? h.adressen[0]) + (h.adressen.length > 1 ? ` (+${h.adressen.length - 1})` : '') : 'keine Antwort';
    const rot = ende && h.verlust_prozent > 0;
    const breite = h.median_ms != null && max > 0 ? Math.max(2, (h.median_ms / max) * 100) : 0;
    return el('li', { class: `hop${rot ? ' ende-verlust' : ''}${h.adressen.length ? '' : ' stumm'}` },
      el('span', { class: 'ttl', text: h.ttl }),
      el('span', { class: 'adresse', text: adr + (h.ziel ? ' (Ziel)' : '') }),
      el('span', { class: 'balken', 'aria-hidden': 'true' }, el('span', { style: `width:${breite}%` })),
      el('span', { class: 'zahl', text: ms(h.median_ms) }),
      el('span', { class: 'zahl verlust', text: `${fmt(h.verlust_prozent, 0)} % Verlust` }));
  }

  function wegKarte() {
    const eingabe = el('input', { type: 'text', class: 'ziel-feld', value: wegLaeuft?.ziel ?? '1.1.1.1', 'aria-label': 'Ziel', placeholder: '1.1.1.1', autocapitalize: 'off', spellcheck: 'false' });
    const router = el('button', { class: 'knopf', type: 'button', text: 'Router', onclick: async () => {
      try { const r = (await Netz.invoke('dieses_netz')).info.router; if (r) eingabe.value = r; } catch { /* the field keeps its value */ }
    } });
    const knopf = el('button', { class: 'knopf haupt', type: 'button', text: 'Pfad messen', onclick: messe });
    const lauf = el('p', { class: 'fuss fortschritt', 'aria-live': 'polite' });
    const ort = el('div', { 'aria-live': 'polite' });
    const k = el('section', { class: 'gruppe weg-karte', 'aria-labelledby': 'g-weg' },
      el('h3', { id: 'g-weg', text: 'Weg zum Ziel' }),
      el('div', { class: 'karte weg' },
        el('div', { class: 'werkzeuge' }, eingabe, router, knopf), lauf, ort,
        el('p', { class: 'erklaerung', text: EW.karte })));

    function zeichneWeg() {
      const laeuft = wegLaeuft != null;
      knopf.disabled = laeuft;
      eingabe.disabled = router.disabled = laeuft;
      lauf.replaceChildren(...(laeuft ? [el('progress', { 'aria-label': 'Messung läuft' }), ` Messe den Weg zu ${wegLaeuft.ziel} … das dauert bis zu 30 Sekunden.`] : []));
      if (laeuft) return ort.replaceChildren();
      if (!wegLetzt) return ort.replaceChildren(leerZustand(Netz.icons.qualitaet, 'Noch nicht gemessen. „Pfad messen“ zeigt jede Station zwischen dir und dem Ziel.'));
      if (wegLetzt.fehler) return ort.replaceChildren(meldung(wegLetzt.fehler));
      const e = wegLetzt.erg;
      const max = Math.max(0, ...e.hops.map((h) => h.median_ms ?? 0));
      ort.replaceChildren(...[
        status(e.erreicht ? 'gruen' : 'orange', `${e.ziel} (${e.ziel_ip}) · ${e.hops.length} Stationen · ${zeit(e.zeitpunkt)}`),
        !e.erreicht && meldung('Das Ziel hat nicht geantwortet. Der Weg endet dort, wo die Antworten aufhören. Das Ziel kann Anfragen auch absichtlich ignorieren.', 'orange'),
        el('ol', { class: 'hops' }, e.hops.map((h, i) => hopZeile(h, max, i === e.hops.length - 1))),
        el('p', { class: 'erklaerung', text: EW.balken }),
        el('p', { class: 'erklaerung', text: EW.verlust })].filter(Boolean));
    }

    async function messe() {
      if (wegLaeuft) return;
      const ziel = eingabe.value.trim() || '1.1.1.1';
      const p = Netz.invoke('pfad_messen', { ziel }).then((erg) => ({ erg }), (f) => ({ fehler: Netz.text(f) }));
      wegLaeuft = { ziel, p };
      zeichneWeg();
      wegLetzt = await p;
      wegLaeuft = null;
      if (k.isConnected) { zeichneWeg(); knopf.focus(); }
    }

    zeichneWeg();
    // Re-render during a run: wait for the same run, then draw.
    if (wegLaeuft) wegLaeuft.p.then(() => k.isConnected && zeichneWeg());
    return k;
  }

  // ---- Karte „WLAN-Verlauf“ (`netzinfo wlan --verlauf`, src/wlanverlauf.rs, issue #36) ----
  const EV = {
    marken: 'Roaming heißt: Der Mac hat den Zugangspunkt gewechselt, ohne dass die Verbindung abriss. Das ist normal, wenn man sich bewegt, und ein Problem, wenn man am Schreibtisch sitzt.',
    vergleich: 'Verglichen wird der Median dieses Besuchs mit dem Median der Mediane früherer Besuche in diesem Netz.',
    ohne_hash: 'Ohne Standortfreigabe sieht Netzinfo keinen Wechsel des Zugangspunkts, Abbrüche schon. Die Freigabe gibst du in „Dieses Netz“.',
  };
  const ARTEN = { Roaming: ['Zugangspunkt gewechselt', 'currentColor', 'M0 -5L4 0L0 5L-4 0Z'], Getrennt: ['Verbindung weg', 'var(--rot)', 'M-4 -4L4 4M4 -4L-4 4'], Verbunden: ['wieder verbunden', 'var(--gruen)', 'M-4 0L-1 4L5 -4'], Bandwechsel: ['Band gewechselt', 'var(--orange)', 'M-4 4L0 -5L4 4Z'] };

  // Signal (solid) and rate (dashed) of the ring buffer; events as marks on a strip below.
  function wlanKurve(a) {
    const B = 600, H = 200, L = 70, R = 46, T = 22, Y1 = 130;
    const p = a.proben;
    const von = p[0].zeit, bis = Math.max(p[p.length - 1].zeit, von + 60);
    const x = (t) => L + ((t - von) / (bis - von)) * (B - L - R);
    const ySig = (v) => Y1 - ((Math.min(-30, Math.max(-90, v)) + 90) / 60) * (Y1 - T);
    const rmax = Math.max(100, ...p.map((q) => q.rate ?? 0));
    const yRate = (v) => Y1 - (v / rmax) * (Y1 - T);
    const g = sv('svg', { viewBox: `0 0 ${B} ${H}`, class: 'wlan-kurve', role: 'img' });
    const sig = p.filter((q) => q.rssi != null).map((q) => q.rssi).sort((m, n) => m - n);
    g.setAttribute('aria-label', `Signal und Senderate der letzten ${Math.max(1, Math.round((bis - von) / 60))} Minuten. `
      + (sig.length ? `Signal im Median ${sig[Math.floor(sig.length / 2)]} dBm. ` : '')
      + `${a.ereignisse.length} Ereignisse.`);
    for (const f of [0, 0.5, 1]) {
      const yy = Y1 - f * (Y1 - T);
      g.append(sv('line', { x1: L, x2: B - R, y1: yy, y2: yy, class: 'gitter' }),
        sv('text', { x: L - 6, y: yy + 4, class: 'achse', 'text-anchor': 'end' }, `${-90 + f * 60} dBm`),
        sv('text', { x: B - R + 6, y: yy + 4, class: 'achse' }, `${fmt(rmax * f, 0)}`));
    }
    g.append(sv('text', { x: B - R + 6, y: T - 12, class: 'achse' }, 'MBit/s'));
    for (let i = 0; i <= 4; i++) {
      const t = von + ((bis - von) * i) / 4;
      g.append(sv('text', { x: x(t), y: H - 6, class: 'achse', 'text-anchor': i === 0 ? 'start' : i === 4 ? 'end' : 'middle' }, uhr(t)));
    }
    const linie = (wert, y, attr) => {
      let d = '', vor = null;
      for (const q of p) {
        const w = q.verbunden ? wert(q) : null;
        if (w == null) { vor = null; continue; }
        d += `${vor != null && q.zeit - vor <= 30 ? 'L' : 'M'}${x(q.zeit).toFixed(1)} ${y(w).toFixed(1)}`;
        vor = q.zeit;
      }
      if (d) g.append(sv('path', { d, class: 'linie', ...attr }));
    };
    linie((q) => q.rate, yRate, { stroke: 'var(--gruen)', 'stroke-dasharray': '6 3' });
    linie((q) => q.rssi, ySig, { stroke: 'var(--accent)' });
    g.append(sv('text', { x: L - 6, y: Y1 + 28, class: 'achse', 'text-anchor': 'end' }, 'Ereignisse'));
    for (const e of a.ereignisse.filter((q) => q.zeit >= von)) {
      const [name, farbe, form] = ARTEN[e.art];
      const m = sv('g', { transform: `translate(${x(e.zeit).toFixed(1)} ${Y1 + 24})`, class: 'marke' });
      m.append(sv('title', {}, `${uhr(e.zeit)}: ${name}`), sv('path', { d: form, stroke: farbe, fill: e.art === 'Bandwechsel' ? farbe : 'none' }));
      g.append(sv('line', { x1: x(e.zeit), x2: x(e.zeit), y1: T, y2: Y1 + 14, class: 'marke-linie', stroke: farbe }), m);
    }
    return g;
  }

  function wlanKarte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const k = el('section', { class: 'gruppe wlan-karte', 'aria-labelledby': 'g-wlan' }, el('h3', { id: 'g-wlan', text: 'WLAN-Verlauf' }), el('div', { class: 'karte' }, ort));
    let timer = null;

    function zeichne(a) {
      if (!a.verbunden && a.proben.every((q) => !q.verbunden)) {
        return ort.replaceChildren(leerZustand(Netz.icons.qualitaet, 'Kein WLAN. Am Kabel oder bei ausgeschaltetem WLAN gibt es keinen Funkverlauf zu zeigen.'));
      }
      const b = a.besuch;
      const arten = [...new Set(a.ereignisse.map((e) => e.art))];
      ort.replaceChildren(...[
        status(a.verbunden ? 'gruen' : 'orange', a.verbunden ? `Verbunden · tastet alle 5 s ab${a.gemerkt ? ' · Besuch wird gespeichert' : ' · Netz nicht gemerkt, nichts wird gespeichert'}` : 'Gerade keine Funkverbindung'),
        wlanKurve(a),
        el('ul', { class: 'legende' },
          ...['Signal', 'Senderate'].map((n, i) => {
            const l = sv('svg', { viewBox: '0 0 28 8', class: 'muster', 'aria-hidden': 'true' });
            l.append(sv('line', { x1: 1, x2: 27, y1: 4, y2: 4, stroke: STIL[i].farbe, 'stroke-dasharray': STIL[i].dash, 'stroke-width': 2 }));
            return el('li', {}, l, el('span', { text: n }));
          }),
          ...arten.map((n) => {
            const l = sv('svg', { viewBox: '-7 -7 14 14', class: 'zeichen', 'aria-hidden': 'true' });
            l.append(sv('path', { d: ARTEN[n][2], stroke: ARTEN[n][1], fill: n === 'Bandwechsel' ? ARTEN[n][1] : 'none', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }));
            return el('li', {}, l, el('span', { text: ARTEN[n][0] }));
          })),
        b && gruppe('Dieser Besuch', [
          zeile('Signal', b.signal ? `Median ${b.signal.median} dBm (${b.signal.min} bis ${b.signal.max})` : '-'),
          zeile('Senderate', b.rate ? `Median ${fmt(b.rate.median, 0)} MBit/s (${fmt(b.rate.min, 0)} bis ${fmt(b.rate.max, 0)})` : '-'),
          zeile('Wechsel des Zugangspunkts', a.roaming_erkennbar ? String(b.wechsel) : 'nicht erkennbar', EV.marken),
          zeile('Abbrüche', String(b.abbrueche)),
        ]),
        a.vergleich && el('p', { class: 'wlan-vergleich', text: a.vergleich, title: EV.vergleich }),
        !a.roaming_erkennbar && meldung(EV.ohne_hash, 'orange'),
        a.hinweis && meldung(a.hinweis),
      ].filter(Boolean));
    }

    async function tick() {
      if (!k.isConnected) return; // view was left: the loop ends, nothing else to clean up
      if (document.hidden) return; // resumed by `sicht`
      try {
        zeichne(await Netz.invoke('wlan_verlauf_abtasten'));
      } catch (e) {
        ort.replaceChildren(meldung(Netz.text(e)));
      }
      if (k.isConnected) timer = setTimeout(tick, TAKT);
    }
    const sicht = () => {
      if (!k.isConnected) return document.removeEventListener('visibilitychange', sicht);
      clearTimeout(timer);
      if (!document.hidden) tick();
    };
    document.addEventListener('visibilitychange', sicht);
    setTimeout(tick, 100); // the caller attaches the view only after render() returned
    return k;
  }

  // ---- Karte „Paketgröße“ (`netzinfo mtu`, src/mtu.rs, issue #47) ----
  const EM = {
    karte: 'Jedes Netzgerät gibt nur Pakete bis zu einer Obergrenze weiter (MTU). Ich schicke Pings mit dem Zeichen „Nicht zerlegen“ und halbiere den Bereich von 576 bis 1500 Byte, bis die Grenze feststeht. Normal sind 1500 Byte; weniger heißt, dass etwas auf dem Weg Platz für eigene Kopfdaten braucht (DSL, Tunnel, VPN, Mobilfunk). Das ist eine Erklärung, kein Beweis.',
  };
  const MTU_AMPEL = { normal: 'gruen', pppoe: 'gruen', tunnel: 'gruen', klein: 'orange', ungewoehnlich: 'orange' };
  let mtuLaeuft = null; // { ziel, p }, outside the view like the path card: ⌘R must not start a second run
  let mtuLetzt = null; // { erg } | { fehler }

  function mtuKarte() {
    const eingabe = el('input', { type: 'text', class: 'ziel-feld', value: mtuLaeuft?.ziel ?? '1.1.1.1', 'aria-label': 'Ziel', placeholder: '1.1.1.1', autocapitalize: 'off', spellcheck: 'false' });
    const knopf = el('button', { class: 'knopf haupt', type: 'button', text: 'Paketgröße messen', onclick: messe });
    const lauf = el('p', { class: 'fuss fortschritt', 'aria-live': 'polite' });
    const ort = el('div', { 'aria-live': 'polite' });
    const k = el('section', { class: 'gruppe mtu-karte', 'aria-labelledby': 'g-mtu' },
      el('h3', { id: 'g-mtu', text: 'Paketgröße' }),
      el('div', { class: 'karte mtu' }, el('div', { class: 'werkzeuge' }, eingabe, knopf), lauf, ort,
        el('p', { class: 'erklaerung', text: EM.karte })));

    function zeichneMtu() {
      const laeuft = mtuLaeuft != null;
      knopf.disabled = eingabe.disabled = laeuft;
      lauf.replaceChildren(...(laeuft ? [el('progress', { 'aria-label': 'Messung läuft' }), ` Messe die Paketgröße zu ${mtuLaeuft.ziel} … das dauert bis zu 30 Sekunden.`] : []));
      if (laeuft) return ort.replaceChildren();
      if (!mtuLetzt) return ort.replaceChildren(leerZustand(Netz.icons.qualitaet, 'Noch nicht gemessen. „Paketgröße messen“ zeigt, wie große Pakete bis zum Ziel unzerlegt ankommen.'));
      if (mtuLetzt.fehler) return ort.replaceChildren(meldung(mtuLetzt.fehler));
      const e = mtuLetzt.erg;
      ort.replaceChildren(...[
        status(e.quelle === 'stille' ? 'orange' : MTU_AMPEL[e.einstufung], `${e.ziel} (${e.ziel_ip}) · ${e.pfad_mtu} Byte · ${zeit(e.zeitpunkt)}`),
        gruppe('Ergebnis', [
          zeile('Größtes Paket ohne Zerlegen', `${e.pfad_mtu} Byte`),
          e.lokale_mtu != null && zeile('Grenze deiner Schnittstelle', `${e.lokale_mtu} Byte`),
          e.netz && zeile('Gespeichert für', e.netz),
          e.vorher != null && zeile('Letzter Besuch', `${e.vorher} Byte`),
        ].filter(Boolean)),
        el('p', { class: 'mtu-satz', text: e.satz }),
        e.vorher != null && e.vorher !== e.pfad_mtu && meldung(`Beim letzten Besuch waren es ${e.vorher} Byte, jetzt ${e.pfad_mtu} Byte.`, 'orange'),
        !e.netz && meldung('Dieses Netz ist nicht gemerkt, es wird nichts gespeichert.', 'orange'),
      ].filter(Boolean));
    }

    async function messe() {
      if (mtuLaeuft) return;
      const ziel = eingabe.value.trim() || '1.1.1.1';
      const p = Netz.invoke('mtu_messen', { ziel }).then((erg) => ({ erg }), (f) => ({ fehler: Netz.text(f) }));
      mtuLaeuft = { ziel, p };
      zeichneMtu();
      mtuLetzt = await p;
      mtuLaeuft = null;
      if (k.isConnected) { zeichneMtu(); knopf.focus(); }
    }

    zeichneMtu();
    if (mtuLaeuft) mtuLaeuft.p.then(() => k.isConnected && zeichneMtu());
    return k;
  }

  // ---- Karte „DNS-Tempo“ (`netzinfo dns --tempo`, src/dnstempo.rs, issue #50) ----
  function dnsTempoKarte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'DNS vergleichen' });
    knopf.addEventListener('click', async () => {
      knopf.disabled = true;
      ort.replaceChildren(status('grau', 'Misst vier Server nacheinander, etwa 20 Sekunden'));
      try {
        const r = await Netz.invoke('dns_tempo');
        const hoechst = Math.max(...r.zeilen.flatMap((z) => [z.bekannt_p95, z.unbekannt_p95]).filter((v) => v != null), 1);
        const balken = (v) => { const b = el('span', { class: 'balken' }, el('span')); b.firstChild.style.width = `${v == null ? 0 : Math.max(2, (v / hoechst) * 100)}%`; return b; };
        ort.replaceChildren(
          el('ul', { class: 'dns-tempo' }, r.zeilen.map((z, i) => el('li', { class: i === r.empfehlung ? 'rat' : '' },
            el('span', { class: 'name', text: z.name }),
            el('span', { class: 'werte' }, balken(z.bekannt_median), el('span', { class: 'zahl', text: `bekannt ${ms(z.bekannt_median)}` })),
            el('span', { class: 'werte' }, balken(z.unbekannt_median), el('span', { class: 'zahl', text: `neu ${ms(z.unbekannt_median)}` })),
            el('span', { class: 'zahl', text: `${Math.round(z.fehlerrate * 100)} % Fehler` })))),
          status(r.empfehlung == null ? 'gruen' : 'orange', r.satz),
          el('p', { class: 'fuss', text: r.datenschutz }),
          el('p', { class: 'erklaerung', text: r.erklaerung }));
      } catch (e) {
        ort.replaceChildren(meldung(Netz.text(e)));
      }
      knopf.disabled = false;
    });
    return el('section', { class: 'gruppe dns-tempo-karte', 'aria-labelledby': 'g-dns-tempo' }, el('h3', { id: 'g-dns-tempo', text: 'DNS-Tempo' }),
      el('div', { class: 'karte' }, el('p', { class: 'fuss', text: 'Fragt je Server höchstens 60 Namen ab, nacheinander. Balken: Median, bekannte Namen und neue.' }), knopf, ort));
  }

  // ---- Karte „IPv6“ (`netzinfo ipv6`, src/ipv6.rs) ----
  const V6 = {
    kein_ipv6: ['grau', 'kein IPv6'], gleich: ['gruen', 'IPv6 nutzbar und gleich schnell'], langsamer: ['orange', 'IPv6 nutzbar, aber langsamer'],
    kaputt: ['rot', 'IPv6 kaputt'], unsicher: ['grau', 'unsicher'],
  };
  function ipv6Spalte(p) {
    if (!p.aufgeloest) return 'keine Adresse';
    return p.median_ms == null ? `scheitert (${p.fehler} von ${p.versuche})` : `${ms(p.median_ms)}${p.fehler ? `, ${p.fehler} von ${p.versuche} gescheitert` : ''}`;
  }
  function ipv6Karte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'IPv6 messen' });
    knopf.addEventListener('click', async () => {
      knopf.disabled = true;
      ort.replaceChildren(status('grau', 'Misst etwa 5 Sekunden an fünf Diensten'));
      try {
        const r = await Netz.invoke('ipv6_messen');
        const [ampel, wort] = V6[r.urteil];
        ort.replaceChildren(
          r.ziele.length ? el('table', { class: 'ipv6-tabelle' },
            el('thead', {}, el('tr', {}, el('th', { text: 'Dienst' }), el('th', { text: 'IPv4' }), el('th', { text: 'IPv6' }))),
            el('tbody', {}, r.ziele.map((z) => el('tr', {}, el('td', { text: z.name }), el('td', { text: ipv6Spalte(z.v4) }), el('td', { text: ipv6Spalte(z.v6) }))))) : '',
          el('p', { class: 'diagnose-urteil' }, el('span', { class: `punkt ${ampel}` }), el('strong', { text: wort })),
          el('p', { class: 'ipv6-satz', text: r.satz }));
      } catch (e) {
        ort.replaceChildren(meldung(Netz.text(e)));
      }
      knopf.disabled = false;
    });
    return el('section', { class: 'gruppe', 'aria-labelledby': 'g-ipv6' }, el('h3', { id: 'g-ipv6', text: 'IPv6' }), el('div', { class: 'karte ipv6-karte' }, knopf, ort));
  }

  // ---- Zeile „Uhrzeit“ (`netzinfo zeit`, src/zeit.rs, issue #69) ----
  function zeitKarte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const k = el('section', { class: 'zeit-karte' }, ort);
    const sek = (v) => `${v > 0 ? '+' : ''}${Math.abs(v) < 10000 ? `${v} ms` : `${fmt(v / 1000)} s`}`;
    ort.replaceChildren(gruppe('Uhrzeit', [zeile('Uhr', 'wird geprüft …')]));
    Netz.invoke('zeit_pruefen').then((e) => {
      ort.replaceChildren(gruppe('Uhrzeit', [
        zeile('Uhr', e.klasse ? { Harmlos: 'stimmt', Auffaellig: 'weicht ab', Alarm: 'Alarm' }[e.klasse] : 'nicht geprüft', 'Abweichung zur Uhr dieses Macs, gemessen per NTP und HTTPS-Date. Ich stelle die Uhr nie.'),
        ...e.befunde.map((b) => zeile(b.quelle, b.versatz_ms == null ? 'keine Antwort' : sek(b.versatz_ms))),
      ], e.satz));
    }, (f) => ort.replaceChildren(meldung(Netz.text(f))));
    return k;
  }

  // ---- Karte „Funkumfeld“ (`netzinfo wlan --umfeld`, src/kanal.rs): scannt nur auf Klick, speichert nichts ----
  const BAENDER = ['2,4 GHz', '5 GHz', '6 GHz'];
  function umfeldBalken(a) {
    if (a.belegung.length === 0) return meldung('Der Scan hat kein Netz gehört. Ohne Standortfreigabe liefert macOS manchmal nichts; erteile sie unter „Dieses Netz“.', 'orange');
    const eigene = (band, kanal) => a.beobachtungen.filter((b) => b.band === band && b.kanal === kanal).length;
    const hoechst = Math.max(...a.beobachtungen.map((b) => eigene(b.band, b.kanal)));
    const ziel = a.empfehlung.ziel;
    return el('div', {},
      el('p', { class: 'umfeld-summe', text: `${a.beobachtungen.length} Netze in Reichweite${a.land ? `, Regelgebiet ${a.land}` : ''}` }),
      BAENDER.map((band) => {
        const kanaele = [...new Set([...a.beobachtungen, a.aktuell, ziel].filter((k) => k?.band === band).map((k) => k.kanal))].sort((x, y) => x - y);
        return kanaele.length > 0 && el('div', { class: 'umfeld-band' }, el('h4', { text: band }),
          el('ul', { class: 'umfeld-balken' }, kanaele.map((kanal) => {
            const n = eigene(band, kanal);
            const stoert = a.belegung.find((l) => l.band === band && l.kanal === kanal);
            const du = a.aktuell?.band === band && a.aktuell.kanal === kanal;
            const rat = ziel?.band === band && ziel.kanal === kanal;
            const saeule = el('span', { class: 'saeule' });
            saeule.style.setProperty('--h', `${Math.round((n / hoechst) * 80)}px`);
            return el('li', { class: du ? 'jetzt' : rat ? 'rat' : '', title: `Kanal ${kanal}${du ? ' (dein Kanal)' : rat ? ' (empfohlen)' : ''}: ${n} Netze senden hier${stoert ? `, mit Nachbarkanälen ${stoert.netze} Netze, zusammen ${Math.round(stoert.dbm)} dBm` : ''}` },
              el('span', { class: 'zahl', text: String(n) }), saeule, el('span', { class: 'kanal', text: String(kanal) }));
          })));
      }),
      el('p', { class: 'umfeld-legende', text: 'Zahl = Netze auf genau diesem Kanal. Der Rat zählt im 2,4 GHz Band die Nachbarkanäle mit, weil sie sich überlappen. Blau: dein Kanal, grün: empfohlen.' }),
      el('p', { class: 'umfeld-rat', text: a.empfehlung.text }));
  }

  function umfeldKarte() {
    const ort = el('div', { 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Umgebung ansehen' });
    knopf.addEventListener('click', async () => {
      knopf.disabled = true;
      ort.replaceChildren(status('grau', 'Scannt etwa 5 Sekunden, die Verbindung kann kurz haken'));
      try {
        ort.replaceChildren(umfeldBalken(await Netz.invoke('wlan_umfeld')));
      } catch (e) {
        ort.replaceChildren(meldung(Netz.text(e)));
      }
      knopf.disabled = false;
    });
    return el('section', { class: 'gruppe umfeld-karte', 'aria-labelledby': 'g-umfeld' }, el('h3', { id: 'g-umfeld', text: 'Funkumfeld' }),
      el('div', { class: 'karte' }, el('p', { class: 'umfeld-hinweis', text: 'Zählt Netze je Kanal, ohne Namen. Von den Nachbarn wird nichts gespeichert. Der Scan läuft nur auf Klick.' }), knopf, ort));
  }

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
        const unbekannt = e?.art === 'nicht_gemerkt';
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
      wurzel.append(diagnoseKarte());
      zeichne();
      wurzel.append(wegKarte());
      wurzel.append(wlanKarte());
      wurzel.append(mtuKarte());
      wurzel.append(ipv6Karte());
      wurzel.append(dnsTempoKarte());
      wurzel.append(zeitKarte());
      wurzel.append(umfeldKarte());
      if (weiter) schalte(true);
    },
  });
})();
