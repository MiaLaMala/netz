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
  const NAME_QUELLE = { PTR: 'Name aus dem DNS', 'Gerät': 'Name aus „Geräte“' };

  const zustand = { nurNeue: false, merken: false };
  let ergebnis = null;
  // Nach dem Neuzeichnen der ganzen Ansicht (Merken, Löschen) steht der Fokus sonst im Nichts.
  let fokusNach = null;
  const neuLaden = (selektor) => { fokusNach = selektor; Netz.neuLaden(); };
  // Eine Ansage für VoiceOver, einmal pro Ergebnis. Die Liste selbst ist keine Live-Region.
  const ansage = el('p', { class: 'nur-vorlesen', 'aria-live': 'polite' });

  const schalter = (text, key, aenderung) => {
    const s = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': String(zustand[key]), 'data-schalter': key },
      el('span', { class: 'spur' }), el('span', { text }));
    s.addEventListener('click', () => {
      zustand[key] = !zustand[key];
      s.setAttribute('aria-checked', String(zustand[key]));
      aenderung();
    });
    return s;
  };

  // Ohne Namen ist die Adresse der Titel und steht nicht noch einmal darunter.
  function verbindung(v) {
    const adresse = v.ziel.includes(':') ? `[${v.ziel}]` : v.ziel;
    const ziel = [v.name && adresse, v.dienst ? `Port ${v.ziel_port} (${v.dienst})` : `Port ${v.ziel_port}`].filter(Boolean);
    const herkunft = [ART[v.art], v.land, v.anbieter, NAME_QUELLE[v.name_quelle]].filter(Boolean);
    const titel = v.name || adresse;
    return el('div', { class: 'netz-zeile verbindung', role: 'listitem' },
      el('div', { class: 'text' },
        el('div', { class: 'netz-name num kuerzen', title: titel, text: titel }),
        el('div', { class: 'fein num', text: ziel.join(' · ') }),
        herkunft.length ? el('div', { class: 'fein', text: herkunft.join(' · ') }) : null),
      v.neu && el('span', { class: 'status verbindung-status' }, el('span', { class: 'punkt gruen' }), el('span', { text: 'neu' })),
      Netz.woher(v.pid, v.programm));
  }

  // „Nach Land“: Zählung der Verbindungen je Land, nur wenn die Datenbank da ist und etwas trifft.
  // Folgt dem Filter „Nur neue“: Sonst zählte die Gruppe etwas anderes, als die Liste darüber zeigt.
  function nachLand(gruppen) {
    const zaehler = new Map();
    for (const g of gruppen) for (const v of g.verbindungen) if (v.land) zaehler.set(v.land, (zaehler.get(v.land) || 0) + 1);
    if (!zaehler.size) return null;
    const ohne = gruppen.reduce((s, g) => s + g.verbindungen.filter((v) => !v.land).length, 0);
    const sortiert = [...zaehler].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], 'de'));
    const n = (x) => `${x} ${x === 1 ? 'Verbindung' : 'Verbindungen'}`;
    const rest = sortiert.slice(6).reduce((s, [, x]) => s + x, 0);
    return gruppe(zustand.nurNeue ? 'Nach Land, nur neue' : 'Nach Land', [
      ...sortiert.slice(0, 6).map(([land, x]) => zeile(land, n(x))),
      rest && zeile(`${sortiert.length - 6} weitere Länder`, n(rest)),
      ohne ? zeile('Ohne Land', n(ohne), 'Verbindungen im eigenen Netz, zu diesem Mac selbst oder zu Adressen, die nicht in der Datei stehen.') : null,
    ].filter(Boolean));
  }

  function herkunftFuss(r) {
    return el('p', { class: 'fuss', text: r.datenbank_stand
      ? `Land und Anbieter aus einer Datei mit Stand ${r.datenbank_stand.slice(5)}/${r.datenbank_stand.slice(0, 4)}, nichts wird nachgefragt. IP Geolocation by DB-IP (db-ip.com), Anbieter von IPtoASN.`
      : 'Land und Anbieter bleiben leer: Die Datei dafür fehlt in dieser Installation.' });
  }

  function liste(r) {
    const gruppen = r.gruppen
      .map((g) => ({ ...g, verbindungen: zustand.nurNeue ? g.verbindungen.filter((v) => v.neu) : g.verbindungen }))
      .filter((g) => g.verbindungen.length);
    const alle = r.gruppen.reduce((s, g) => s + g.verbindungen.length, 0);
    const neu = r.gruppen.reduce((s, g) => s + g.neu, 0);
    const teile = [alle ? `${alle} ${alle === 1 ? 'Verbindung' : 'Verbindungen'} von ${r.gruppen.length} ${r.gruppen.length === 1 ? 'Programm' : 'Programmen'}` : 'Keine Verbindungen'];
    if (r.netz_name && !r.erster_lauf) teile.push(`${neu} neu in „${r.netz_name}“`);
    const kopf = status(alle ? '' : 'orange', teile.join(' · '));
    const hinweis = !r.netz_name
      ? 'Dieses Netz ist nicht gemerkt: Es gibt kein „neu“ und nichts wird gespeichert.'
      : r.erster_lauf ? `Erster Lauf in „${r.netz_name}“: Das ist der Ausgangsstand, nichts ist „neu“.${r.gespeichert ? '' : ' Gespeichert wird nichts, schalte „Merken“ ein.'}`
      : r.gespeichert ? null : 'Gespeichert wird nichts, deshalb bleiben alte Ziele „neu“. Schalte „Merken“ ein.';
    const fuss = hinweis && el('p', { class: 'fuss', text: hinweis });
    if (!gruppen.length) {
      return [kopf, fuss, leerZustand(Netz.icons.verbindungen,
        !alle ? 'Keine aktiven Verbindungen von dir gefunden. Öffne eine Webseite oder ein Programm, dann erscheint es hier.'
          : !r.netz_name ? 'Ohne gemerktes Netz gibt es kein „neu“. Merke das Netz unter „Gemerkte Netze“.'
          : r.erster_lauf ? 'Beim ersten Lauf ist nichts „neu“. Schau beim nächsten Mal wieder nach.'
          : 'Nichts Neues: Alle Ziele hat dein Rechner in diesem Netz schon einmal angesprochen.', 'verbindungen')];
    }
    return [kopf, fuss, ...gruppen.map((g) => {
      const n = g.verbindungen.length;
      return el('section', { class: 'gruppe' },
        el('h3', { text: `${g.programm} · ${n} ${n === 1 ? 'Verbindung' : 'Verbindungen'}` }),
        el('div', { class: 'liste', role: 'list' }, g.verbindungen.map(verbindung)));
    }), nachLand(gruppen), herkunftFuss(r)];
  }

  // „Wer sendet gerade“ (#58): measures only on click; repeats only while this view is on screen.
  const TRAFFIC_SEKUNDEN = 5;
  const TRAFFIC_TAKT = 30000;
  const menge = (b) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : b >= 1e3 ? `${(b / 1e3).toFixed(0)} kB` : `${Math.round(b)} B`).replace('.', ',');

  function trafficBalken(liste) {
    const aktiv = liste.filter((v) => v.ein + v.aus > 0).slice(0, 8);
    if (!aktiv.length) return [el('p', { class: 'fuss', text: 'In diesen Sekunden floss bei keinem Programm etwas.' })];
    const max = Math.max(...aktiv.map((v) => v.rate_ein + v.rate_aus));
    const legende = el('div', { class: 'traffic-legende', 'aria-hidden': 'true' },
      el('span', { class: 'ein', text: '↓ Empfangen' }), el('span', { class: 'aus', text: '↑ Gesendet' }));
    return [legende, ...aktiv.map((v) => el('div', { class: 'traffic-zeile' },
      el('div', { class: 'traffic-kopf' },
        el('span', { class: 'netz-name kuerzen', title: v.programm, text: v.programm }),
        el('span', { class: 'fein num traffic-rate', 'aria-hidden': 'true' }, el('span', { text: `↓ ${menge(v.rate_ein)}/s` }), el('span', { text: `↑ ${menge(v.rate_aus)}/s` }))),
      el('div', { class: 'traffic-balken', role: 'img', 'aria-label': `${v.programm}: empfängt ${menge(v.rate_ein)} pro Sekunde, sendet ${menge(v.rate_aus)} pro Sekunde` },
        el('span', { class: 'ein', style: `width:${((v.rate_ein / max) * 100).toFixed(1)}%` }),
        el('span', { class: 'aus', style: `width:${((v.rate_aus / max) * 100).toFixed(1)}%` }))))];
  }

  function trafficAbschnitt() {
    const ort = el('div', { class: 'traffic-liste' }, el('p', { class: 'traffic-leer', text: 'Noch nicht gemessen.' }));
    const messen = el('button', { class: 'knopf haupt', type: 'button', text: 'Messen' });
    let wiederholen = false;
    let laeuft = false;
    let timer = null;
    const einmal = async (vonHand = true) => {
      if (laeuft) return;
      laeuft = true;
      // Ein deaktivierter Knopf verliert den Fokus: danach zurück, aber nur wenn er ihn hatte.
      const hatteFokus = document.activeElement === messen;
      messen.disabled = true;
      messen.textContent = 'Messe …';
      try {
        const werte = await Netz.invoke('traffic_messen', { sekunden: TRAFFIC_SEKUNDEN });
        const balken = trafficBalken(werte);
        const uhr = new Date().toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
        ort.replaceChildren(...balken, el('p', { class: 'fuss', text: `Gemessen um ${uhr} Uhr, ${TRAFFIC_SEKUNDEN} Sekunden lang.` }));
        // Nur die Messung per Hand wird angesagt, die Wiederholung alle 30 Sekunden bleibt still.
        if (vonHand) {
          const top = werte.filter((v) => v.ein + v.aus > 0).sort((a, b) => b.rate_ein + b.rate_aus - (a.rate_ein + a.rate_aus))[0];
          ansage.textContent = top ? `Messung fertig. Am meisten Verkehr: ${top.programm}.` : 'Messung fertig. Bei keinem Programm floss etwas.';
        }
      } catch (f) {
        ort.replaceChildren(Netz.fehlerKarte(Netz.text(f), einmal));
        wiederholen = false;
        sw.setAttribute('aria-checked', 'false');
      }
      laeuft = false;
      messen.disabled = false;
      messen.textContent = 'Messen';
      if (hatteFokus) messen.focus();
      clearTimeout(timer);
      if (wiederholen && messen.isConnected) timer = setTimeout(takt, TRAFFIC_TAKT);
    };
    const takt = () => {
      if (!messen.isConnected) return; // view was left: the loop ends
      if (document.hidden) timer = setTimeout(takt, TRAFFIC_TAKT);
      else einmal(false);
    };
    const sw = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': 'false', title: 'Solange diese Ansicht offen ist' },
      el('span', { class: 'spur' }), el('span', { text: 'Alle 30 Sekunden wiederholen' }));
    sw.addEventListener('click', () => {
      wiederholen = !wiederholen;
      sw.setAttribute('aria-checked', String(wiederholen));
      clearTimeout(timer);
      if (wiederholen) einmal();
    });
    messen.addEventListener('click', () => einmal());
    return Object.assign(el('section', { class: 'gruppe traffic', 'aria-labelledby': 'g-traffic' },
      el('h3', { id: 'g-traffic', text: 'Wer sendet gerade' }),
      el('div', { class: 'liste' },
      el('p', { class: 'traffic-text', text: `Misst ${TRAFFIC_SEKUNDEN} Sekunden lang, welches Programm empfängt und sendet. Die Balken zeigen das Verhältnis zum lautesten Programm, gezählt werden alle Wege, auch zum Router.` }),
      el('div', { class: 'traffic-werkzeuge' }, messen, sw),
      ort)), { className: 'gruppe traffic' });
  }

  Netz.registriere({
    id: 'verbindungen',
    titel: 'Verbindungen',
    icon: Netz.icons.verbindungen,
    async render(wurzel) {
      const ort = el('div', { class: 'ergebnis' }, el('p', { class: 'fuss', text: 'Wird gelesen …' }));
      ansage.textContent = '';
      const zeichne = () => ort.replaceChildren(...liste(ergebnis).filter(Boolean));
      let loeschenAus = () => {};
      const merkenGruppe = () => {
        const fehler = el('div', { 'aria-live': 'polite' });
        const loeschen = el('button', { class: 'knopf gefahr', type: 'button', text: 'Löschen …', 'aria-label': 'Gemerkte Daten dieses Netzes löschen' });
        const zeileLoeschen = el('div', { class: 'netz-zeile' },
          el('div', { class: 'text' },
            el('div', { class: 'fein', text: 'Löscht, was Netzinfo über Verbindungen in diesem Netz kennt. Danach ist wieder jedes Ziel „neu“.' })),
          loeschen);
        loeschen.addEventListener('click', () => {
          zeileLoeschen.replaceChildren(Netz.bestaetigung({
            frage: 'Alle gemerkten Verbindungen dieses Netzes löschen? Das lässt sich nicht rückgängig machen.',
            label: 'Gemerkte Daten löschen?',
            abbrechen: () => { zeileLoeschen.replaceChildren(...zeileLoeschen0); loeschen.focus(); },
            aktion: async () => { await Netz.invoke('verbindungen_loeschen'); neuLaden('#g-merken'); },
            fehler: (f) => fehler.replaceChildren(meldung(Netz.text(f), 'rot')),
          }));
        });
        const zeileLoeschen0 = [...zeileLoeschen.childNodes];
        // Ohne gemerktes Netz gibt es nichts zu löschen.
        loeschenAus = () => {
          loeschen.disabled = true;
          zeileLoeschen.querySelector('.fein').textContent = 'Dieses Netz ist nicht gemerkt, es gibt nichts zu löschen.';
        };
        return el('section', { class: 'gruppe', 'aria-labelledby': 'g-merken' },
          el('h3', { id: 'g-merken', tabindex: '-1', text: 'Gemerkte Daten' }),
          el('div', { class: 'liste' },
            el('div', { class: 'netz-zeile' },
              el('div', { class: 'text' },
                el('div', { class: 'fein', text: 'Speichert, welche Ziele dieses Netz kennt, damit neue auffallen. In „meins“-Netzen geschieht das immer, sonst nur mit diesem Schalter.' })),
              schalter('Merken', 'merken', () => neuLaden('[role=switch][data-schalter=merken]'))),
            zeileLoeschen),
          fehler);
      };
      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Verbindungen' }),
          el('p', { class: 'status' }, el('span', { text: 'Wer gerade mit wem spricht, nach Programm gruppiert.' }))),
        el('div', { class: 'werkzeuge' }, schalter('Nur neue', 'nurNeue', () => {
          if (!ergebnis) return;
          zeichne();
          ansage.textContent = ort.querySelector('.status')?.textContent ?? '';
        })),
        ort,
        ansage,
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
        if (!ergebnis.netz_name) loeschenAus();
      } catch (f) {
        ort.replaceChildren(Netz.fehlerKarte(Netz.text(f)));
      }
      if (fokusNach) {
        const ziel = fokusNach;
        fokusNach = null;
        // Die Ansicht hängt erst nach render() im Dokument: einen Takt später fokussieren.
        setTimeout(() => { const e = wurzel.querySelector(ziel); if (e?.isConnected) e.focus(); });
      }
    },
  });
})();
