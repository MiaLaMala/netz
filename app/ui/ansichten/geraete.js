// „Geräte“: eine Karte je gemerktem Gerät (Typ, Hersteller, Dienste, Hinweise), „Suchen“ frischt auf.
// Die Schutzregel („meins“) steckt im Kern (src/geraete.rs). Hier wird nur ihre Ablehnung gezeigt.
(() => {
  const { el, zeit, status, meldung, gruppe, zeile, leerZustand } = Netz;

  const DIENSTE = {
    22: 'SSH', 53: 'DNS', 80: 'Web', 443: 'Web (TLS)', 445: 'Dateifreigabe (SMB)', 548: 'Dateifreigabe (AFP)',
    631: 'Drucker (IPP)', 7000: 'AirPlay', 8009: 'Cast', 9100: 'Drucker (RAW)', 62078: 'iPhone-Sync',
  };
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

  // Type icons in the line style of the sidebar; the core also sends an emoji (`symbol`), which is not shown.
  const SYMBOLE = {
    Router: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="14" width="18" height="6" rx="2"/><path d="M7 17h.01M11 17h.01M8 11a6 6 0 0 1 8 0M5.5 8.5a10 10 0 0 1 13 0"/></svg>',
    Drucker: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 9V4h10v5M7 17H4V9h16v8h-3"/><rect x="7" y="14" width="10" height="6" rx="1"/></svg>',
    Medien: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="5" width="18" height="12" rx="2"/><path d="M8 21h8M12 17v4"/></svg>',
    'Smart Home': '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 11l8-7 8 7M6 9.5V20h12V9.5"/><path d="M10 20v-5h4v5"/></svg>',
    Computer: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="5" width="16" height="11" rx="1.5"/><path d="M2 20h20"/></svg>',
    Handy: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/></svg>',
    Unbekannt: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5M12 16.5h.01"/></svg>',
  };

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

  // The core words some hints that the card already shows elsewhere (random address as a chip,
  // visits and dates in the time line). Here they are dropped instead of said twice.
  const doppelt = (k, h) => (k.mac_zufaellig && h.includes('zufällige MAC')) || /^wurde erst einmal gesehen/.test(h) || /^zuletzt vor .* gesehen$/.test(h);

  // Open ports and Bonjour services in one list: a port with its service name, a service without a port after it.
  function angebote(k) {
    const nachPort = new Map((k.ports ?? []).map((p) => [p, []]));
    const ohnePort = [];
    for (const d of k.dienste) {
      const name = d.instanz || d.typ;
      if (!d.port) ohnePort.push(name);
      else nachPort.set(d.port, [...(nachPort.get(d.port) ?? []), name]);
    }
    return [...nachPort].sort((x, y) => x[0] - y[0]).map(([port, namen]) => [String(port), DIENSTE[port] ?? 'unbekannter Dienst', [...new Set(namen)].join(', ')])
      .concat(ohnePort.map((n) => ['', n, '']));
  }

  function zeiten(k) {
    return k.besuche <= 1 ? `Einmal gesehen am ${zeit(k.last_seen)}`
      : `Seit ${zeit(k.first_seen)}, zuletzt ${zeit(k.last_seen)}, ${k.besuche} Besuche`;
  }

  function karte(k) {
    const tag = markiert.get(k.ip);
    const wort = tag === 'neu' ? ['gruen', 'neu'] : tag === 'weg' ? ['orange', 'nicht gefunden'] : null;
    const formular = el('div', { class: 'karte-bearbeiten' });
    const hinweise = k.hinweise.filter((h) => !doppelt(k, h));
    const liste = angebote(k);
    // One grid: the symbol column runs through the whole card, so all text shares one left edge.
    return el('article', { class: 'karte', 'aria-label': k.anzeigename },
      el('span', { class: 'karte-symbol', html: SYMBOLE[k.typ] ?? SYMBOLE.Unbekannt, 'aria-hidden': 'true' }),
      el('div', { class: 'karte-kopf' },
        el('div', { class: 'text' },
          el('div', {}, el('span', { class: 'netz-name', text: k.anzeigename }),
            k.namequelle && el('span', { class: 'marke', text: ` · ${k.namequelle}` })),
          el('div', { class: 'fein' },
            el('span', { text: k.hersteller || 'Hersteller unbekannt' }),
            k.mac_zufaellig && el('span', { class: 'zufall' }, el('span', { html: ZUFALL }), el('span', { text: 'Zufällige Adresse' })))),
        el('span', { class: 'karte-typ', text: k.typ }),
        wort && el('span', { class: 'status geraet-status' }, el('span', { class: `punkt ${wort[0]}` }), el('span', { text: wort[1] }))),
      el('div', { class: 'fein num', text: [k.ip, k.mac && `MAC ${k.mac}`].filter(Boolean).join(' · ') }),
      k.upnp && el('div', { class: 'fein', text: `UPnP: ${[k.upnp.modell, k.upnp.geraetetyp?.split(':')[3]].filter(Boolean).join(' · ')}` }),
      el('div', { class: 'fein', text: zeiten(k) }),
      liste.length > 0 && el('ul', { class: 'karte-ports', 'aria-label': 'Offene Ports und Dienste' },
        liste.map(([port, name, wer]) => el('li', {}, el('span', { class: 'num', text: port }), el('span', {}, name, wer && el('span', { class: 'marke', text: ` · ${wer}` }))))),
      hinweise.length > 0 && el('ul', { class: 'karte-hinweise', 'aria-label': 'Hinweise' }, hinweise.map((h) => el('li', { text: h }))),
      el('p', { class: 'erklaerung', text: k.erklaerung }),
      el('div', { class: 'karte-aktionen' },
        k.gateway && routerKnoepfe(),
        el('button', {
          class: 'knopf leise', type: 'button', text: 'Name und Typ ändern',
          onclick: (e) => {
            e.currentTarget.hidden = true;
            formular.replaceChildren(bearbeiten(k, () => { formular.replaceChildren(); e.currentTarget.hidden = false; }));
            formular.querySelector('input').focus();
          },
        })),
      k.gateway && routerOrt,
      formular);
  }

  // Router (Rust: router::Ergebnis, portfreigaben::Anzeige): two read-only looks at the gateway,
  // shown inside the router card. Both send, so both wait for a click.
  const E_ROUTER = 'Netzinfo ruft die Startseite des Routers einmal über http ab und lässt sich über https das Zertifikat zeigen. Gelesen wird nur die Kennung des Webservers, der Seitentitel und das Zertifikat. Es gibt kein Anmelden und kein Ausprobieren anderer Seiten. Ein selbst ausgestelltes Zertifikat ist bei Routern üblich und kein Zeichen für einen Angriff.';
  const E_FREIGABEN = 'Jede Zeile ist eine offene Tür: Was aus dem Internet an diesen Port kommt, geht an das Gerät im Heimnetz. Spielkonsolen legen so etwas oft selbst an, zum Beispiel Port 3074. Netzinfo fragt den Router per UPnP und kann nichts ändern. Von Hand im Router angelegte Freigaben erscheinen hier nicht.';
  const portStatus = (s) => (s === 'offen' ? 'erreichbar' : s === 'zu' ? 'geschlossen' : s?.fehler ?? 'unbekannt');
  let routerErgebnis = null;
  let freigabenErgebnis = null;
  const seiteOrt = el('div', { class: 'router-seite', 'aria-live': 'polite' });
  const freigabenOrt = el('div', { class: 'router-seite', 'aria-live': 'polite' });
  const routerOrt = el('div', { class: 'karte-router' }, seiteOrt, freigabenOrt);

  function zeigeSeite(r) {
    const s = r.seite;
    const z = s.zertifikat;
    seiteOrt.replaceChildren(...[
      el('p', { class: 'router-titel', text: 'Router-Webseite' }),
      el('div', { class: 'fein', text: `http (80): ${portStatus(s.http)}${s.http === 'offen' ? (s.umleitung ? ', leitet auf https um' : ', ohne Umleitung auf https') : ''}` }),
      el('div', { class: 'fein', text: `https (443): ${portStatus(s.https)}` }),
      z && el('div', { class: 'fein', text: `Zertifikat bis ${z.gueltig_bis}${z.abgelaufen ? ' (abgelaufen)' : ''}${z.selbstsigniert ? ', selbst ausgestellt' : ''}` }),
      s.server && el('div', { class: 'fein', text: `Server: ${s.server}` }),
      s.titel && el('div', { class: 'fein', text: `Seitentitel: ${s.titel}` }),
      r.hinweise.length > 0 && el('ul', { class: 'karte-hinweise', 'aria-label': 'Hinweise zur Router-Seite' }, r.hinweise.map((h) => el('li', { text: h }))),
      el('p', { class: 'erklaerung', text: E_ROUTER })].filter(Boolean));
  }

  function zeigeFreigaben(a) {
    const doppelNat = a.externe_ip_klasse === 'privat' || a.externe_ip_klasse === 'cgnat';
    const alarm = a.befunde.some(([schlimm]) => schlimm);
    let kopf;
    if (!a.upnp) kopf = [status('grau', 'UPnP aus'), el('div', { class: 'fein', text: 'Der Router antwortet nicht auf UPnP. Dann kann kein Programm selbst Ports ins Internet öffnen. Von Hand angelegte Freigaben siehst du hier nicht.' })];
    else if (a.liste_nur_mit_login) kopf = [status('orange', 'Liste nur mit Login'), el('div', { class: 'fein', text: 'Der Router zeigt seine Portfreigaben erst nach Anmeldung (bei der Fritz!Box üblich). Schau dort in der Oberfläche nach.' })];
    else if (a.listenfehler) kopf = [status('rot', 'Liste nicht lesbar'), el('div', { class: 'fein', text: a.listenfehler })];
    else if (!a.freigaben.length) kopf = [status('gruen', 'Keine Portfreigaben')];
    else kopf = [status(alarm ? 'rot' : 'orange', `${a.freigaben.length} ${a.freigaben.length === 1 ? 'Portfreigabe' : 'Portfreigaben'}`)];
    freigabenOrt.replaceChildren(...[
      el('p', { class: 'router-titel', text: 'Portfreigaben' }),
      ...kopf,
      a.freigaben.length > 0 && el('ul', { class: 'karte-ports freigaben', 'aria-label': 'Portfreigaben' },
        a.freigaben.map((f) => el('li', {},
          el('span', { class: 'num', text: f.extern }),
          el('span', {}, `${f.protokoll} → ${f.intern_ip}:${f.intern_port}`,
            el('span', { class: 'marke', text: ` · ${[DIENSTE[f.extern], f.beschreibung?.toLowerCase() === DIENSTE[f.extern]?.toLowerCase() ? '' : f.beschreibung, f.aktiv ? '' : 'ausgeschaltet'].filter(Boolean).join(', ') || 'ohne Beschreibung'}` }))))),
      a.upnp && !a.listenfehler && !a.liste_nur_mit_login && el('div', { class: 'fein', text: `Verbindung: ${a.verbindung}. Externe Adresse: ${a.externe_ip_klasse}${doppelNat ? ', der Router hat keine öffentliche Adresse (Doppel-NAT), Freigaben bringen nichts' : ''}.` }),
      ...a.befunde.map(([schlimm, satz]) => meldung(satz, schlimm ? 'rot' : 'orange')),
      el('p', { class: 'erklaerung', text: E_FREIGABEN })].filter(Boolean));
  }

  function routerKnoepfe() {
    const lauf = (knopf, ort, ruf, zeige, ergebnis) => async () => {
      knopf.disabled = true;
      try {
        zeige(await ruf());
      } catch (f) {
        ort.replaceChildren(meldung(Netz.text(f), f?.art === 'nicht_meins' ? 'orange' : 'rot'));
      }
      knopf.disabled = false;
    };
    const seite = el('button', { class: 'knopf', type: 'button', text: 'Sicherheit der Router-Webseite prüfen' });
    const freigaben = el('button', { class: 'knopf', type: 'button', text: 'Portfreigaben ansehen' });
    seite.addEventListener('click', lauf(seite, seiteOrt, async () => (routerErgebnis = await Netz.invoke('router_seite')), zeigeSeite));
    freigaben.addEventListener('click', lauf(freigaben, freigabenOrt, async () => (freigabenErgebnis = await Netz.invoke('portfreigaben')), zeigeFreigaben));
    if (routerErgebnis) zeigeSeite(routerErgebnis);
    if (freigabenErgebnis) zeigeFreigaben(freigabenErgebnis);
    return el('span', { class: 'router-knoepfe' }, seite, freigaben);
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
      kartenOrt.replaceChildren(leerZustand(Netz.icons.geraete, 'Noch keine Geräte gemerkt. „Netz absuchen“ legt für jedes gefundene Gerät eine Karte an.'));
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
    aktualisieren: false, // „Netz absuchen“ lädt die Karten ebenfalls neu
    async render(wurzel) {
      const knopf = el('button', { class: 'knopf haupt', type: 'button', text: letztes ? 'Netz noch mal absuchen' : 'Netz absuchen' });
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
        el('p', { class: 'fuss aktion-hinweis', text: '„Netz absuchen“ tastet das Netz ab und legt neue Karten an.' }),
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
