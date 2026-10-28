// „Geräte“: Geräte im eigenen Netz, „neu“ und „verschwunden“ seit dem letzten Lauf.
// Die Schutzregel („meins“) steckt im Kern (src/geraete.rs). Hier wird nur ihre Ablehnung gezeigt.
(() => {
  const { el, zeit, status, meldung, gruppe, zeile, leerZustand } = Netz;

  const DIENSTE = {
    22: 'SSH', 53: 'DNS', 80: 'Web', 443: 'Web (TLS)', 445: 'Dateifreigabe (SMB)', 548: 'Dateifreigabe (AFP)',
    631: 'Drucker (IPP)', 7000: 'AirPlay', 8009: 'Cast', 9100: 'Drucker (RAW)', 62078: 'iPhone-Sync',
  };
  const portText = (p) => (DIENSTE[p] ? `${p} ${DIENSTE[p]}` : `${p}`);
  // IPv4 as one number, so 192.168.178.9 sorts before 192.168.178.10.
  const ipZahl = (ip) => ip.split('.').reduce((s, o) => s * 256 + Number(o), 0);

  const E = {
    abgelehnt: 'Wenn du an einen Port klopfst und die Verbindung aktiv abgelehnt wird, hat ein laufendes Gerät geantwortet: Nur ein Gerät kann diese Ablehnung (ein RST-Paket) schicken. Auch ohne offenen Port beweist sie also, dass dort jemand ist.',
    ptr: 'PTR ist die Rückwärtsauflösung im DNS: von der IP zum Namen, abgefragt unter z. B. 20.178.168.192.in-addr.arpa. Viele Router tragen dort die Namen ihrer DHCP-Kunden ein, andere liefern nichts, dann bleibt der Name leer.',
    fehlen: 'Ohne Root-Rechte kann der Mac weder pingen noch ARP lesen. Ein Gerät, das jedes Paket verwirft (Firewall im Tarnmodus, Handy im Standby), antwortet auf nichts und bleibt unsichtbar. „Verschwunden“ heißt deshalb nur: diesmal nicht gefunden.',
  };

  let letztes = null; // last result, so a tab change does not lose it

  // The kernel refuses with one fixed sentence; this is the only part of it the view looks at.
  const abgelehnt = (f) => typeof f === 'string' && f.includes('als „meins“ markiert');

  function geraet(d, tag) {
    const ports = d.ports && d.ports.length
      ? d.ports.map(portText).join(', ')
      : 'Keine offenen Ports, Verbindung abgelehnt';
    const wort = tag === 'neu' ? ['gruen', 'neu'] : tag === 'weg' ? ['orange', 'verschwunden'] : null;
    return el('div', { class: 'netz-zeile' },
      el('div', { class: 'text' },
        el('div', {}, el('span', { class: 'netz-name num', text: d.ip }),
          el('span', { class: 'marke', text: d.name ? ` · ${d.name}` : ' · kein Name' })),
        el('div', { class: 'fein', text: ports }),
        el('div', { class: 'fein', text: `Zuerst gesehen ${zeit(d.first_seen)}, zuletzt gesehen ${zeit(d.last_seen)}` })),
      wort && el('span', { class: 'status geraet-status' }, el('span', { class: `punkt ${wort[0]}` }), el('span', { text: wort[1] })));
  }

  function ergebnis(r) {
    const neu = new Set(r.neu.map((d) => d.ip));
    const zeilen = [
      ...r.geraete.map((d) => [d, neu.has(d.ip) ? 'neu' : '']),
      ...r.verschwunden.map((d) => [d, 'weg']),
    ].sort((a, b) => ipZahl(a[0].ip) - ipZahl(b[0].ip));

    const teile = [`${r.geraete.length} gefunden in ${r.bereich}`];
    if (!r.erster_lauf) teile.push(`${r.neu.length} neu`, `${r.verschwunden.length} verschwunden`);
    const kopf = status(r.geraete.length ? 'gruen' : 'orange', teile.join(' · '));
    const erster = r.erster_lauf && el('p', { class: 'fuss', text: 'Erster Lauf in diesem Netz: Das ist der Ausgangsstand, nichts ist „neu“. Ab dem nächsten Lauf siehst du, was dazukommt und was fehlt.' });
    if (!zeilen.length) {
      return [kopf, erster, leerZustand(Netz.icons.geraete, 'Keine Geräte gefunden. Geräte, die jedes Paket verwerfen, bleiben unsichtbar.')];
    }
    return [kopf, erster, el('section', { class: 'gruppe', 'aria-label': 'Gefundene Geräte' },
      el('div', { class: 'liste' }, zeilen.map(([d, t]) => geraet(d, t))))];
  }

  Netz.registriere({
    id: 'geraete',
    titel: 'Geräte',
    icon: Netz.icons.geraete,
    async render(wurzel) {
      const knopf = el('button', { class: 'knopf haupt', type: 'button', text: letztes ? 'Noch mal suchen' : 'Suchen' });
      const fortschritt = el('p', { class: 'fuss fortschritt', 'aria-live': 'polite' });
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'polite' });

      async function suche() {
        knopf.disabled = true;
        ort.replaceChildren();
        let bereich = 'deinem Netz';
        try {
          const s = (await Netz.invoke('dieses_netz')).info.subnet;
          if (s) bereich = `${s.network}/${s.prefix}`;
        } catch { /* the sentence stays general, the search itself reports real problems */ }
        fortschritt.textContent = `Suche in ${bereich} …`;
        try {
          letztes = await Netz.invoke('geraete_suchen', { cidr: null, gross: false });
          ort.replaceChildren(...ergebnis(letztes).filter(Boolean));
        } catch (f) {
          const nein = abgelehnt(f);
          ort.replaceChildren(...[meldung(Netz.text(f), nein ? 'orange' : 'rot'),
            nein && el('div', { class: 'aktion' }, el('button', {
              class: 'knopf', type: 'button', text: 'Zu „Gemerkte Netze“',
              onclick: () => document.getElementById('tab-gemerkte-netze')?.click(),
            }))].filter(Boolean));
        }
        fortschritt.textContent = '';
        knopf.disabled = false;
        knopf.focus();
      }
      knopf.addEventListener('click', suche);

      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Geräte im Netz' }),
          el('p', { class: 'status' }, el('span', { text: 'Nur in Netzen, die du als „meins“ markiert hast.' }))),
        el('div', { class: 'aktion' }, knopf),
        fortschritt, ort,
        Object.assign(gruppe('So funktioniert die Suche', [
          zeile('Abgelehnter Port', 'zeigt ein Gerät', E.abgelehnt),
          zeile('Name (PTR)', 'Rückwärts-DNS', E.ptr),
          zeile('Fehlende Geräte', 'können sich verstecken', E.fehlen),
        ]), { className: 'gruppe erklaer-gruppe' }));
      if (letztes) ort.replaceChildren(...ergebnis(letztes).filter(Boolean));
      else ort.append(leerZustand(Netz.icons.geraete, 'Noch nicht gesucht. „Suchen“ probiert ein paar gängige Ports an jeder Adresse deines Netzes.'));
    },
  });
})();
