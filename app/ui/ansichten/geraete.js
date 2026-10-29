// „Geräte“: eine Karte je gemerktem Gerät (Typ, Hersteller, Dienste, Hinweise), „Suchen“ frischt auf.
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
    mac: 'Die MAC-Adresse ist die Hardware-Nummer der Netzkarte und bleibt gleich, auch wenn der Router dem Gerät eine andere IP gibt. Der Mac kennt sie aus seiner Nachbartabelle (ARP): Er fragt „Wer hat diese IP?“ und merkt sich die Antwort. Darum erkenne ich ein Gerät wieder und zeige seine früheren IPs.',
    typ: 'Der Typ kommt aus Regeln: Bietet ein Gerät per Bonjour „Drucken“ an oder hat Port 631 offen, hält Netzinfo es für einen Drucker. Mehrere Hinweise zählen zusammen; bei Gleichstand bleibt es „Unbekannt“. Ein Gerät kann sich anders ausgeben, als es ist. Was du selbst einträgst, ändert kein neuer Lauf.',
    fehlen: 'Ohne Root-Rechte kann der Mac weder pingen noch ARP lesen. Ein Gerät, das jedes Paket verwirft (Firewall im Tarnmodus, Handy im Standby), antwortet auf nichts und bleibt unsichtbar. „Verschwunden“ heißt deshalb nur: diesmal nicht gefunden.',
  };

  let letztes = null; // last result, so a tab change does not lose it
  let laufend = null; // running search; a re-render (⌘R) waits for it instead of starting a second scan

  // The kernel names the refusal in `art`; the view never reads the sentence.
  const abgelehnt = (f) => f?.art === 'nicht_meins';

  // Card = one stored device (Rust: geraetekarte::Karte). All texts about the device come from the core.
  const TYPEN = ['Router', 'Drucker', 'Medien', 'Smart Home', 'Computer', 'Handy', 'Unbekannt'];
  let filter = null; // null = all types
  let karten = [];
  let markiert = new Map(); // ip -> 'neu' | 'weg', from the last search
  const ZUFALL = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 4.5h2.5c3 0 4 7 7 7H14M2 11.5h2.5c1 0 1.8-1 2.5-2.2M9 6.2c.7-1 1.5-1.7 2.5-1.7H14M12.5 3 14 4.5 12.5 6M12.5 10 14 11.5 12.5 13"/></svg>';

  function bearbeiten(k, zu) {
    const name = el('input', { type: 'text', value: k.von_mia?.name ?? '', maxlength: '60', placeholder: k.anzeigename, 'aria-label': 'Name' });
    const typ = el('select', { 'aria-label': 'Typ' }, el('option', { value: '', text: 'Typ automatisch' }),
      TYPEN.slice(0, -1).map((t) => el('option', { value: t, text: t, selected: k.von_mia?.typ === t })));
    const fehler = el('div');
    const speichern = el('button', { class: 'knopf haupt', type: 'submit', text: 'Speichern' });
    return el('form', {
      class: 'karte-formular',
      onsubmit: async (e) => {
        e.preventDefault();
        speichern.disabled = true;
        try {
          await Netz.invoke('geraet_setzen', { schluessel: k.mac || k.ip, name: name.value, typ: typ.value });
          karten = await Netz.invoke('geraete_karten');
          zeichne();
        } catch (f) {
          fehler.replaceChildren(meldung(Netz.text(f), 'rot'));
          speichern.disabled = false;
        }
      },
    }, name, typ,
    el('button', { class: 'knopf', type: 'button', text: 'Abbrechen', onclick: zu }), speichern, fehler);
  }

  function karte(k) {
    const tag = markiert.get(k.ip);
    const wort = tag === 'neu' ? ['gruen', 'neu'] : tag === 'weg' ? ['orange', 'nicht gefunden'] : null;
    const formular = el('div', { class: 'karte-bearbeiten' });
    const ports = k.ports && k.ports.length ? `Offene Ports: ${k.ports.map(portText).join(', ')}` : null;
    return el('article', { class: 'karte', 'aria-label': k.anzeigename },
      el('div', { class: 'karte-kopf' },
        el('span', { class: 'karte-symbol', text: k.symbol, 'aria-hidden': 'true' }),
        el('div', { class: 'text' },
          el('div', {}, el('span', { class: 'netz-name', text: k.anzeigename }),
            k.namequelle && el('span', { class: 'marke', text: ` · ${k.namequelle}` })),
          el('div', { class: 'fein' },
            el('span', { text: k.hersteller || (k.mac_zufaellig ? 'Hersteller nicht erkennbar' : 'Hersteller unbekannt') }),
            k.mac_zufaellig && el('span', { class: 'zufall', title: 'Zufällige Adresse', html: `${ZUFALL}<span>Zufällige Adresse</span>` }))),
        el('span', { class: 'karte-typ', text: k.typ }),
        wort && el('span', { class: 'status geraet-status' }, el('span', { class: `punkt ${wort[0]}` }), el('span', { text: wort[1] }))),
      el('div', { class: 'fein num', text: [k.ip, k.mac && `MAC ${k.mac}`].filter(Boolean).join(' · ') }),
      k.dienste.length > 0 && el('ul', { class: 'karte-dienste', 'aria-label': 'Bonjour-Dienste' },
        k.dienste.map((d) => el('li', {}, el('span', { text: d.instanz || d.typ }), el('span', { class: 'marke', text: ` ${d.typ}${d.port ? `:${d.port}` : ''}` })))),
      ports && el('div', { class: 'fein', text: ports }),
      k.upnp && el('div', { class: 'fein', text: `UPnP: ${[k.upnp.modell, k.upnp.geraetetyp?.split(':')[3]].filter(Boolean).join(' · ')}` }),
      k.hinweise.length > 0 && el('ul', { class: 'karte-hinweise', 'aria-label': 'Hinweise' }, k.hinweise.map((h) => el('li', { text: h }))),
      el('div', { class: 'fein', text: `Zuerst gesehen ${zeit(k.first_seen)}, zuletzt gesehen ${zeit(k.last_seen)}, ${k.besuche} ${k.besuche === 1 ? 'Besuch' : 'Besuche'}` }),
      el('p', { class: 'erklaerung', text: k.erklaerung }),
      k.gateway && routerBlock(),
      formular,
      el('div', { class: 'karte-aktion' }, el('button', {
        class: 'knopf', type: 'button', text: 'Name und Typ ändern',
        onclick: (e) => {
          e.currentTarget.hidden = true;
          formular.replaceChildren(bearbeiten(k, () => { formular.replaceChildren(); e.currentTarget.hidden = false; }));
          formular.querySelector('input').focus();
        },
      })));
  }

  // Router-Seite (Rust: router::Ergebnis): one read-only look at the gateway, shown inside the router card.
  const E_ROUTER = 'Netzinfo ruft die Startseite des Routers einmal über http ab und lässt sich über https das Zertifikat zeigen. Gelesen wird nur die Kennung des Webservers, der Seitentitel und das Zertifikat. Es gibt kein Anmelden und kein Ausprobieren anderer Seiten. Ein selbst ausgestelltes Zertifikat ist bei Routern üblich und kein Zeichen für einen Angriff.';
  const portStatus = (s) => (s === 'offen' ? 'erreichbar' : s === 'zu' ? 'geschlossen' : s?.fehler ?? 'unbekannt');
  let routerErgebnis = null;
  function routerBlock() {
    const ort = el('div', { class: 'router-seite', 'aria-live': 'polite' });
    const knopf = el('button', { class: 'knopf', type: 'button', text: 'Router-Seite prüfen' });
    const zeige = (r) => {
      const s = r.seite;
      const z = s.zertifikat;
      ort.replaceChildren(
        el('div', { class: 'fein', text: `http (80): ${portStatus(s.http)}${s.http === 'offen' ? (s.umleitung ? ', leitet auf https um' : ', ohne Umleitung auf https') : ''}` }),
        el('div', { class: 'fein', text: `https (443): ${portStatus(s.https)}` }),
        z && el('div', { class: 'fein', text: `Zertifikat bis ${z.gueltig_bis}${z.abgelaufen ? ' (abgelaufen)' : ''}${z.selbstsigniert ? ', selbst ausgestellt' : ''}` }),
        s.server && el('div', { class: 'fein', text: `Server: ${s.server}` }),
        s.titel && el('div', { class: 'fein', text: `Seitentitel: ${s.titel}` }),
        r.hinweise.length > 0 && el('ul', { class: 'karte-hinweise', 'aria-label': 'Hinweise zur Router-Seite' }, r.hinweise.map((h) => el('li', { text: h }))),
        el('p', { class: 'erklaerung', text: E_ROUTER }));
    };
    if (routerErgebnis) zeige(routerErgebnis);
    knopf.addEventListener('click', async () => {
      knopf.disabled = true;
      try {
        routerErgebnis = await Netz.invoke('router_seite');
        zeige(routerErgebnis);
      } catch (f) {
        ort.replaceChildren(meldung(Netz.text(f), f?.art === 'nicht_meins' ? 'orange' : 'rot'));
      }
      knopf.disabled = false;
    });
    return el('div', { class: 'karte-router' }, el('div', { class: 'karte-aktion' }, knopf), ort);
  }

  const kartenOrt = el('div', { class: 'karten-ort' });
  function zeichne() {
    const vorhanden = new Set(karten.map((k) => k.typ));
    if (filter && !vorhanden.has(filter)) filter = null;
    const sicht = karten.filter((k) => !filter || k.typ === filter);
    const chip = (t) => el('button', {
      class: 'chip', type: 'button', 'aria-pressed': String(filter === t), onclick: () => { filter = t; zeichne(); },
      text: t === null ? `Alle ${karten.length}` : `${t} ${karten.filter((k) => k.typ === t).length}`,
    });
    if (!karten.length) {
      kartenOrt.replaceChildren(leerZustand(Netz.icons.geraete, 'Noch keine Geräte gemerkt. „Suchen“ legt für jedes gefundene Gerät eine Karte an.'));
      return;
    }
    kartenOrt.replaceChildren(
      el('div', { class: 'filter', role: 'group', 'aria-label': 'Nach Typ filtern' },
        [null, ...TYPEN.filter((t) => vorhanden.has(t))].map(chip)),
      el('p', { class: 'fuss', text: 'Zuletzt gesehen zuerst. Der Typ ist eine Vermutung aus dem, was ein Gerät selbst ankündigt.' }),
      el('section', { class: 'karten', 'aria-label': 'Gerätekarten' }, sicht.map(karte)));
  }

  async function ladeKarten() {
    try {
      karten = await Netz.invoke('geraete_karten');
      zeichne();
    } catch (f) {
      kartenOrt.replaceChildren(meldung(Netz.text(f), 'rot'));
    }
  }

  function ergebnis(r) {
    markiert = new Map([...r.neu.map((d) => [d.ip, 'neu']), ...r.verschwunden.map((d) => [d.ip, 'weg'])]);
    const teile = [`${r.geraete.length} gefunden in ${r.bereich}`];
    if (!r.erster_lauf) teile.push(`${r.neu.length} neu`, `${r.verschwunden.length} verschwunden`);
    const kopf = status(r.geraete.length ? 'gruen' : 'orange', teile.join(' · '));
    const erster = r.erster_lauf && el('p', { class: 'fuss', text: 'Erster Lauf in diesem Netz: Das ist der Ausgangsstand, nichts ist „neu“. Ab dem nächsten Lauf siehst du, was dazukommt und was fehlt.' });
    return [kopf, erster, !r.geraete.length && el('p', { class: 'fuss', text: 'Keine Geräte gefunden. Geräte, die jedes Paket verwerfen, bleiben unsichtbar.' })];
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
          laufend ??= Netz.invoke('geraete_suchen', { cidr: null, gross: false }).finally(() => { laufend = null; });
          letztes = await laufend;
          ort.replaceChildren(...ergebnis(letztes).filter(Boolean));
          await ladeKarten();
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
        fortschritt, ort, kartenOrt,
        Object.assign(gruppe('So funktioniert die Suche', [
          zeile('Abgelehnter Port', 'zeigt ein Gerät', E.abgelehnt),
          zeile('Name (PTR)', 'Rückwärts-DNS', E.ptr),
          zeile('MAC-Adresse', 'bleibt gleich', E.mac),
          zeile('Typ eines Geräts', 'eine Vermutung', E.typ),
          zeile('Fehlende Geräte', 'können sich verstecken', E.fehlen),
        ]), { className: 'gruppe erklaer-gruppe' }));
      if (letztes) ort.replaceChildren(...ergebnis(letztes).filter(Boolean));
      ladeKarten();
      if (laufend) suche(); // reattach to the running search
    },
  });
})();
