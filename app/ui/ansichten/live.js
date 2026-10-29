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
  const komma = (z) => z.replace('.', ',');
  const ms = (v) => (v == null ? '–' : komma(`${v.toFixed(v < 10 ? 1 : 0)} ms`));

  const sv = (tag, attr) => {
    const e = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attr)) e.setAttribute(k, v);
    return e;
  };

  // Pool: genau n Kinder gleichen Typs in g, vorhandene werden wiederverwendet statt neu gebaut.
  const pool = (g, n, tag, cls) => {
    while (g.childNodes.length < n) g.append(sv(tag, { class: cls }));
    while (g.childNodes.length > n) g.lastChild.remove();
    return [...g.children];
  };

  // Kurve, Band P5 bis P95 (nur über das Kennzahlenfenster der letzten 60 s) und rote Striche für Verlust.
  // Das Gerüst (Achsen, Gitter) entsteht einmal; `zeichne(a)` setzt danach nur Zahlen und Pfade.
  function kurve(label) {
    const B = 600, H = 190, L = 50, U = 28;
    const s = sv('svg', { viewBox: `0 0 ${B} ${H}`, class: 'kurve', role: 'img', 'aria-label': label });
    for (const [px, anker, wort] of [[L, 'start', 'vor 5 min'], [B - 8, 'end', 'jetzt']]) {
      const z = sv('text', { x: px, y: H - 2, 'text-anchor': anker, class: 'achse' });
      z.textContent = wort;
      s.append(z);
    }
    const y = (v, max) => H - U - (v / max) * (H - U - 12);
    const beschriftung = [0, 0.5, 1].map((t) => {
      const yy = y(t, 1);
      s.append(sv('line', { x1: L, x2: B - 4, y1: yy, y2: yy, class: 'gitter' }));
      const z = sv('text', { x: L - 6, y: yy + 4, 'text-anchor': 'end', class: 'achse' });
      s.append(z);
      return z;
    });
    const band = s.appendChild(sv('rect', { class: 'band' }));
    const linien = s.appendChild(sv('g', {}));
    const striche = s.appendChild(sv('g', {}));
    s.zeichne = (a) => {
      const jetzt = a.punkte.length ? a.punkte[a.punkte.length - 1].zeit : 0;
      const werte = a.punkte.map((p) => p.ms).filter((v) => v != null);
      // Glatte Achse (50, 100, 200 …), damit die Beschriftung nicht bei jedem Takt springt.
      const hoechst = Math.max(0, ...werte) * 1.05;
      const max = [50, 100, 200, 500, 1000, 2000, 5000].find((g) => g >= hoechst) ?? hoechst;
      const x = (t) => L + ((t - (jetzt - FENSTER_MS)) / FENSTER_MS) * (B - L - 4);
      [0, 0.5, 1].forEach((t, i) => { const v = max * t; beschriftung[i].textContent = t === 1 ? `${Math.round(v)} ms` : Math.round(v); });
      const k = a.kennzahlen;
      band.toggleAttribute('hidden', !(k && k.p5_ms != null));
      if (k && k.p5_ms != null) {
        band.setAttribute('x', x(jetzt - 60000));
        band.setAttribute('y', y(k.p95_ms, max));
        band.setAttribute('width', x(jetzt) - x(jetzt - 60000));
        band.setAttribute('height', Math.max(2, y(k.p5_ms, max) - y(k.p95_ms, max)));
      }
      // Eine Linie je Strecke ohne Verlust; ein Verlust unterbricht sie.
      const teile = [[]];
      for (const p of a.punkte) {
        if (p.ms == null) teile.push([]);
        else teile[teile.length - 1].push(`${x(p.zeit).toFixed(1)} ${y(p.ms, max).toFixed(1)}`);
      }
      const nichtLeer = teile.filter((t) => t.length);
      pool(linien, nichtLeer.length, 'path', 'linie').forEach((e, i) => e.setAttribute('d', `M${nichtLeer[i].join(' L')}`));
      const weg = a.punkte.filter((p) => p.ms == null);
      pool(striche, weg.length, 'line', 'verlust').forEach((e, i) => {
        e.setAttribute('x1', x(weg[i].zeit)); e.setAttribute('x2', x(weg[i].zeit));
        e.setAttribute('y1', H - U + 3); e.setAttribute('y2', H - U + 9);
      });
    };
    s.zeichne({ punkte: [] });
    return s;
  }

  const sek = (t) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, '0')}`;

  // Vor dem ersten Takt: dieselbe Fläche mit Achsen, ohne Kurve, dazu ein Satz, was Start tut.
  const leereFlaeche = () => el('div', { class: 'karte leer-karte' }, kurve('Leeres Diagramm, Antwortzeit der letzten 5 Minuten'),
    el('p', { class: 'leer-text', text: 'Noch keine Messung. Start sendet jede Sekunde ein Ping an deine Ziele und zeichnet die Antwortzeit hier auf. Gemessen wird nur, solange diese Ansicht offen ist.' }));

  const setzeWert = (dd, v) => { dd.textContent = v == null || v === '' ? 'unbekannt' : v; dd.classList.toggle('leer', v == null || v === ''); };
  const setzeAmpel = (p, st) => { p.firstChild.className = `punkt ${FARBE[st]}`; p.lastChild.textContent = WORT[st]; };

  // Die Karte eines Ziels entsteht einmal; `setze(a)` aktualisiert nur Werte. So bleibt der Fokus, VoiceOver
  // springt nicht, und die Kurve zuckt nicht. `melde` sagt einen Wechsel einmal an (nicht die Zahlen).
  function karte(nr, melde) {
    const kachel = (name, erkl) => {
      const gross = el('dd', { class: 'gross', text: '–' });
      return [el('div', { class: 'kachel' }, el('dt', { class: 'label', text: name }), gross, erkl && el('dd', { class: 'erklaerung', text: erkl })), gross];
    };
    const [kAktuell, tAktuell] = kachel('Aktuell');
    const [kJitter, tJitter] = kachel('Jitter (60 s)', E.jitter);
    const [kVerlust, tVerlust] = kachel('Verlust (60 s)', E.verlust);
    const diagramm = kurve('');
    const antwortlos = status('rot', 'Antwortet nicht');
    const legende = [
      el('li', {}, el('span', { class: 'muster linie-muster' }), 'Antwortzeit in ms'),
      el('li', {}, el('span', { class: 'muster band-muster' }), 'P5 bis P95 (60 s)'),
    ];
    const ampeln = ['Videoanrufe', 'Spiele', 'Streaming'].map((n) => [n, status('gruen', 'gut')]);
    const eignungKopf = el('h3', { class: 'unter eignung', text: 'Eignung' });
    const eignungListe = el('dl', { class: 'liste eignung' }, ampeln.map(([n, st]) => el('div', { class: 'zeile' },
      el('dt', { class: 'label', text: n }), el('dd', { class: 'wert' }, st))));
    const zMedian = zeile('Median'), zP95 = zeile('P95'), zMax = zeile('Maximum');
    const fuss = el('p', { class: 'fuss' });
    const h = el('h3', { id: `live-ziel-${nr}` });
    const knoten = el('section', { class: 'gruppe live-ziel', 'aria-labelledby': h.id },
      h, antwortlos,
      el('dl', { class: 'kacheln' }, kAktuell, kJitter, kVerlust),
      el('div', { class: 'karte' }, diagramm,
        el('ul', { class: 'legende' }, legende, el('li', {}, el('span', { class: 'muster verlust-muster' }), 'Verlust'))),
      eignungKopf, eignungListe,
      el('h3', { class: 'unter', text: 'Letzte 60 Sekunden' }),
      el('dl', { class: 'liste' }, zMedian, zP95, zMax), fuss);
    let vorher = null; // Eignung und „antwortet nicht“ der letzten Karte, für die einmalige Ansage

    const setze = (a) => {
      const k = a.kennzahlen;
      const tot = !!k && k.verlust_prozent >= 100;
      const letzter = a.punkte[a.punkte.length - 1];
      const kopf = a.ziel === a.ziel_ip ? a.ziel : `${a.ziel} · ${a.ziel_ip}`;
      if (h.textContent !== kopf) h.textContent = kopf;
      antwortlos.hidden = !tot;
      legende[0].hidden = legende[1].hidden = tot;
      const bild = tot
        ? `Antwortzeit zu ${a.ziel}: keine Antwort, die Kurve zeigt nur Verluststriche.`
        : `Kurve der Antwortzeit zu ${a.ziel} in den letzten 5 Minuten. Die aktuellen Zahlen stehen in den Kacheln und in der Liste darunter.`;
      if (diagramm.getAttribute('aria-label') !== bild) diagramm.setAttribute('aria-label', bild);
      diagramm.zeichne(a);
      tAktuell.textContent = !letzter ? '–' : letzter.ms == null ? 'Verlust' : ms(letzter.ms);
      tJitter.textContent = k ? ms(k.jitter_ms) : '–';
      tVerlust.textContent = k ? komma(`${k.verlust_prozent.toFixed(1)} %`) : '–';
      eignungKopf.hidden = eignungListe.hidden = !a.eignung;
      const st = a.eignung ? [a.eignung.anruf, a.eignung.spiel, a.eignung.streaming] : null;
      if (st) ampeln.forEach(([, p], i) => setzeAmpel(p, st[i]));
      setzeWert(zMedian.querySelector('dd'), k && ms(k.median_ms));
      setzeWert(zP95.querySelector('dd'), k && ms(k.p95_ms));
      setzeWert(zMax.querySelector('dd'), k && ms(k.max_ms));
      fuss.textContent = !k ? 'Noch keine Antwort.' : tot
        ? `Keine Antwort auf ${k.gesendet} ${k.gesendet === 1 ? 'Paket' : 'Pakete'}. Prüfe die Verbindung; manche Ziele beantworten Ping nicht.`
        : `${k.gesendet} ${k.gesendet === 1 ? 'Paket' : 'Pakete'} gesendet.`;
      // Angesagt wird nur ein Wechsel, nie die sekündliche Zahl.
      if (tot && !vorher?.tot) melde(`${a.ziel} antwortet nicht.`);
      else if (st && vorher?.st) {
        const neu = ['Videoanrufe', 'Spiele', 'Streaming'].map((n, i) => (st[i] !== vorher.st[i] ? `${n} ${WORT[st[i]]}` : null)).filter(Boolean);
        if (neu.length) melde(`${a.ziel}: ${neu.join(', ')}.`);
      }
      vorher = { tot, st };
    };
    return { knoten, setze };
  }

  Netz.registriere({
    id: 'live',
    teil: true,
    aktualisieren: false, // Die Ansicht misst von selbst, ⌘R hätte nichts zu tun.
    titel: 'Live',
    icon: Netz.icons.live,
    render: (wurzel) => bauen(wurzel),
  });
  Netz.live = { bauen };

  // `kompakt` (aus tempo.js): ohne Kopf und Erklärgruppe, pro Ziel Kennzahlen, Kurve und Eignung; die Liste der letzten 60 s fehlt.
  async function bauen(wurzel, kompakt) {
      if (kompakt) wurzel.classList.add('kompakt');
      const HINWEIS = 'Name oder Adresse, mehrere mit Leerzeichen getrennt, bis zu vier.';
      let laeuft = false, nr = 0, t0Lauf = 0, messungen = 0;
      const feld = el('input', { type: 'text', id: 'live-ziel', class: 'ziel-feld', value: '1.1.1.1', placeholder: '1.1.1.1 9.9.9.9', autocapitalize: 'off', spellcheck: 'false', 'aria-describedby': 'live-lauf' });
      const knopf = el('button', { class: kompakt ? 'knopf' : 'knopf haupt', type: 'button', text: 'Start', 'aria-label': 'Start: Messung beginnen' });
      const ort = el('div', { class: 'ergebnis' }, leereFlaeche());
      // Einzige Ansage-Region: nur Wechsel der Eignung oder „antwortet nicht“, einmal, nie die Sekundenwerte.
      const ansage = el('p', { class: 'nur-vorlesen', role: 'status', 'aria-live': 'polite' });
      let karten = [], gebaut = null;
      const zeichne = (antworten) => {
        const schluessel = antworten.map((a) => a.ziel).join(' ');
        if (schluessel !== gebaut) { // nur bei neuen Zielen bauen; sonst werden Werte gesetzt
          karten = antworten.map((_, i) => karte(i, (satz) => { ansage.textContent = satz; }));
          ort.replaceChildren(...karten.map((k) => k.knoten));
          gebaut = schluessel;
        }
        antworten.forEach((a, i) => karten[i].setze(a));
      };
      const laufText = el('p', { class: 'fuss', id: 'live-lauf', text: HINWEIS });
      const zeit = () => `${sek((Date.now() - t0Lauf) / 1000)} · ${messungen} Messungen`;
      // Gleiche Ziele nur einmal: Sie teilen sich eine Serie und würden sich gegenseitig stören.
      const alle = () => [...new Set(feld.value.split(/[\s,]+/).filter(Boolean))];
      const ziele = () => alle().slice(0, 4);

      const takt = async (neu, lauf) => {
        if (lauf !== nr) return; // gestoppt oder neu gestartet
        // Nur messen, solange die Ansicht im Dokument hängt; verborgenes Fenster setzt nur aus.
        if (!wurzel.isConnected) return stopp();
        const t0 = Date.now();
        if (document.hidden) laufText.textContent = `Pausiert, solange das Fenster verborgen ist (${zeit()})`;
        else {
          try {
            const antworten = await Promise.all(ziele().map((z) => invoke('live_sonde', { ziel: z, neu })));
            if (lauf !== nr) return;
            zeichne(antworten);
            messungen++;
            laufText.textContent = `Läuft seit ${zeit()}`;
          } catch (f) {
            if (lauf !== nr) return;
            stopp();
            gebaut = null;
            ort.replaceChildren(meldung(Netz.text(f), 'rot'));
            return knopf.focus();
          }
        }
        setTimeout(() => takt(false, lauf), Math.max(0, 1000 - (Date.now() - t0)));
      };
      const stopp = () => {
        if (laeuft) laufText.textContent = messungen ? `Gestoppt nach ${zeit()}` : HINWEIS;
        laeuft = false; nr++; knopf.textContent = 'Start'; knopf.setAttribute('aria-label', 'Start: Messung beginnen'); feld.disabled = false;
      };
      const start = () => {
        if (!ziele().length) { laufText.textContent = 'Gib mindestens ein Ziel ein, zum Beispiel 1.1.1.1.'; return feld.focus(); }
        laeuft = true; feld.disabled = true; knopf.textContent = 'Stopp'; knopf.setAttribute('aria-label', 'Stopp: Messung beenden'); gebaut = null; ansage.textContent = '';
        knopf.focus(); // das Feld ist jetzt gesperrt (auch nach Enter im Feld): der Fokus bleibt am Knopf
        t0Lauf = Date.now(); messungen = 0; laufText.textContent = alle().length > 4 ? 'Startet, gemessen werden die ersten vier Ziele …' : 'Startet, warte auf die erste Antwort …';
        takt(true, ++nr);
      };
      knopf.addEventListener('click', () => (laeuft ? stopp() : start()));
      feld.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !laeuft) { e.preventDefault(); start(); } }); // preventDefault: Enter löst sonst am neu fokussierten Knopf einen Klick aus
      wurzel.addEventListener('keydown', (e) => { if (e.key === 'Escape' && laeuft) { stopp(); knopf.focus(); } });

      wurzel.append(
        kompakt ? '' : el('div', { class: 'kopf' }, el('h2', { text: 'Live' }),
          el('p', { class: 'status' }, el('span', { text: 'Antwortzeit, Jitter und Verlust zu bis zu vier Zielen, jede Sekunde.' }))),
        el('section', { class: 'gruppe steuerung' }, el('div', { class: 'liste werkzeuge' }, el('label', { class: 'label', for: 'live-ziel', text: 'Ziele' }), feld, knopf), laufText),
        ort, ansage,
        kompakt ? '' : Object.assign(gruppe('So wird gemessen', [
          zeile('Messung', 'Ping (ICMP Echo), jede Sekunde', E.ping),
          zeile('Eignung', 'Faustwerte, keine Norm', E.ampel),
        ]), { className: 'gruppe erklaer-gruppe' }));
      // Beim Verlassen stoppt der Takt von selbst: die Wurzel hängt dann nicht mehr im Dokument.
  }
})();
