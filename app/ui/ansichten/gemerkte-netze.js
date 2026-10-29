// „Gemerkte Netze“: die Liste aus netze.json.
(() => {
  const { el, zeit, meldung } = Netz;

  function zeile(start) {
    let n = start;
    const ort = el('div', { class: 'netz-zeile', role: 'listitem' });
    const fehler = el('div', { class: 'zeilenfehler', 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Vergessen' });
    const zeigeFehler = (e) => fehler.replaceChildren(meldung(Netz.text(e)));
    const zurueck = () => { normal(); ort.querySelector('.umbenennen-knopf').focus(); };

    // Rename in place: Enter saves, Escape cancels, an error stays in the row.
    const umbenennen = () => {
      const feld = el('input', { type: 'text', class: 'umbenennen', value: n.name, 'aria-label': `Neuer Name für ${n.name}`, autocomplete: 'off', spellcheck: 'false' });
      feld.addEventListener('keydown', async (e) => {
        if (e.key === 'Escape') zurueck();
        if (e.key !== 'Enter' || feld.disabled) return;
        e.preventDefault();
        if (feld.value.trim() === n.name) return zurueck();
        feld.disabled = true;
        try {
          n = await Netz.invoke('netz_umbenennen', { alt: n.name, neu: feld.value });
          zurueck();
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
      const s = el('button', { class: 'erklaeren-schalter', type: 'button', role: 'switch', 'aria-checked': n.meins, 'aria-label': `${n.name} ist mein Netz`, 'aria-describedby': `meins-hilfe-${n.name}` },
        el('span', { class: 'spur' }), el('span', { text: 'meins' }));
      s.addEventListener('click', async () => {
        s.disabled = true;
        try {
          n = await Netz.invoke('netz_markieren', { name: n.name, meins: s.getAttribute('aria-checked') !== 'true' });
          normal();
          ort.querySelector('[role="switch"]').focus();
        } catch (err) {
          zeigeFehler(err);
          s.disabled = false;
        }
      });
      return s;
    };

    // Letzte Messung des verschlüsselten DNS (#88), ohne zu senden. Fehlt sie, bleibt der Satz weg.
    const ja = { Erreichbar: 'ja', Blockiert: 'nein', Abgefangen: 'abgefangen' };
    const dns = el('div', { class: 'fein wert' });
    Netz.invoke('dns_gespeichert', { name: n.name }).then((g) => {
      if (!g) return;
      const m = g.messung;
      const tag = new Date(m.zeit * 1000).toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit' });
      dns.textContent = `DNS verschlüsselt: DoT ${ja[m.dot]}, DoH ${ja[m.doh]}, DDR ${m.ddr ? 'ja' : 'nein'} · ${tag}`;
    }).catch(() => {});

    const fein = () => [`zuletzt hier ${zeit(n.last_seen)}`, `Router ${n.fingerprint.router}`, n.fingerprint.domain ? `Domain ${n.fingerprint.domain}` : 'ohne Domain'];
    const normal = () => {
      knopf.setAttribute('aria-label', `${n.name} vergessen`);
      fehler.replaceChildren();
      ort.replaceChildren(
        el('div', { class: 'text' },
          el('div', { class: 'name-ort' }, el('span', { class: 'netz-name', text: n.name, ondblclick: umbenennen })),
          el('div', { class: 'fein wert', text: fein().join(' · ') }), dns),
        el('button', { class: 'knopf umbenennen-knopf', type: 'button', text: 'Umbenennen', 'aria-label': `${n.name} umbenennen`, onclick: umbenennen }),
        knopf,
        el('div', { class: 'meins-zeile' }, schalter(),
          el('span', { class: 'fein', id: `meins-hilfe-${n.name}`, text: 'Erlaubt Gerätesuche und Portscans in diesem Netz.' })),
        fehler);
    };
    knopf.addEventListener('click', () => {
      const ja = el('button', { class: 'knopf gefahr', type: 'button', text: 'Vergessen' });
      const nein = el('button', { class: 'knopf', type: 'button', text: 'Abbrechen' });
      nein.addEventListener('click', () => { normal(); knopf.focus(); });
      ja.addEventListener('click', async () => {
        ja.disabled = true;
        try {
          await Netz.invoke('netz_vergessen', { name: n.name });
          Netz.neuLaden();
        } catch (e) {
          fehler.replaceChildren(meldung(Netz.text(e)));
          ja.disabled = false;
        }
      });
      ort.replaceChildren(
        el('div', { class: 'text' }, el('span', { class: 'netz-name', text: n.name })),
        el('div', { class: 'bestaetigung', role: 'group', 'aria-label': `${n.name} vergessen?` },
          el('p', { text: `„${n.name}“ vergessen? Beim nächsten Besuch gilt das Netz wieder als neu, und die Markierung „meins“ ist weg.` }),
          nein, ja),
        fehler);
      nein.focus();
    });
    normal();
    return ort;
  }

  Netz.registriere({
    id: 'gemerkte-netze',
    titel: 'Gemerkte Netze',
    icon: Netz.icons.merken,
    async render(wurzel) {
      const liste = await Netz.invoke('gemerkte_netze');
      if (!liste.length) {
        wurzel.append(Netz.leerZustand(Netz.icons.merken, 'Noch kein Netz gemerkt. Unter „Dieses Netz“ merkst du das aktuelle.'));
        return;
      }
      const titel = `${liste.length} ${liste.length === 1 ? 'Netz' : 'Netze'}`;
      wurzel.append(el('section', { class: 'gruppe', 'aria-labelledby': 'g-netze' },
        el('h3', { id: 'g-netze', text: titel }),
        el('div', { class: 'liste', role: 'list' }, liste.map((n) => zeile(n)))));
    },
  });
})();
