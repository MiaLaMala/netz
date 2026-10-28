// Gerüst der App: Seitenleiste, Werkzeugleiste, Erklärmodus und die kleinen Bausteine,
// die alle Ansichten teilen. Jede Ansicht liegt in ansichten/<name>.js und meldet sich mit
// Netz.registriere({ id, titel, icon, render(wurzel) }) an. Die Reihenfolge der Script-Tags
// in index.html ist die Reihenfolge in der Seitenleiste.
(() => {
  const ansichten = [];
  let aktuell = null;
  let zaehler = 0;

  const invoke = (befehl, args) => window.__TAURI__.core.invoke(befehl, args);

  // Kleiner Elementbauer. `html` nur für die festen Icons unten, nie für Daten.
  function el(tag, props = {}, ...kinder) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
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
    new Date(sekunden * 1000).toLocaleString('de-DE', { dateStyle: 'medium', timeStyle: 'short' });

  // Fehler aus dem Kern sind deutsche Sätze. Alles andere ist ein Fehler der Oberfläche.
  const text = (fehler) =>
    typeof fehler === 'string' ? fehler : 'In der Oberfläche ist etwas schiefgegangen. Starte die App neu.';

  const meldung = (satz, farbe = 'rot') =>
    el('p', { class: 'meldung', role: 'alert' }, el('span', { class: `punkt ${farbe}` }), el('span', { text: satz }));

  const status = (farbe, wort) =>
    el('p', { class: 'status' }, el('span', { class: `punkt ${farbe}` }), el('span', { text: wort }));

  // Gruppe wie in den Systemeinstellungen: Titel, abgerundete Liste, optional eine Fußnote.
  function gruppe(titel, zeilen, fuss) {
    const id = `g-${titel.toLowerCase().replace(/[^a-zäöüß]+/g, '-')}`;
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

  function leerZustand(icon, satz) {
    return el('div', { class: 'leer-zustand' }, el('span', { html: icon }), el('p', { text: satz }));
  }

  // ---- Seitenleiste ----
  function baueMenue() {
    const menue = document.getElementById('menue');
    for (const a of ansichten) {
      menue.append(el('button', {
        class: 'eintrag', role: 'tab', id: `tab-${a.id}`, 'data-id': a.id,
        'aria-selected': 'false', tabindex: '-1',
        onclick: () => zeige(a.id),
      }, el('span', { html: a.icon }), el('span', { text: a.titel })));
    }
    menue.addEventListener('keydown', (e) => {
      const schritt = { ArrowDown: 1, ArrowUp: -1, Home: -Infinity, End: Infinity }[e.key];
      if (!schritt) return;
      e.preventDefault();
      const i = ansichten.findIndex((a) => a.id === aktuell);
      const n = Math.max(0, Math.min(ansichten.length - 1, i + schritt));
      zeige(ansichten[n].id, { fokus: true });
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
    feld.setAttribute('aria-labelledby', `tab-${id}`);
    const nr = ++zaehler;
    const wurzel = el('div', { class: `ansicht${wechsel ? ' neu' : ''}` });
    feld.setAttribute('aria-busy', 'true');
    try {
      await a.render(wurzel);
    } catch (e) {
      wurzel.replaceChildren(meldung(text(e)));
    }
    if (nr !== zaehler) return; // inzwischen woanders hingeklickt
    const oben = wechsel || !neuLaden ? 0 : feld.scrollTop;
    feld.replaceChildren(wurzel);
    feld.scrollTop = oben;
    feld.removeAttribute('aria-busy');
  }

  const neuLaden = () => zeige(aktuell, { neuLaden: true });

  // ---- Erklärmodus ----
  function erklaermodus() {
    const knopf = document.getElementById('erklaeren');
    const setze = (an) => {
      document.body.classList.toggle('erklaeren', an);
      knopf.setAttribute('aria-checked', an);
      try { localStorage.setItem('netz.erklaeren', an ? '1' : '0'); } catch { /* ohne Speicher bleibt er pro Start */ }
    };
    let an = false;
    try { an = localStorage.getItem('netz.erklaeren') === '1'; } catch { /* s. o. */ }
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
      if (e.metaKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'r') {
        e.preventDefault();
        neuLaden();
      }
    });
    zeige(ansichten[0].id);
  }

  window.Netz = {
    invoke, el, icons, zeit, text, meldung, status, gruppe, zeile, leerZustand, neuLaden,
    registriere: (a) => ansichten.push(a),
  };
  document.addEventListener('DOMContentLoaded', start);
})();
