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
  const ART = { Netz: 'Netz', Lokal: 'Lokal', Vpn: 'VPN', UdpClient: 'UDP-Client', Unbekannt: 'Unklar' };

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

  // `*` und die Loopback-Adressen sagen Menschen nichts.
  const adresse = (a) => (a === '*' ? 'alle Adressen' : a === '127.0.0.1' || a === '::1' ? 'nur dieser Mac' : a);

  function zelle(l) {
    const hinweise = [
      l.apple_signiert === false && 'nicht von Apple signiert',
      l.von_aussen && 'auch von außen erreichbar',
    ].filter(Boolean);
    return el('tr', {},
      el('td', {}, el('span', { class: `offen-art ${l.reichweite.toLowerCase()}`, text: ART[l.reichweite] })),
      el('td', { class: 'programm' }, el('div', { class: 'kuerzen', title: l.programm, text: l.programm || `Unbekanntes Programm (PID ${l.pid})` }),
        hinweise.length ? el('div', { class: 'fein warn' }, el('span', { class: 'punkt orange' }), el('span', { text: hinweise.join(' · ') })) : null,
        Netz.woher(l.pid, l.programm)),
      el('td', { class: 'num' }, el('div', { text: `${l.protokoll} ${l.port}` }),
        l.dienst ? el('div', { class: 'fein', text: l.dienst }) : null),
      el('td', { class: 'fein num adresse', text: adresse(l.adresse) }));
  }

  // Der Kopf trägt die Aussage; ein UDP-Client lauscht nicht und zählt nicht mit.
  function kopfSatz(alle) {
    const dienste = alle.filter((l) => l.reichweite !== 'UdpClient').length;
    const netz = alle.filter((l) => l.reichweite === 'Netz').length;
    if (!dienste) return 'Kein Programm lauscht';
    if (!netz) return 'Kein Programm ist fürs Netz erreichbar';
    return `${netz} von ${dienste} Programmen ${netz === 1 ? 'ist' : 'sind'} fürs Netz erreichbar`;
  }

  function zeichne(ort, titel, unter) {
    const alle = liste;
    const netz = alle.filter((l) => l.reichweite === 'Netz');
    const gezeigt = zustand.nurNetz ? netz : alle;
    const auffaellig = netz.filter((l) => l.apple_signiert === false || l.von_aussen).length;
    titel.textContent = kopfSatz(alle);
    unter.replaceChildren(...(alle.length
      ? status(auffaellig ? 'orange' : 'gruen', auffaellig ? `${auffaellig} mit Hinweis` : 'Keine Hinweise').children
      : []));
    if (!gezeigt.length) {
      ort.replaceChildren(leerZustand(Netz.icons.offen,
        !alle.length ? 'Keine lauschenden Programme von dir gefunden. Programme anderer Nutzer sind ohne Root nicht sichtbar.'
          : 'Kein Programm lauscht im Netz. Der Rest erreicht nur dieser Mac oder das VPN.', 'offen'));
      return;
    }
    // Ohne Rückmeldung wüsste man nach „Von außen prüfen“ nicht, ob die Prüfung lief.
    const tcp = netz.filter((l) => l.protokoll === 'TCP');
    const geprueft = zustand.aussen && tcp.length ? el('p', { class: 'fuss', text: `Von außen geprüft: ${tcp.filter((l) => l.von_aussen).length} von ${tcp.length} TCP-Ports im Netz sind auch von außen offen.` }) : null;
    ort.replaceChildren(geprueft ?? '', el('div', { class: 'karte' }, el('table', { class: 'offen-tabelle' },
      el('thead', {}, el('tr', {}, ['Reichweite', 'Programm', 'Port', 'Adresse'].map((t) => el('th', { scope: 'col', text: t })))),
      el('tbody', {}, gezeigt.map(zelle)))));
  }

  Netz.registriere({
    id: 'offen',
    titel: 'Offen',
    icon: Netz.icons.offen,
    async render(wurzel) {
      const ort = el('div', { class: 'ergebnis', 'aria-live': 'polite' });
      const titel = el('h2', { text: 'Wird gelesen …' });
      const unter = el('p', { class: 'status' });
      const pruefen = el('button', { class: 'knopf', type: 'button', text: zustand.aussen ? 'Erneut prüfen' : 'Von außen prüfen' });
      pruefen.addEventListener('click', () => {
        zustand.aussen = true;
        pruefen.disabled = true;
        pruefen.textContent = 'Prüfe …';
        Netz.neuLaden();
      });
      wurzel.append(
        el('div', { class: 'kopf' }, titel, unter),
        el('div', { class: 'werkzeuge' },
          el('div', { class: 'werkzeug' }, schalter('Nur Netz', 'nurNetz', () => liste && zeichne(ort, titel, unter)),
            el('p', { class: 'fein', text: 'Blendet Programme aus, die nur dieser Mac erreicht. Ändert nur die Liste.' })),
          el('div', { class: 'werkzeug' }, pruefen,
            el('p', { class: 'fein', text: 'Verbindet sich von diesem Mac aus mit den TCP-Ports der Netz-Zeilen und zeigt, welche auch von außen offen sind. Nur in einem „meins“-Netz.' }))),
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
        zeichne(ort, titel, unter);
      } catch (f) {
        // Scheitert nur die Prüfung von außen, bleibt die Liste sichtbar.
        const aussenFehler = zustand.aussen;
        zustand.aussen = false; // sonst scheitert jedes Neuladen am selben Grund
        try {
          if (!aussenFehler) throw f;
          liste = await Netz.invoke('offen_zeigen', { aussen: false });
          zeichne(ort, titel, unter);
          ort.prepend(meldung(`Die Prüfung von außen ging nicht: ${Netz.text(f)}`, 'orange'));
          pruefen.disabled = false;
          pruefen.textContent = 'Von außen prüfen';
        } catch {
          titel.textContent = 'Offen';
          ort.replaceChildren(Netz.fehlerKarte(Netz.text(f)));
        }
      }
    },
  });
})();
