// Gerüst der App: Seitenleiste, Werkzeugleiste, Erklärmodus und die kleinen Bausteine,
// die alle Ansichten teilen. Jede Ansicht liegt in ansichten/<name>.js und meldet sich mit
// Netz.registriere({ id, titel, icon, render(wurzel), aktualisieren? }) an. Welche Ansicht in welchem
// Abschnitt der Seitenleiste steht, sagt die Tabelle `abschnitte`; was dort fehlt, landet unten ohne Titel.
(() => {
  const abschnitte = [
    ['Netz', ['dieses-netz', 'gemerkte-netze', 'pruefung']],
    ['Messen', ['qualitaet', 'live']],
    ['Geräte und Mac', ['geraete', 'verbindungen', 'offen']],
  ];
  const ansichten = [];
  let aktuell = null;
  let reihe = [];
  let zaehler = 0;

  const invoke = (befehl, args) => window.__TAURI__.core.invoke(befehl, args);

  // Kleiner Elementbauer. `html` nur für die festen Icons unten, nie für Daten.
  function el(tag, props = {}, ...kinder) {
    const e = document.createElement(tag);
    for (let [k, v] of Object.entries(props)) {
      // ARIA wants the words true and false, not a present or missing attribute.
      if (k.startsWith('aria-') && typeof v === 'boolean') v = String(v);
      if (v == null || v === false) continue;
      if (k === 'class') e.className = v;
      else if (k === 'text') e.textContent = v;
      else if (k === 'html') e.innerHTML = v;
      else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
      else e.setAttribute(k, v === true ? '' : v);
    }
    e.append(...kinder.flat().filter((k) => k != null && k !== false));
    return e;
  }

  const svg = (pfade) =>
    `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${pfade}</svg>`;
  const icons = {
    wlan: svg('<path d="M1.8 6a8.5 8.5 0 0 1 12.4 0M4 8.5a5.4 5.4 0 0 1 8 0M6.2 11a2.3 2.3 0 0 1 3.6 0"/><circle cx="8" cy="13" r=".6" fill="currentColor"/>'),
    merken: svg('<path d="M4 2.5h8v11L8 10.5l-4 3z"/>'),
    pruefung: svg('<path d="M8 1.8 13 3.6v4.2c0 3-2 5.2-5 6.4-3-1.2-5-3.4-5-6.4V3.6z"/><path d="m5.8 8 1.7 1.7L10.4 6.4"/>'),
    qualitaet: svg('<path d="M1.5 8.5h3l2-5 3 9 2-4h3"/>'),
    geraete: svg('<rect x="2" y="3" width="12" height="8" rx="1.2"/><path d="M5.5 13.5h5M8 11v2.5"/>'),
  };

  // Zeit in der Zeitzone des Macs, z. B. „26.10.2026, 21:30“.
  const zeit = (sekunden) =>
    Number.isFinite(sekunden)
      ? new Date(sekunden * 1000).toLocaleString('de-DE', { dateStyle: 'medium', timeStyle: 'short' })
      : 'unbekannt';

  // Fehler aus dem Kern sind deutsche Sätze, entweder pur oder als { art, text } (art: nicht_gemerkt,
  // nicht_meins, sonstiges). Ansichten fragen `f?.art`, nie den Satz. Alles andere ist ein Fehler der Oberfläche.
  const text = (fehler) =>
    typeof fehler === 'string' ? fehler : typeof fehler?.text === 'string' ? fehler.text
      : 'In der Oberfläche ist etwas schiefgegangen. Starte die App neu.';

  // Core sentences name CLI commands in `backticks`: show them as code, not as raw marks.
  const mitCode = (satz) => satz.split('`').map((t, i) => (i % 2 ? el('code', { text: t }) : t));

  const meldung = (satz, farbe = 'rot') =>
    el('p', { class: 'meldung', role: 'alert' }, el('span', { class: `punkt ${farbe}` }), el('span', {}, mitCode(satz)));

  // Fehler mit Ausweg: Satz plus „Erneut versuchen“ (lädt die Ansicht neu). `nochmal` ersetzt das Neuladen, z. B. für eine einzelne Messung.
  const fehlerKarte = (satz, nochmal) =>
    el('div', { class: 'fehler-karte' }, meldung(satz, 'rot'),
      el('button', {
        class: 'knopf', type: 'button', text: 'Erneut versuchen',
        // Zeichnet der Versuch den Inhalt neu, hängt der Fokus sonst am entfernten Knopf.
        onclick: async () => { await (nochmal ?? neuLaden)(); if (document.activeElement === document.body) zumKopf(); },
      }));

  const status = (farbe, wort) =>
    el('p', { class: 'status' }, el('span', { class: `punkt ${farbe}` }), el('span', { text: wort }));

  // Gruppe wie in den Systemeinstellungen: Titel, abgerundete Liste, optional eine Fußnote.
  let gruppenNr = 0;
  function gruppe(titel, zeilen, fuss) {
    // Die Nummer hält die id eindeutig, auch wenn zwei Gruppen denselben Titel tragen.
    const id = `g-${titel.toLowerCase().replace(/[^a-zäöüß]+/g, '-')}-${++gruppenNr}`;
    return el('section', { class: 'gruppe', 'aria-labelledby': id },
      el('h3', { id, text: titel }),
      el('dl', { class: 'liste' }, zeilen),
      fuss && el('p', { class: 'fuss', text: fuss }));
  }

  // Eine Zeile: Name links, Wert rechts, darunter der Satz für den Erklärmodus.
  // `wert` leer = noch unbekannt (gedämpft). `erklaerung` ist ein Satz.
  function zeile(label, wert, erklaerung) {
    const leer = wert == null || wert === '';
    return el('div', { class: 'zeile' },
      el('dt', { class: 'label', text: label }),
      el('dd', { class: `wert${leer ? ' leer' : ''}`, text: leer ? 'unbekannt' : wert }),
      erklaerung && el('dd', { class: 'erklaerung', text: erklaerung }));
  }

  // `bild` (optional) ist ein Name aus ui/bilder.js und ersetzt das kleine Symbol durch ein Strichbild.
  function leerZustand(icon, satz, bild) {
    const gross = bild && window.NETZ_BILDER?.[bild];
    return el('div', { class: 'leer-zustand' },
      gross ? el('span', { class: 'leer-bild', html: gross }) : el('span', { html: icon }),
      el('p', { text: satz }));
  }

  // „Woher?“: Aktion einer Zeile, lädt die Startkette des Prozesses erst beim Aufklappen (#56).
  // Gibt [Knopf, Feld] zurück; beide als Kinder der Zeile einsetzen (el() flacht Listen).
  let woherNr = 0;
  const QUELLE = { Terminal: 'Terminal', Systemdienst: 'Systemdienst', Anmeldeobjekt: 'Anmeldeobjekt', Unbekannt: 'unbekannt' };
  function woher(pid, programm) {
    const id = `woher-${++woherNr}`;
    const inhalt = el('div', { class: 'woher-inhalt', id, hidden: true });
    const knopf = el('button', { class: 'woher-knopf', type: 'button', text: 'Woher?', 'aria-expanded': 'false', 'aria-controls': id, 'aria-label': `Woher kommt ${programm || `PID ${pid}`}?` });
    knopf.addEventListener('click', async () => {
      const auf = inhalt.hidden;
      inhalt.hidden = !auf;
      knopf.setAttribute('aria-expanded', String(auf));
      if (!auf || inhalt.childElementCount) return;
      try {
        const h = await invoke('woher_zeigen', { pid });
        inhalt.append(
          el('div', { class: 'fein', text: `Quelle: ${QUELLE[h.quelle] ?? 'unbekannt'}` }),
          el('ul', { class: 'woher-kette' }, [...h.kette].reverse().map((p, i) =>
            el('li', { class: 'num', style: `margin-left:${i * 14}px`, text: `${p.name} (PID ${p.pid})` }))),
          el('p', { class: 'fein', text: h.satz }));
      } catch (f) {
        inhalt.append(meldung(text(f), 'rot'));
      }
    });
    return [knopf, inhalt];
  }

  // Löschen mit Bestätigung: Frage, „Abbrechen“ (auch Escape) und der rote Knopf. `aktion` darf werfen,
  // dann zeigt `fehler(f)` den Satz und der Knopf wird wieder frei. Der Aufrufer setzt das Ergebnis
  // in seine Zeile und baut bei `abbrechen` die alte Zeile zurück. Der Fokus landet auf „Abbrechen“.
  function bestaetigung({ frage, label, aria, knopf = 'Löschen', aktion, abbrechen, fehler }) {
    const nein = el('button', { class: 'knopf', type: 'button', text: 'Abbrechen', onclick: abbrechen });
    const ja = el('button', { class: 'knopf gefahr', type: 'button', text: knopf, 'aria-label': aria });
    ja.addEventListener('click', async () => {
      ja.disabled = true;
      try { await aktion(); } catch (f) { fehler(f); ja.disabled = false; }
    });
    queueMicrotask(() => nein.focus());
    return el('div', {
      class: 'bestaetigung', role: 'group', 'aria-label': label,
      onkeydown: (e) => { if (e.key === 'Escape') { e.stopPropagation(); abbrechen(); } },
    }, el('p', { text: frage }), nein, ja);
  }

  // ---- Seitenleiste ----
  function baueMenue() {
    const menue = document.getElementById('menue');
    const eintrag = (a) => el('button', {
      class: 'eintrag', role: 'tab', id: `tab-${a.id}`, 'data-id': a.id,
      'aria-selected': 'false', tabindex: '-1',
      // Ein Klick holt den Fokus in die Seitenleiste (WebKit tut das bei Knöpfen nicht), sonst gehen die Pfeiltasten ins Leere.
      onclick: () => zeige(a.id, { fokus: true }),
      title: `${a.titel} (⌘${reihe.indexOf(a) + 1})`, 'aria-keyshortcuts': `Meta+${reihe.indexOf(a) + 1}`,
    }, el('span', { html: a.icon }), el('span', { text: a.titel }));
    const rest = new Set(ansichten);
    const bloecke = abschnitte.map(([titel, ids]) => {
      const mit = ids.map((id) => ansichten.find((a) => a.id === id)).filter(Boolean);
      mit.forEach((a) => rest.delete(a));
      return [titel, mit];
    });
    bloecke.push([null, [...rest]]);
    // Pfeiltasten gehen in der Reihenfolge der Seitenleiste, nicht der Anmeldung.
    reihe = bloecke.flatMap(([, mit]) => mit);
    for (const [titel, mit] of bloecke) {
      if (!mit.length) continue;
      menue.append(el('div', { class: 'abschnitt', role: 'presentation' },
        titel && el('div', { class: 'abschnitt-titel', 'aria-hidden': 'true', text: titel }),
        mit.map(eintrag)));
    }
    menue.addEventListener('keydown', (e) => {
      const schritt = { ArrowDown: 1, ArrowUp: -1, Home: -Infinity, End: Infinity }[e.key];
      if (!schritt || e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      const i = reihe.findIndex((a) => a.id === aktuell);
      const n = Math.max(0, Math.min(reihe.length - 1, i + schritt));
      zeige(reihe[n].id, { fokus: true });
    });
  }

  // ---- Ansicht anzeigen ----
  async function zeige(id, { fokus = false, neuLaden = false } = {}) {
    const a = ansichten.find((x) => x.id === id);
    if (!a) return;
    const wechsel = id !== aktuell;
    aktuell = id;
    for (const b of document.querySelectorAll('.eintrag')) {
      const an = b.dataset.id === id;
      b.setAttribute('aria-selected', an);
      b.tabIndex = an ? 0 : -1;
      if (an && fokus) b.focus();
    }
    document.getElementById('titel').textContent = a.titel;
    const feld = document.getElementById('scroll');
    const knopf = document.getElementById('aktualisieren');
    // Fokus merken, bevor „Lädt …“ oder das Sperren des Knopfs ihn wegnehmen.
    const imInhalt = feld.contains(document.activeElement);
    const amKnopf = document.activeElement === knopf;
    // A view that owns ⌘R itself (aktualisieren: false) hides the shared button.
    document.getElementById('aktualisieren').hidden = a.aktualisieren === false;
    const nr = ++zaehler;
    // data-ansicht scopes a view's CSS file: all views share one document.
    const wurzel = el('div', { class: `ansicht${wechsel ? ' neu' : ''}`, 'data-ansicht': id });
    feld.setAttribute('aria-busy', 'true');
    // Dauert das Laden, steht beim Wechsel nicht die alte Ansicht unter dem neuen Titel.
    knopf.disabled = true;
    const wartet = wechsel && setTimeout(() => {
      if (nr === zaehler) feld.replaceChildren(el('div', { class: 'ansicht' }, leerZustand('', 'Lädt …')));
    }, 250);
    try {
      await a.render(wurzel);
    } catch (e) {
      console.error(e);
      wurzel.replaceChildren(fehlerKarte(text(e)));
    }
    clearTimeout(wartet);
    if (nr !== zaehler) return; // inzwischen woanders hingeklickt
    knopf.disabled = false;
    const oben = wechsel || !neuLaden ? 0 : feld.scrollTop;
    feld.replaceChildren(wurzel);
    feld.scrollTop = oben;
    feld.removeAttribute('aria-busy');
    // Wo der Fokus im alten Inhalt stand, steht er jetzt an der Überschrift des neuen (nicht im Nichts).
    if (imInhalt || feld.contains(document.activeElement)) zumKopf();
    else if (amKnopf) knopf.focus();
  }

  // Fokus an die Überschrift der Ansicht (tabindex -1), sonst an „Erneut versuchen“, sonst ans Inhaltsfeld.
  function zumKopf() {
    const feld = document.getElementById('scroll');
    const ziel = feld.querySelector('h2') ?? feld.querySelector('.fehler-karte .knopf') ?? feld;
    if (!ziel.matches('button')) ziel.tabIndex = -1;
    ziel.focus({ preventScroll: true });
  }

  const neuLaden = () => zeige(aktuell, { neuLaden: true });

  // ---- Erklärmodus ----
  function erklaermodus() {
    const knopf = document.getElementById('erklaeren');
    const setze = (an) => {
      document.body.classList.toggle('erklaeren', an);
      knopf.setAttribute('aria-checked', an);
      try { localStorage.setItem('netzinfo.erklaeren', an ? '1' : '0'); } catch { /* ohne Speicher bleibt er pro Start */ }
    };
    let an = false;
    try { an = localStorage.getItem('netzinfo.erklaeren') === '1'; } catch { /* s. o. */ }
    setze(an);
    knopf.addEventListener('click', () => setze(knopf.getAttribute('aria-checked') !== 'true'));
  }

  function start() {
    // Only events, no initial check: unknown focus (e.g. headless Chrome) looks active.
    for (const ev of ['focus', 'blur']) window.addEventListener(ev, () => document.documentElement.classList.toggle('inaktiv', ev === 'blur'));
    baueMenue();
    erklaermodus();
    document.getElementById('aktualisieren').addEventListener('click', neuLaden);
    document.addEventListener('keydown', (e) => {
      if (!e.metaKey || e.shiftKey || e.altKey) return;
      // ⌘1 bis ⌘8: Ansichten in der Reihenfolge der Seitenleiste.
      if (/^[1-8]$/.test(e.key) && reihe[e.key - 1]) { e.preventDefault(); zeige(reihe[e.key - 1].id, { fokus: !!document.activeElement.closest('#menue') }); }
      else if (e.key.toLowerCase() === 'r') { e.preventDefault(); neuLaden(); }
    });
    zeige(reihe[0].id);
  }

  window.Netz = {
    invoke, el, icons, zeit, text, meldung, fehlerKarte, status, gruppe, zeile, leerZustand, neuLaden, zumKopf, woher, bestaetigung,
    registriere: (a) => ansichten.push(a),
  };
  document.addEventListener('DOMContentLoaded', start);
})();
