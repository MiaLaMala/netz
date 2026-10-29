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
    herkunft: 'Land und Anbieter stehen in einer Datei, die mit Netzinfo kommt. Dafür geht keine Adresse ins Netz. Gehört ein Ziel zu einem großen Cloud-Anbieter in Irland, sagt die Adresse allein nicht, welcher Dienst dahinter steckt. Bei Anbietern mit vielen Standorten kann das Land falsch sein.',
    fremd: 'Programme anderer Nutzer und Systemdienste sind ohne Root nicht sichtbar.',
  };
  const ART = { netz: 'im eigenen Netz', lokal: 'dieser Mac' };
  const NAME_QUELLE = { PTR: 'Name per PTR', 'Gerät': 'Name aus „Geräte“' };

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

  // „Woher?“: Aktion der Zeile; lädt die Startkette erst beim Aufklappen (Issue #56).
  const QUELLE = { Terminal: 'Terminal', Systemdienst: 'Systemdienst', Anmeldeobjekt: 'Anmeldeobjekt', Unbekannt: 'unbekannt' };
  function woher(pid, programm) {
    const inhalt = el('div', { class: 'woher-inhalt', hidden: true });
    const knopf = el('button', { class: 'woher-knopf', type: 'button', text: 'Woher?', 'aria-expanded': 'false', 'aria-label': `Woher kommt ${programm}?` });
    knopf.addEventListener('click', async () => {
      const auf = inhalt.hidden;
      inhalt.hidden = !auf;
      knopf.setAttribute('aria-expanded', String(auf));
      if (!auf || inhalt.childElementCount) return;
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
    return [knopf, inhalt];
  }

  // Ohne Namen ist die Adresse der Titel und steht nicht noch einmal darunter.
  function verbindung(v) {
    const adresse = v.ziel.includes(':') ? `[${v.ziel}]` : v.ziel;
    const fein = [v.name && adresse, `Port ${v.ziel_port}`, v.dienst, ART[v.art], v.land, v.anbieter, NAME_QUELLE[v.name_quelle]].filter(Boolean);
    const [knopf, inhalt] = woher(v.pid, v.programm);
    return el('div', { class: 'netz-zeile' },
      el('div', { class: 'text' },
        el('div', { class: 'netz-name num', text: v.name || adresse }),
        el('div', { class: 'fein', text: fein.join(' · ') })),
      v.neu && el('span', { class: 'status verbindung-status' }, el('span', { class: 'punkt gruen' }), el('span', { text: 'neu' })),
      knopf, inhalt);
  }

  // „Nach Land“: Zählung der Verbindungen je Land, nur wenn die Datenbank da ist und etwas trifft.
  function nachLand(r) {
    const zaehler = new Map();
    for (const g of r.gruppen) for (const v of g.verbindungen) if (v.land) zaehler.set(v.land, (zaehler.get(v.land) || 0) + 1);
    if (!zaehler.size) return null;
    const sortiert = [...zaehler].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], 'de'));
    const n = (x) => `${x} ${x === 1 ? 'Verbindung' : 'Verbindungen'}`;
    const rest = sortiert.slice(6).reduce((s, [, x]) => s + x, 0);
    return gruppe('Nach Land', [
      ...sortiert.slice(0, 6).map(([land, x]) => zeile(land, n(x))),
      rest && zeile(`${sortiert.length - 6} weitere Länder`, n(rest)),
    ].filter(Boolean));
  }

  function herkunftFuss(r) {
    return el('p', { class: 'fuss', text: r.datenbank_stand
      ? `Land und Anbieter aus einer Datei mit Stand ${r.datenbank_stand.slice(5)}/${r.datenbank_stand.slice(0, 4)}, nichts wird nachgefragt. IP Geolocation by DB-IP (db-ip.com), Anbieter von IPtoASN.`
      : 'Die Datenbank für Land und Anbieter fehlt, deshalb sind diese Angaben leer.' });
  }

  function liste(r) {
    const gruppen = r.gruppen
      .map((g) => ({ ...g, verbindungen: zustand.nurNeue ? g.verbindungen.filter((v) => v.neu) : g.verbindungen }))
      .filter((g) => g.verbindungen.length);
    const alle = r.gruppen.reduce((s, g) => s + g.verbindungen.length, 0);
    const neu = r.gruppen.reduce((s, g) => s + g.neu, 0);
    const teile = [`${alle} Verbindungen von ${r.gruppen.length} Programmen`];
    if (r.netz_name && !r.erster_lauf) teile.push(`${neu} neu in „${r.netz_name}“`);
    const kopf = status(alle ? '' : 'orange', teile.join(' · '));
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
    }), nachLand(r), herkunftFuss(r)];
  }

  // „Wer sendet gerade“ (#58): measures only on click; repeats only while this view is on screen.
  const TRAFFIC_SEKUNDEN = 5;
  const TRAFFIC_TAKT = 30000;
  const menge = (b) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : b >= 1e3 ? `${(b / 1e3).toFixed(0)} kB` : `${Math.round(b)} B`).replace('.', ',');

  function trafficBalken(liste) {
    const aktiv = liste.filter((v) => v.ein + v.aus > 0).slice(0, 8);
    if (!aktiv.length) return [el('p', { class: 'fuss', text: 'In diesen Sekunden floss bei keinem Programm etwas.' })];
    const max = Math.max(...aktiv.map((v) => v.rate_ein + v.rate_aus));
    return aktiv.map((v) => el('div', { class: 'traffic-zeile' },
      el('div', { class: 'traffic-kopf' },
        el('span', { class: 'netz-name', text: v.programm }),
        el('span', { class: 'fein num', text: `↓ ${menge(v.rate_ein)}/s · ↑ ${menge(v.rate_aus)}/s` })),
      el('div', { class: 'traffic-balken', role: 'img', 'aria-label': `${v.programm}: ${menge(v.ein)} empfangen, ${menge(v.aus)} gesendet` },
        el('span', { class: 'ein', style: `width:${((v.rate_ein / max) * 100).toFixed(1)}%` }),
        el('span', { class: 'aus', style: `width:${((v.rate_aus / max) * 100).toFixed(1)}%` }))));
  }

  function trafficAbschnitt() {
    const ort = el('div', { class: 'traffic-liste', 'aria-live': 'polite' }, el('p', { class: 'traffic-leer', text: 'Noch nicht gemessen.' }));
    const messen = el('button', { class: 'knopf haupt', type: 'button', text: 'Messen' });
    let wiederholen = false;
    let laeuft = false;
    let timer = null;
    const einmal = async () => {
      if (laeuft) return;
      laeuft = true;
      messen.disabled = true;
      messen.textContent = 'Messe …';
      try {
        ort.replaceChildren(...trafficBalken(await Netz.invoke('traffic_messen', { sekunden: TRAFFIC_SEKUNDEN })));
      } catch (f) {
        ort.replaceChildren(meldung(Netz.text(f), 'rot'));
        wiederholen = false;
        sw.setAttribute('aria-checked', 'false');
      }
      laeuft = false;
      messen.disabled = false;
      messen.textContent = 'Messen';
      clearTimeout(timer);
      if (wiederholen && messen.isConnected) timer = setTimeout(takt, TRAFFIC_TAKT);
    };
    const takt = () => {
      if (!messen.isConnected) return; // view was left: the loop ends
      if (document.hidden) timer = setTimeout(takt, TRAFFIC_TAKT);
      else einmal();
    };
    const sw = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': 'false', title: 'Solange diese Ansicht offen ist' },
      el('span', { class: 'spur' }), el('span', { text: 'Alle 30 Sekunden wiederholen' }));
    sw.addEventListener('click', () => {
      wiederholen = !wiederholen;
      sw.setAttribute('aria-checked', String(wiederholen));
      clearTimeout(timer);
      if (wiederholen) einmal();
    });
    messen.addEventListener('click', einmal);
    return Object.assign(gruppe('Wer sendet gerade', [
      el('p', { class: 'traffic-text', text: `Misst ${TRAFFIC_SEKUNDEN} Sekunden lang, welches Programm empfängt (blau) und sendet (orange). Die Balken zeigen das Verhältnis zum lautesten Programm, gezählt werden alle Wege, auch zum Router.` }),
      el('div', { class: 'traffic-werkzeuge' }, messen, sw),
      ort,
    ]), { className: 'gruppe traffic' });
  }

  Netz.registriere({
    id: 'verbindungen',
    titel: 'Verbindungen',
    icon: Netz.icons.verbindungen,
    async render(wurzel) {
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'polite' });
      const zeichne = () => ort.replaceChildren(...liste(ergebnis).filter(Boolean));
      const merkenGruppe = () => {
        const fehler = el('div', { 'aria-live': 'polite' });
        const loeschen = el('button', { class: 'knopf gefahr', type: 'button', text: 'Löschen …', 'aria-label': 'Gemerkte Daten dieses Netzes löschen' });
        const zeileLoeschen = el('div', { class: 'netz-zeile' },
          el('div', { class: 'text' },
            el('div', { class: 'fein', text: 'Löscht, was Netzinfo über Verbindungen in diesem Netz kennt. Danach ist wieder jedes Ziel „neu“.' })),
          loeschen);
        loeschen.addEventListener('click', () => {
          const nein = el('button', { class: 'knopf', type: 'button', text: 'Abbrechen' });
          const ja = el('button', { class: 'knopf gefahr', type: 'button', text: 'Löschen' });
          nein.addEventListener('click', () => { zeileLoeschen.replaceChildren(...zeileLoeschen0); loeschen.focus(); });
          ja.addEventListener('click', async () => {
            ja.disabled = true;
            try {
              await Netz.invoke('verbindungen_loeschen');
              Netz.neuLaden();
            } catch (f) {
              fehler.replaceChildren(meldung(Netz.text(f), 'rot'));
              ja.disabled = false;
            }
          });
          zeileLoeschen.replaceChildren(el('div', { class: 'bestaetigung', role: 'group', 'aria-label': 'Gemerkte Daten löschen?' },
            el('p', { text: 'Alle gemerkten Verbindungen dieses Netzes löschen? Das lässt sich nicht rückgängig machen.' }), nein, ja));
          nein.focus();
        });
        const zeileLoeschen0 = [...zeileLoeschen.childNodes];
        return el('section', { class: 'gruppe', 'aria-labelledby': 'g-merken' },
          el('h3', { id: 'g-merken', text: 'Gemerkte Daten' }),
          el('div', { class: 'liste' },
            el('div', { class: 'netz-zeile' },
              el('div', { class: 'text' },
                el('div', { class: 'fein', text: 'Speichert, welche Ziele dieses Netz kennt, damit neue auffallen. In „meins“-Netzen geschieht das immer, sonst nur mit diesem Schalter.' })),
              schalter('Merken', 'merken', () => Netz.neuLaden())),
            zeileLoeschen),
          fehler);
      };
      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Verbindungen' }),
          el('p', { class: 'status' }, el('span', { text: 'Wer gerade mit wem spricht, nach Programm gruppiert.' }))),
        el('div', { class: 'werkzeuge' }, schalter('Nur neue', 'nurNeue', () => ergebnis && zeichne())),
        ort,
        merkenGruppe(),
        trafficAbschnitt(),
        Object.assign(gruppe('So entsteht die Liste', [
          zeile('Verbunden', 'nur ESTABLISHED', E.verbunden),
          zeile('Name', 'Rückwärts-DNS (PTR)', E.ptr),
          zeile('Land und Anbieter', 'aus einer Datei im Programm', E.herkunft),
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
