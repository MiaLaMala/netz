// „Offen“: welche Programme dieses Macs auf einem Port auf Verbindungen warten und wer sie erreicht.
// Der Kern (src/offen.rs) liest die Sockets per libproc, nichts geht ins Netz. Nur „Von außen prüfen“
// probiert TCP-Ports an der eigenen Netzadresse aus, und das nur in einem „meins“-Netz.
(() => {
  const { el, status, meldung, gruppe, zeile, leerZustand } = Netz;
  Netz.icons.offen = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="7" width="11" height="6.5" rx="1.5"/><path d="M5 7V5a3 3 0 0 1 5.8-1"/></svg>';

  const E = {
    netz: '„Netz“ heißt: Das Programm lauscht auf allen Adressen oder auf einer Netzadresse. Jeder im selben WLAN kann den Port ansprechen.',
    lokal: '„Lokal“ heißt: Das Programm lauscht nur auf 127.0.0.1 oder ::1. Nur dieser Mac selbst kommt hin.',
    vpn: '„VPN“ heißt: Die Adresse gehört zu einem Tunnel (utun, Tailscale). Erreichbar sind nur die Teilnehmer des VPN, nicht das WLAN.',
    client: '„UDP-Client“ heißt: ein UDP-Port weit oben ohne bekannten Dienst. Meist hat ein Programm ihn als Client für Antworten gebunden (z. B. DNS). Hält ein Port über mehrere Läufe in diesem Netz, gilt er als Dienst.',
    apple: 'Netzinfo fragt mit `codesign`, ob das Programm auf Apples Zertifikat zurückgeht. Selbst gebaute Programme sind es nie, das ist normal. Bei einem Dienst im Netz ist es einen Blick wert.',
    aussen: '„Von außen prüfen“ verbindet sich zu den TCP-Ports der Netz-Zeilen an der Netzadresse dieses Macs, wie der Portscan. Das geht nur in einem „meins“-Netz.',
    fremd: 'Programme anderer Nutzer und Systemdienste sind ohne Root nicht sichtbar.',
  };
  const ART = { Netz: 'Netz', Lokal: 'lokal', Vpn: 'VPN', UdpClient: 'UDP-Client', Unbekannt: '?' };

  const zustand = { nurNetz: false, aussen: false };
  let liste = null;

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

  function zelle(l) {
    const wo = el('td', {}, el('span', { class: `offen-art ${l.reichweite.toLowerCase()}`, text: ART[l.reichweite] }));
    const hinweise = [
      l.apple_signiert === false && 'nicht von Apple signiert',
      l.von_aussen && 'auch von außen erreichbar',
    ].filter(Boolean);
    return el('tr', {},
      wo,
      el('td', {}, el('div', { text: l.programm || `PID ${l.pid}` }),
        hinweise.length ? el('div', { class: 'fein warn', text: hinweise.join(' · ') }) : null),
      el('td', { class: 'num' }, el('div', { text: `${l.protokoll} ${l.port}` }),
        l.dienst ? el('div', { class: 'fein', text: l.dienst }) : null),
      el('td', { class: 'num fein', text: l.adresse }));
  }

  function zeichne(ort) {
    const alle = liste;
    const netz = alle.filter((l) => l.reichweite === 'Netz');
    const gezeigt = zustand.nurNetz ? netz : alle;
    const auffaellig = netz.filter((l) => l.apple_signiert === false || l.von_aussen).length;
    const kopf = status(netz.length ? 'orange' : 'gruen',
      `${alle.length} lauschend, ${netz.length} für das Netz erreichbar${auffaellig ? `, ${auffaellig} mit Hinweis` : ''}`);
    if (!gezeigt.length) {
      ort.replaceChildren(kopf, leerZustand(Netz.icons.offen,
        zustand.nurNetz ? 'Kein Programm lauscht im Netz. Alles, was offen ist, erreicht nur dieser Mac.' : 'Keine lauschenden Programme von dir gefunden.'));
      return;
    }
    ort.replaceChildren(kopf, el('div', { class: 'karte' }, el('table', { class: 'offen-tabelle' },
      el('thead', {}, el('tr', {}, ['Reichweite', 'Programm', 'Port', 'Adresse'].map((t) => el('th', { scope: 'col', text: t })))),
      el('tbody', {}, gezeigt.map(zelle)))));
  }

  Netz.registriere({
    id: 'offen',
    titel: 'Offen',
    icon: Netz.icons.offen,
    async render(wurzel) {
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'polite' });
      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Offen' }),
          el('p', { class: 'status' }, el('span', { text: 'Welche Programme auf diesem Mac auf Verbindungen warten.' }))),
        el('div', { class: 'werkzeuge' },
          schalter('Nur Netz', 'nurNetz', () => liste && zeichne(ort)),
          schalter('Von außen prüfen', 'aussen', () => Netz.neuLaden())),
        ort,
        Object.assign(gruppe('So entsteht die Liste', [
          zeile('Netz', 'erreichbar für andere', E.netz),
          zeile('Lokal', 'nur dieser Mac', E.lokal),
          zeile('VPN', 'nur über den Tunnel', E.vpn),
          zeile('UDP-Client', 'kein Dienst', E.client),
          zeile('Signatur', 'von Apple oder nicht', E.apple),
          zeile('Von außen', 'Abgleich mit dem Portscan', E.aussen),
          zeile('Sichtbar', 'nur deine Programme', E.fremd),
        ]), { className: 'gruppe erklaer-gruppe' }));
      try {
        liste = await Netz.invoke('offen_zeigen', { aussen: zustand.aussen });
        zeichne(ort);
      } catch (f) {
        ort.replaceChildren(meldung(Netz.text(f), 'rot'));
      }
    },
  });
})();
