// „Gemerkte Netze“: die Liste aus netze.json.
(() => {
  const { el, zeit, meldung } = Netz;

  function zeile(start, nr, hier) {
    let n = start;
    const ort = el('div', { class: 'netz-zeile', role: 'listitem' });
    const fehler = el('div', { class: 'zeilenfehler', 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Vergessen' });
    knopf.classList.add('gefahr');
    const zeigeFehler = (e) => fehler.replaceChildren(meldung(Netz.text(e)));
    // Quiet confirmation for screen readers and eyes, gone after a few seconds.
    let weg;
    const rueckmeldung = (satz) => {
      fehler.replaceChildren(el('p', { class: 'fein rueckmeldung', text: satz }));
      clearTimeout(weg);
      weg = setTimeout(() => fehler.replaceChildren(), 4000);
    };
    const zurueck = () => { normal(); ort.querySelector('.umbenennen-knopf').focus(); };

    // Rename in place: Enter saves, Escape cancels, an error stays in the row.
    const umbenennen = () => {
      const feld = el('input', { type: 'text', class: 'umbenennen', value: n.name, 'aria-label': `Neuer Name für ${n.name}`, autocomplete: 'off', spellcheck: 'false' });
      ort.classList.add('bearbeiten');
      feld.addEventListener('keydown', async (e) => {
        if (e.key === 'Escape') zurueck();
        if (e.key !== 'Enter' || feld.disabled) return;
        e.preventDefault();
        if (feld.value.trim() === n.name) return zurueck();
        feld.disabled = true;
        try {
          n = await Netz.invoke('netz_umbenennen', { alt: n.name, neu: feld.value });
          zurueck();
          rueckmeldung(`Umbenannt in „${n.name}“.`);
        } catch (err) {
          zeigeFehler(err);
          feld.disabled = false;
          feld.focus();
        }
      });
      ort.querySelector('.name-ort').replaceChildren(feld);
      fehler.replaceChildren();
      feld.focus();
      feld.select();
    };

    const schalter = () => {
      const s = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': String(!!n.meins), 'aria-label': `${n.name} ist mein Netz`, 'aria-describedby': 'meins-hilfe' },
        el('span', { class: 'spur' }), el('span', { text: 'meins' }));
      s.addEventListener('click', async () => {
        s.disabled = true;
        try {
          n = await Netz.invoke('netz_markieren', { name: n.name, meins: s.getAttribute('aria-checked') !== 'true' });
          normal();
          ort.querySelector('[role="switch"]').focus();
          rueckmeldung(n.meins ? `„${n.name}“ ist jetzt dein Netz.` : `„${n.name}“ ist nicht mehr dein Netz.`);
        } catch (err) {
          zeigeFehler(err);
          s.disabled = false;
          s.focus();
        }
      });
      return s;
    };

    // Label über Wert, in einem Raster: bricht nie mitten im Satz, sondern nur als ganzes Feld um.
    const feld = (label, wert, farbe, leer) => el('div', { class: 'feld' }, el('span', { class: 'fl', text: label }),
      el('span', { class: `fw wert${leer ? ' leer' : ''}` }, farbe && el('span', { class: `punkt ${farbe}` }), wert));
    // Letzte Messung des verschlüsselten DNS (#88), ohne zu senden. Fehlt sie, steht ein Feld mit „noch nicht geprüft“.
    const tag = (s) => new Date(s * 1000).toLocaleDateString('de-DE', { dateStyle: 'medium' });
    const ja = { Erreichbar: ['ja', 'gruen'], Blockiert: ['nein', 'orange'], Abgefangen: ['abgefangen', 'rot'] };
    const dns = el('div', { class: 'felder' }, feld('Verschlüsseltes DNS', 'noch nicht geprüft', null, true));
    Netz.invoke('dns_gespeichert', { name: n.name }).then((g) => {
      if (!g) return;
      const m = g.messung;
      const [dot, doh] = [m.dot, m.doh].map((x) => ja[x] ?? ['unbekannt', 'grau']);
      dns.replaceChildren(feld('DoT', dot[0], dot[1]), feld('DoH', doh[0], doh[1]),
        feld('DDR', m.ddr ? 'ja' : 'nein', m.ddr ? 'gruen' : 'grau'), feld('DNS geprüft am', tag(m.zeit)));
    }).catch(() => {});

    // Verbunden: Der Status steht schon oben, der Platz bleibt leer, damit die Felder in allen Zeilen untereinander stehen.
    const fein = () => [feld('Zuletzt hier', hier ? 'jetzt' : zeit(n.last_seen)), feld('Gemerkt seit', tag(n.first_seen)),
      feld('Router', n.fingerprint.router), feld('Domain', n.fingerprint.domain || 'ohne Domain', null, !n.fingerprint.domain)];
    const normal = () => {
      ort.classList.remove('bearbeiten');
      knopf.setAttribute('aria-label', `${n.name} vergessen`);
      fehler.replaceChildren();
      ort.replaceChildren(
        el('div', { class: 'kopf-zeile' },
          el('div', { class: 'name-ort' }, el('span', { class: 'netz-name', text: n.name, ondblclick: umbenennen }),
            hier && el('span', { class: 'hier' }, el('span', { class: 'punkt gruen' }), 'Verbunden')),
          el('button', { class: 'knopf umbenennen-knopf', type: 'button', text: 'Umbenennen', 'aria-label': `${n.name} umbenennen`, onclick: umbenennen }),
          knopf),
        el('div', { class: 'felder' }, fein()), dns,
        el('div', { class: 'meins-zeile' }, schalter()),
        fehler);
    };
    knopf.addEventListener('click', () => {
      fehler.replaceChildren();
      ort.replaceChildren(
        el('div', { class: 'kopf-zeile' }, el('div', { class: 'name-ort' }, el('span', { class: 'netz-name', text: n.name }))),
        Netz.bestaetigung({
          frage: `„${n.name}“ vergessen? Beim nächsten Besuch gilt das Netz wieder als neu, und die Markierung „meins“ ist weg.${hier ? ' Die Verbindung bleibt, nur Netzinfo vergisst das Netz.' : ''}`,
          label: `${n.name} vergessen?`,
          aria: `${n.name} endgültig vergessen`,
          knopf: 'Vergessen',
          abbrechen: () => { normal(); knopf.focus(); },
          aktion: async () => {
            await Netz.invoke('netz_vergessen', { name: n.name });
            await Netz.neuLaden();
            // The row is gone: focus the one that moved up, not the page.
            const rest = document.querySelectorAll('[data-ansicht="gemerkte-netze"] .umbenennen-knopf');
            rest[Math.min(nr, rest.length - 1)]?.focus();
          },
          fehler: zeigeFehler,
        }),
        fehler);
    });
    normal();
    return ort;
  }

  // Start mit der Anmeldung (#9). Fehlt der Befehl oder scheitert er beim Lesen, bleibt der Schalter weg.
  async function autostart() {
    const an = await Netz.invoke('autostart_lesen').catch(() => null);
    if (an === null) return null;
    // Dev build: the login item would point into target/, the core says so (#142).
    const hinweis = await Netz.invoke('autostart_hinweis').catch(() => null);
    const s = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': an, 'aria-describedby': 'autostart-hilfe' },
      el('span', { class: 'spur' }), el('span', { text: 'Mit der Anmeldung starten' }));
    const fehler = el('div', { 'aria-live': 'polite' });
    s.addEventListener('click', async () => {
      s.disabled = true;
      try {
        s.setAttribute('aria-checked', await Netz.invoke('autostart_setzen', { an: s.getAttribute('aria-checked') !== 'true' }));
        fehler.replaceChildren();
      } catch (err) {
        fehler.replaceChildren(meldung(Netz.text(err)));
      }
      s.disabled = false;
    });
    return el('section', { class: 'gruppe autostart' }, s,
      el('p', { class: 'fein', id: 'autostart-hilfe', text: 'Netzinfo läuft dann in der Menüleiste und meldet sich, wenn ein Netz auffällig ist. Das Fenster zu schließen beendet die Wache nicht.' }),
      hinweis && el('p', { class: 'fein', text: hinweis }), fehler);
  }

  Netz.registriere({
    id: 'gemerkte-netze',
    titel: 'Gemerkte Netze',
    icon: Netz.icons.merken,
    async render(wurzel) {
      const liste = await Netz.invoke('gemerkte_netze');
      // Which saved network is the Mac in right now? Unknown or not saved: no row is marked.
      const jetzt = (await Netz.invoke('dieses_netz').catch(() => null))?.netz?.name;
      const start = await autostart();
      if (!liste.length) {
        // Leere Liste: Der Leerzustand füllt die Höhe, der Schalter steht deshalb davor.
        if (start) wurzel.append(start);
        wurzel.append(Netz.leerZustand(Netz.icons.merken, 'Noch kein Netz gemerkt. Unter „Dieses Netz“ merkst du das aktuelle.', 'gemerkte-netze'));
        return;
      }
      const titel = `${liste.length} ${liste.length === 1 ? 'Netz' : 'Netze'}`;
      wurzel.append(el('section', { class: 'gruppe', 'aria-labelledby': 'g-netze' },
        el('h3', { id: 'g-netze', text: titel }),
        el('div', { class: 'liste', role: 'list' }, liste.map((n, i) => zeile(n, i, n.name === jetzt))),
        el('p', { class: 'fuss', id: 'meins-hilfe', text: '„meins“ erlaubt Gerätesuche und Portscans in diesem Netz. Schalte es nur für Netze ein, die dir gehören.' }),
        el('p', { class: 'fuss erklaer-gruppe', text: 'DoT und DoH verschlüsseln deine DNS-Anfragen (die Frage nach der Adresse zu einem Namen). DDR heißt: Der Router verrät selbst, welchen verschlüsselten Server er anbietet. „nein“ heißt: Bei der letzten Prüfung ging das in diesem Netz nicht.' })));
      if (start) wurzel.append(start);
    },
  });
})();
