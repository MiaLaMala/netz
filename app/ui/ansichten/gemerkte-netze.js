// „Gemerkte Netze“: die Liste aus netze.json. Umbenennen und Markieren kommen in der nächsten Runde.
(() => {
  const { el, zeit, meldung } = Netz;

  function zeile(n) {
    const fein = [`zuletzt hier ${zeit(n.last_seen)}`, `Router ${n.fingerprint.router}`, n.fingerprint.domain ? `Domain ${n.fingerprint.domain}` : 'ohne Domain'];
    const ort = el('div', { class: 'netz-zeile', role: 'listitem' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Vergessen', 'aria-label': `${n.name} vergessen` });

    const normal = () => {
      ort.replaceChildren(
        el('div', { class: 'text' },
          el('div', {}, el('span', { class: 'netz-name', text: n.name }), n.meins && el('span', { class: 'marke', text: ' · meins' })),
          el('div', { class: 'fein wert', text: fein.join(' · ') })),
        knopf);
    };
    knopf.addEventListener('click', () => {
      const fehler = el('div', { 'aria-live': 'polite' });
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
        el('div', { class: 'liste', role: 'list' }, liste.map(zeile))));
    },
  });
})();
