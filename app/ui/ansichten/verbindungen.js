// „Verbindungen“: welches Programm gerade mit welchem Ziel verbunden ist, gruppiert nach Programm.
// „neu“ heißt nur: In diesem Netz noch nie gesehen. Der Kern (src/verbindungen.rs) speichert nur in
// „meins“-Netzen oder wenn der Schalter „Merken“ an ist.
(() => {
  const { el, status, meldung, gruppe, zeile, leerZustand } = Netz;
  Netz.icons.verbindungen = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="3.5" cy="8" r="1.8"/><circle cx="12.5" cy="4" r="1.8"/><circle cx="12.5" cy="12" r="1.8"/><path d="M5.2 7.2 10.8 4.8M5.2 8.8l5.6 2.4"/></svg>';

  const E = {
    verbunden: 'ESTABLISHED heißt: Beide Seiten haben den TCP-Handschlag abgeschlossen, die Verbindung steht. Die Liste zeigt nur diese. Wartende oder gerade endende Verbindungen fehlen.',
    ptr: 'Der Name kommt per Rückwärts-DNS (PTR) vom DNS-Server deines Systems, höchstens 500 ms lang. Oft nennt er den Hoster statt des Dienstes. Fehlt er, siehst du nur die Adresse. Bei Geräten im eigenen Netz hat der Gerätename aus „Geräte“ Vorrang.',
    neu: 'Ein Ziel mit „neu“ hat dein Rechner in diesem Netz noch nie angesprochen. Das ist nicht gefährlich, aber neu. Netzinfo sperrt nichts. Gemerkt wird nur in „meins“-Netzen oder mit dem Schalter „Merken“.',
    fremd: 'Programme anderer Nutzer und Systemdienste sind ohne Root nicht sichtbar.',
  };
  const ART = { netz: 'Netz', lokal: 'lokal' };

  const zustand = { nurNeue: false, merken: false };
  let ergebnis = null;

  const schalter = (text, key, aenderung) => {
    const s = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': String(zustand[key]) },
      el('span', { class: 'spur' }), el('span', { text }));
    s.addEventListener('click', () => {
      zustand[key] = !zustand[key];
      s.setAttribute('aria-checked', String(zustand[key]));
      aenderung();
    });
    return s;
  };

  // „Woher?“: lädt die Startkette erst beim Aufklappen (Issue #56).
  const QUELLE = { Terminal: 'Terminal', Systemdienst: 'Systemdienst', Anmeldeobjekt: 'Anmeldeobjekt', Unbekannt: 'unbekannt' };
  function woher(pid) {
    const inhalt = el('div', { class: 'woher-inhalt' });
    const d = el('details', { class: 'woher' }, el('summary', { text: 'Woher?' }), inhalt);
    d.addEventListener('toggle', async () => {
      if (!d.open || inhalt.childElementCount) return;
      try {
        const h = await Netz.invoke('woher_zeigen', { pid });
        inhalt.append(
          el('div', { class: 'fein', text: `Quelle: ${QUELLE[h.quelle]}` }),
          el('ul', { class: 'woher-kette' }, [...h.kette].reverse().map((p, i) =>
            el('li', { class: 'num', style: `margin-left:${i * 14}px`, text: `${p.name} (PID ${p.pid})` }))),
          el('p', { class: 'fein', text: h.satz }));
      } catch (f) {
        inhalt.append(meldung(Netz.text(f), 'rot'));
      }
    });
    return d;
  }

  function verbindung(v) {
    const ziel = v.name || v.ziel;
    const fein = [`${v.ziel.includes(':') ? `[${v.ziel}]` : v.ziel}:${v.ziel_port}`, v.dienst, ART[v.art], v.name_quelle && `Name laut ${v.name_quelle}`].filter(Boolean);
    return el('div', { class: 'verbindung-eintrag' },
      el('div', { class: 'netz-zeile' },
        el('div', { class: 'text' },
          el('div', { class: 'netz-name num', text: ziel }),
          el('div', { class: 'fein', text: fein.join(' · ') })),
        v.neu && el('span', { class: 'status verbindung-status' }, el('span', { class: 'punkt gruen' }), el('span', { text: 'neu' }))),
      woher(v.pid));
  }

  function liste(r) {
    const gruppen = r.gruppen
      .map((g) => ({ ...g, verbindungen: zustand.nurNeue ? g.verbindungen.filter((v) => v.neu) : g.verbindungen }))
      .filter((g) => g.verbindungen.length);
    const alle = r.gruppen.reduce((s, g) => s + g.verbindungen.length, 0);
    const neu = r.gruppen.reduce((s, g) => s + g.neu, 0);
    const teile = [`${alle} Verbindungen von ${r.gruppen.length} Programmen`];
    if (r.netz_name && !r.erster_lauf) teile.push(`${neu} neu in „${r.netz_name}“`);
    const kopf = status(alle ? 'gruen' : 'orange', teile.join(' · '));
    const hinweis = !r.netz_name
      ? 'Dieses Netz ist nicht gemerkt: Es gibt kein „neu“ und nichts wird gespeichert.'
      : r.erster_lauf ? `Erster Lauf in „${r.netz_name}“: Das ist der Ausgangsstand, nichts ist „neu“.` : null;
    const fuss = hinweis && el('p', { class: 'fuss', text: hinweis });
    if (!gruppen.length) {
      return [kopf, fuss, leerZustand(Netz.icons.verbindungen,
        zustand.nurNeue && alle ? 'Nichts Neues: Alle Ziele hat dein Rechner in diesem Netz schon einmal angesprochen.' : 'Keine aktiven Verbindungen von dir gefunden.')];
    }
    return [kopf, fuss, ...gruppen.map((g) => {
      const n = g.verbindungen.length;
      return el('section', { class: 'gruppe', 'aria-label': g.programm },
        el('h3', { text: `${g.programm} · ${n} ${n === 1 ? 'Verbindung' : 'Verbindungen'}` }),
        el('div', { class: 'liste' }, g.verbindungen.map(verbindung)));
    })];
  }

  Netz.registriere({
    id: 'verbindungen',
    titel: 'Verbindungen',
    icon: Netz.icons.verbindungen,
    async render(wurzel) {
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'polite' });
      const zeichne = () => ort.replaceChildren(...liste(ergebnis).filter(Boolean));
      const loeschen = el('button', { class: 'knopf', type: 'button', text: 'Gemerkte Daten dieses Netzes löschen' });
      loeschen.addEventListener('click', async () => {
        try {
          await Netz.invoke('verbindungen_loeschen');
          Netz.neuLaden();
        } catch (f) {
          ort.prepend(meldung(Netz.text(f), 'rot'));
        }
      });
      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Verbindungen' }),
          el('p', { class: 'status' }, el('span', { text: 'Wer gerade mit wem spricht, nach Programm gruppiert.' }))),
        el('div', { class: 'werkzeuge' },
          schalter('Nur neue', 'nurNeue', () => ergebnis && zeichne()),
          schalter('Merken', 'merken', () => Netz.neuLaden())),
        ort,
        el('div', { class: 'aktion' }, loeschen),
        Object.assign(gruppe('So entsteht die Liste', [
          zeile('Verbunden', 'nur ESTABLISHED', E.verbunden),
          zeile('Name', 'Rückwärts-DNS (PTR)', E.ptr),
          zeile('Neu', 'in diesem Netz noch nie gesehen', E.neu),
          zeile('Sichtbar', 'nur deine Programme', E.fremd),
        ]), { className: 'gruppe erklaer-gruppe' }));
      try {
        ergebnis = await Netz.invoke('verbindungen_zeigen', { speichern: zustand.merken });
        zeichne();
      } catch (f) {
        ort.replaceChildren(meldung(Netz.text(f), 'rot'));
      }
    },
  });
})();
