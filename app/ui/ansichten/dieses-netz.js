// „Dieses Netz“: was der Mac gerade vom Netz weiß, und ob er es kennt.
// Die Erklärsätze folgen den Zeilen von `netzinfo hier --erklaer` (src/hier.rs).
(() => {
  const { el, zeit, gruppe, zeile, meldung } = Netz;

  const E = {
    schnittstelle: (vpn) =>
      'Die Default-Route sagt, über welche Schnittstelle Pakete ins Internet gehen. ' +
      (vpn
        ? '„utun“ ist ein Tunnel, also ein VPN wie Tailscale oder WireGuard: Dein Verkehr geht gerade durch den Tunnel, nicht direkt ins WLAN.'
        : '„en0“ ist beim Mac meistens das WLAN, andere „en“-Nummern sind Kabel oder Adapter.'),
    ip: (dhcp) =>
      dhcp
        ? 'Die IPv4-Adresse hat dir der DHCP-Server geliehen. Im Heimnetz ist sie privat (10.x, 172.16 bis 172.31, 192.168.x) und wird per NAT ins Internet übersetzt.'
        : 'Zu dieser Adresse gibt es keine DHCP-Antwort: Sie kommt von der Gegenstelle eines Tunnels (z. B. Tailscale oder WireGuard) oder ist fest eingestellt.',
    maske: 'Die Maske trennt Netzanteil und Hostanteil. Die Zahl nach dem Schrägstrich sagt, wie viele Bits vorn zum Netz gehören.',
    netzadresse: 'IP UND Maske ergibt die Netzadresse. Sie benennt das ganze Netz und ist kein Gerät.',
    broadcast: 'Alle Hostbits auf 1 ergeben den Broadcast. Ein Paket an diese Adresse erreicht alle Geräte im Netz.',
    hostbereich: 'Zwischen Netzadresse und Broadcast liegen die Adressen, die Geräte bekommen können.',
    router: 'Der Router (Gateway) ist die Tür aus dem eigenen Subnetz. Alles, was nicht im Subnetz liegt, schickt der Mac an ihn.',
    dhcp: 'Der DHCP-Server vergibt IP, Maske, Router und DNS (Ablauf DORA: Discover, Offer, Request, Ack). Im Heimnetz ist er fast immer der Router.',
    dhcpDns: 'DNS übersetzt Namen in IP-Adressen. Diese Server hat das Netz per DHCP angeboten.',
    systemDns: (magic) =>
      magic
        ? '100.100.100.100 ist der DNS-Dienst von Tailscale (MagicDNS). Er sitzt vor dem DNS aus dem DHCP und reicht normale Fragen weiter.'
        : 'Das ist, was der Mac wirklich fragt. Meist stimmt es mit dem DNS vom DHCP überein.',
    domain: 'Die Domain vom DHCP-Server wird an unvollständige Namen angehängt (Suchdomain).',
    verschl: 'Normales DNS ist im Klartext lesbar. Ich prüfe, ob dein Netz verschlüsseltes DNS durchlässt.',
    dot: {
      Erreichbar: 'DNS über TLS ist erreichbar. Deine Namensanfragen können in diesem Netz nicht mitgelesen werden, wenn du es einschaltest.',
      Blockiert: 'Port 853 ist gesperrt. Das ist in vielen Firmennetzen normal und kein Zeichen für einen Angriff.',
      Abgefangen: 'Der Resolver zeigt ein Zertifikat, das nicht zu ihm gehört: Das Netz lenkt die Verbindung um oder jemand fängt sie ab. Hinter einem Captive Portal ist das normal.',
    },
    doh: 'DNS über HTTPS läuft auf Port 443 wie normales Websurfen und ist deshalb schwerer zu sperren als DNS über TLS.',
    ddr: 'Fragt der Mac den DNS-Server des Netzes nach _dns.resolver.arpa, kann dieser einen verschlüsselten Zugang nennen. Die Antwort ist nicht beglaubigt, ich zeige sie nur an.',
  };

  // Mehrere Werte (DNS-Server) stehen untereinander, nicht in einer Zeile.
  const stapel = (label, a, erkl) => {
    const z = zeile(label, a && a.length ? a.join('\n') : null, erkl);
    z.querySelector('.wert').classList.add('stapel');
    return z;
  };
  // Farbe nur für eine Bewertung (gut, mittel, schlecht); das Wort steht immer im Wert.
  const bewertet = (z, klasse) => {
    if (klasse) z.querySelector('.wert').classList.add(klasse);
    return z;
  };
  // Nach dem Neuzeichnen steht der Fokus an der neuen Überschrift (sonst im Nichts am Seitenanfang).
  const zumKopf = () => document.querySelector("[data-ansicht='dieses-netz'] h2")?.focus();
  // Seltene Werte hinter ein Aufklappen (`details` ist mit der Tastatur bedienbar).
  const mehr = (titel, zeilen) => el('details', { class: 'mehr' }, el('summary', { text: titel }), el('dl', {}, zeilen));

  // Schwellen für Signal (dBm): ab -70 gut, ab -80 mittel.
  const staerkeVon = (rssi) => (rssi == null ? null : rssi >= -70 ? 'gut' : rssi >= -80 ? 'mittel' : 'schwach');

  function kopf(d, funk) {
    const { netz } = d;
    const zeile2 = (farbe, wort, rest) => el('p', { class: 'status' },
      el('span', { class: `punkt ${farbe}`, 'aria-hidden': 'true' }), el('span', { class: 'status-wort', text: wort }),
      rest && el('span', { class: 'status-rest', text: rest }));
    // Zweite Zeile: das Funksignal auf einen Blick (die Zahlen stehen in der WLAN-Gruppe).
    const st = funk && staerkeVon(funk.rssi_dbm);
    const signal = st && zeile2({ gut: 'gruen', mittel: 'orange', schwach: 'rot' }[st], `WLAN-Signal ${st}`);
    if (netz) {
      return el('div', { class: 'kopf' }, el('h2', { tabindex: '-1', text: netz.name }),
        zeile2('gruen', netz.meins ? 'Bekanntes Netz, meins' : 'Bekanntes Netz', `zuletzt hier ${zeit(netz.last_seen)}`), signal);
    }
    return el('div', { class: 'kopf' }, el('h2', { tabindex: '-1', text: 'Unbekanntes Netz' }),
      d.erkennung_fehler ? zeile2('grau', 'Nicht wiedererkennbar') : zeile2('grau', 'Noch nicht gemerkt'), signal);
  }

  // Inline-Formular statt Dialog: Name, „meins“, Merken.
  function merkenFormular(ort, nachher) {
    const name = el('input', { type: 'text', id: 'netz-name', placeholder: 'z. B. WG', autocomplete: 'off', spellcheck: 'false' });
    const meins = el('input', { type: 'checkbox', id: 'netz-meins' });
    const fehler = el('div', { id: 'netz-fehler' });
    const merken = el('button', { class: 'knopf haupt', type: 'submit', text: 'Merken', 'aria-label': 'Dieses Netz merken' });
    const zu = () => {
      ort.replaceChildren(offenKnopf());
      ort.querySelector('button').focus();
    };
    const form = el('form', { class: 'formular', novalidate: true, 'aria-label': 'Dieses Netz merken' },
      el('div', { class: 'liste' },
        el('div', { class: 'zeile' }, el('label', { class: 'label', for: 'netz-name', text: 'Name' }), name),
        el('div', { class: 'zeile' },
          el('label', { class: 'haken' }, meins, el('span', { text: 'Das ist mein Netz' })),
          el('p', { class: 'hilfe', id: 'netz-hilfe', text: 'Erlaubt die Gerätesuche in diesem Netz. Markiere nur Netze, die dir gehören.' }))),
      fehler,
      el('div', { class: 'knoepfe' },
        el('button', { class: 'knopf', type: 'button', text: 'Abbrechen', onclick: zu }),
        merken));
    meins.setAttribute('aria-describedby', 'netz-hilfe');
    form.addEventListener('keydown', (e) => { if (e.key === 'Escape') zu(); });
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      fehler.replaceChildren();
      name.removeAttribute('aria-invalid');
      name.removeAttribute('aria-describedby');
      merken.disabled = true;
      try {
        await Netz.invoke('netz_merken', { name: name.value, meins: meins.checked });
        await nachher();
        zumKopf();
      } catch (err) {
        fehler.replaceChildren(meldung(Netz.text(err)));
        name.setAttribute('aria-invalid', 'true');
        name.setAttribute('aria-describedby', 'netz-fehler');
        merken.disabled = false;
        name.focus();
      }
    });
    return form;
  }

  // Karte „WLAN“ (#34). Die Sätze folgen `netzinfo wlan --erklaer` (src/wlan.rs).
  const SICHER = {
    Offen: 'offen (unverschlüsselt)', Wep: 'WEP (unsicher)', Wpa: 'WPA (veraltet)', Wpa2: 'WPA2', Wpa3: 'WPA3',
    Wpa2Wpa3Gemischt: 'WPA2/WPA3 gemischt', Unbekannt: 'unbekannt',
  };
  const SICHER_SATZ = {
    Offen: 'Das WLAN ist offen: Jeder in Reichweite kann mitlesen. Nutze nur verschlüsselte Verbindungen oder ein VPN.',
    Wep: 'WEP gilt seit Jahren als geknackt, es schützt kaum. Stelle den Router auf WPA2 oder WPA3 um.',
    Wpa: 'WPA (die erste Version) ist veraltet. Stelle den Router auf WPA2 oder WPA3 um.',
  };
  const wlanKarte = (w) => {
    const sicher = w.sicherheit ? SICHER[w.sicherheit] : null;
    const staerke = staerkeVon(w.rssi_dbm);
    const signal = bewertet(zeile('Signal', staerke && `${w.rssi_dbm} dBm, ${staerke}`,
      w.rssi_dbm == null ? null : `Das Signal liegt bei minus ${-w.rssi_dbm} dBm. Je näher an null, desto besser, ab minus 70 wird es merklich schlechter.`),
    { gut: 'gut', mittel: 'mittel', schwach: 'schlecht' }[staerke]);
    const sz = bewertet(zeile('Sicherheit', sicher, SICHER_SATZ[w.sicherheit] || (sicher && w.sicherheit !== 'Unbekannt' ? 'Die Verschlüsselung ist zeitgemäß.' : 'Die Art der Verschlüsselung ließ sich nicht auslesen.')),
      ['Offen', 'Wep', 'Wpa'].includes(w.sicherheit) && 'schlecht');
    const g = gruppe('WLAN', [
      ...(w.ssid ? [zeile('Name (SSID)', w.ssid, 'Der Name, unter dem das WLAN sich meldet. Zwei verschiedene Netze können denselben Namen tragen, der Name allein beweist also nichts.')] : []),
      signal,
      sz,
      zeile('Senderate', w.rate_mbit == null ? null : `${Math.round(w.rate_mbit)} MBit/s`, 'Die Senderate ist das, was Mac und Router gerade aushandeln, nicht die echte Geschwindigkeit. Echt kommt meist die Hälfte oder weniger an.'),
    ]);
    g.append(mehr('Band, Kanal, Rauschen und mehr', [
      ...(w.ssid ? [zeile('Kennung (BSSID)', w.bssid, 'Die Hardware-Adresse des Funkgeräts, mit dem du verbunden bist. Gespeichert wird davon nur ein Hash, nie die Adresse selbst.')] : []),
      zeile('Standard', w.standard, 'Der Standard (Wi-Fi 4, 5, 6) bestimmt, wie schnell die Funkstrecke höchstens sein kann. Neuere Standards sind schneller und gehen besser mit vielen Geräten um.'),
      zeile('Band', w.band, '2,4 GHz reicht weiter durch Wände, 5 und 6 GHz sind schneller, brauchen aber Nähe zum Router.'),
      zeile('Kanal', w.kanal, 'Auf dem Kanal funkt dein WLAN. Teilen sich viele Nachbarnetze denselben Kanal, stören sie sich gegenseitig.'),
      zeile('Kanalbreite', w.breite_mhz == null ? null : `${w.breite_mhz} MHz`, 'Breitere Kanäle sind schneller, aber störanfälliger und belegen mehr vom Band.'),
      zeile('Rauschen', w.rauschen_dbm == null ? null : `${w.rauschen_dbm} dBm`, 'Rauschen ist der Störpegel auf dem Kanal. Je weiter von null weg, desto besser. Manche Treiber melden es nicht, dann bleibt das Feld leer.'),
      zeile('Abstand zum Rauschen', w.snr_db == null ? null : `${w.snr_db} dB`,
        w.snr_db == null ? null : `Der Abstand zum Rauschen beträgt ${w.snr_db} dB. Über 25 dB gilt als gut für Videoanrufe.`),
      zeile('Land', w.land, 'Das Land bestimmt, welche Kanäle und welche Sendeleistung erlaubt sind.'),
    ]));
    return g;
  };

  // Standortfreigabe (#35): macOS gibt Netzname und Kennung nur mit Standortfreigabe heraus.
  const ORT_WARUM = 'macOS behandelt den Namen eines WLANs als Ortsangabe, denn aus ihm lässt sich ableiten, wo du bist. Deshalb zeigt es Name und Kennung nur Apps mit Standortfreigabe. Netzinfo liest damit nur das WLAN, mit dem du verbunden bist, und merkt sich keinen Standort. Ohne Freigabe erkennt Netzinfo Netze etwas ungenauer, alles andere geht weiter.';
  function ortKarte(status) {
    if (status === 'Erlaubt') return null;
    const aktion = (text, fn, label) => el('button', { class: 'knopf', type: 'button', text, 'aria-label': label, onclick: fn });
    const fehler = el('div');
    const nachrichten = {
      NichtGefragt: 'Netzinfo hat noch nicht nach dem Standort gefragt, deshalb zeigt macOS Name und Kennung des WLANs nicht.',
      Abgelehnt: 'Der Standort ist abgelehnt. Netzinfo funktioniert weiter, erkennt Netze aber ungenauer. Ändern geht in den Systemeinstellungen unter Datenschutz & Sicherheit > Ortungsdienste.',
      Eingeschraenkt: 'macOS beschränkt den Standort auf diesem Mac (z. B. durch Bildschirmzeit oder ein Profil). Netzinfo kann ihn nicht selbst freigeben.',
    };
    const knopf = status === 'NichtGefragt'
      ? aktion('Standort freigeben …', async (e) => {
        const knopfEl = e.currentTarget;
        knopfEl.disabled = true;
        knopfEl.textContent = 'Warte auf deine Antwort …';
        try {
          await Netz.invoke('ort_anfragen');
          // Die Antwort kommt erst nach dem Klick im Systemdialog: kurz warten und neu lesen.
          for (let i = 0; i < 60 && (await Netz.invoke('ort_status')) === 'NichtGefragt'; i++) await new Promise((r) => setTimeout(r, 1000));
          await Netz.neuLaden();
          zumKopf();
        } catch (err) {
          fehler.replaceChildren(meldung(Netz.text(err)));
          knopfEl.disabled = false;
          knopfEl.textContent = 'Standort freigeben …';
          knopfEl.focus();
        }
      }, 'Netzinfo darf den Standort nutzen: Freigabe anfragen')
      : status === 'Abgelehnt'
        ? aktion('Systemeinstellungen öffnen', () => Netz.invoke('ort_einstellungen_oeffnen').catch((err) => fehler.replaceChildren(meldung(Netz.text(err)))), 'Systemeinstellungen öffnen, Ortungsdienste')
        : null;
    return el('section', { class: 'gruppe ort-karte', 'aria-labelledby': 'g-ort' },
      el('h3', { id: 'g-ort', text: 'Standortfreigabe' }),
      el('div', { class: 'liste' },
        el('div', { class: 'zeile' }, el('p', { class: 'ort-status', text: nachrichten[status] || nachrichten.NichtGefragt })),
        el('details', { class: 'zeile' }, el('summary', { text: 'Warum fragt Netzinfo nach dem Standort?' }), el('p', { class: 'hilfe', text: ORT_WARUM })),
        knopf && el('div', { class: 'zeile aktionszeile' }, knopf, fehler)));
  }

  function offenKnopf() {
    return el('button', { class: 'knopf haupt', type: 'button', text: 'Dieses Netz merken …', onclick: oeffne });
  }
  function oeffne(e) {
    const ort = e.currentTarget.parentElement;
    ort.replaceChildren(merkenFormular(ort, Netz.neuLaden));
    ort.querySelector('input[type="text"]').focus();
  }

  // Proxy und WPAD (#53): rein lesend. Ohne Proxy genügt eine Zeile in „Router und Dienste“; erst mit
  // Eintrag kommt die ganze Gruppe. Die PAC-Datei wird nur auf Klick geladen, gezeigt werden nur Hostnamen.
  function proxyZeile(dienste) {
    const liste = dienste.querySelector('dl');
    const erkl = 'Ein Proxy ist ein Zwischenrechner, über den dein Mac Webseiten laden soll. Der Betreiber könnte deine Webanfragen sehen. In einem Heimnetz gibt es normalerweise keinen, ein neuer Eintrag ist ein Grund zum Nachsehen.';
    const platz = zeile('Proxy', 'wird geprüft …', erkl);
    liste.append(platz);
    Netz.invoke('proxy_lesen').then((b) => {
      if (!b.manuell && !b.pac_url && !b.wpad_dhcp && !b.wpad_dns) return platz.replaceWith(zeile('Proxy', 'keiner', erkl));
      platz.remove();
      const neu = gruppe('Proxy', [
        zeile('Fester Proxy', b.manuell || 'keiner', erkl),
        zeile('PAC-Datei', b.pac_url || 'keine', 'Die PAC-Datei sagt dem Browser, welcher Proxy für welche Adresse gilt. Ich führe sie nie aus.'),
        zeile('WPAD per DHCP', b.wpad_dhcp || 'keiner', 'Das Netz kann dem Mac mit DHCP-Option 252 die Adresse einer PAC-Datei mitgeben (WPAD).'),
        zeile('WPAD per DNS', b.wpad_dns ? 'gefunden' : 'nicht gefunden', 'Ich frage den DNS-Server des Netzes nach wpad.<Domain>. Ein Treffer ist nur ein Hinweis, die Datei lade ich nicht.'),
      ]);
      if (b.pac_url) {
        const hosts = el('div', { 'aria-live': 'polite' }); // Ergebnis kommt einmal, nach dem Klick
        neu.querySelector('dl').append(el('div', { class: 'zeile' }, el('dt', { class: 'label', text: 'Proxy-Hostnamen' }), el('dd', { class: 'wert' }, el('button', { class: 'knopf', type: 'button', 'data-pac': '', text: 'PAC-Datei ansehen', 'aria-label': 'PAC-Datei ansehen, Proxy-Hostnamen auflisten', onclick: (e) => {
          e.currentTarget.disabled = true;
          Netz.invoke('proxy_pac_ansehen').then((h) => hosts.replaceChildren(el('p', { class: 'fuss', text: h.length ? `Hostnamen in der PAC-Datei: ${h.join(', ')}` : 'In der PAC-Datei stehen keine Proxy-Hostnamen.' })))
            .catch((err) => hosts.replaceChildren(meldung(Netz.text(err), 'orange')));
        } }), hosts)));
      }
      dienste.after(neu);
    }).catch(() => platz.replaceWith(zeile('Proxy', 'nicht lesbar', erkl)));
  }

  // IPv6-Router (#68): eine Zeile unter dem Router; bei einem Fehler oder ohne App-Befehl bleibt sie weg.
  async function ipv6Zeile(dienste) {
    const r = await Netz.invoke('ipv6_router').catch(() => null);
    if (!r) return;
    const fremd = r.anzahl === 1 && r.passt_zu_gateway === false;
    const wert = r.anzahl === 0 ? 'keiner' : r.anzahl > 1 ? `${r.anzahl} Geräte` : fremd ? 'ein anderes Gerät' : r.passt_zu_gateway ? 'dein Router' : 'ein Gerät';
    dienste.querySelector('.zeile').after(zeile('IPv6-Router', wert, r.anzahl === 0
      ? 'Ohne IPv6 im Netz meldet sich kein Gerät als IPv6-Router. Das ist ruhig und kein Fehler.'
      : fremd
        ? 'Ein Gerät meldet sich als IPv6-Router, das nicht dein Router (Gateway) ist. Zu Hause ein Grund zum Nachsehen, in Firmen- und Mesh-Netzen oft normal.'
        : r.anzahl === 1
          ? 'Nur ein Gerät meldet sich als IPv6-Router. Das ist der Normalfall.'
        : 'In diesem Netz melden sich mehrere Geräte als IPv6-Router. Zu Hause sollte nur dein Router das tun; in Firmen- und Mesh-Netzen ist es normal.'));
  }

  Netz.registriere({
    id: 'dieses-netz',
    teil: true,
    titel: 'Dieses Netz',
    icon: Netz.icons.wlan,
    async render(wurzel) {
      const [d, funk] = await Promise.all([Netz.invoke('dieses_netz'), Netz.invoke('wlan_lesen').catch(() => null)]);
      const i = d.info;
      const s = i.subnet;
      const vpn = i.interface.startsWith('utun');
      const magic = i.system_dns.includes('100.100.100.100');

      wurzel.append(kopf(d, funk));
      if (d.hinweis) wurzel.append(meldung(d.hinweis, 'orange'));
      if (!d.netz && !d.erkennung_fehler) {
        wurzel.append(el('div', { class: 'neues-netz' },
          Netz.leerZustand('', 'Merk dir das Netz, dann erkennt Netzinfo es beim nächsten Mal wieder.', 'erster-start'),
          el('div', { class: 'aktion' }, offenKnopf())));
      }
      if (d.erkennung_fehler) wurzel.append(el('p', { class: 'fuss', text: d.erkennung_fehler }));

      const adresse = gruppe('Adresse', [
        zeile('Schnittstelle', i.interface, E.schnittstelle(vpn)),
        zeile('IP-Adresse', i.ip, E.ip(i.dhcp_server != null)),
        ...(s ? [zeile('Maske', `${s.mask} (/${s.prefix})`, E.maske)] : []),
      ]);
      if (s) {
        adresse.append(mehr('Netzadresse, Broadcast, Hostbereich', [
          zeile('Netzadresse', s.network, E.netzadresse),
          zeile('Broadcast', s.broadcast, E.broadcast),
          zeile('Hostbereich', `${s.first} bis ${s.last} (${s.hosts} ${s.hosts === 1 ? 'Host' : 'Hosts'})`, E.hostbereich),
        ]));
      }
      wurzel.append(adresse);

      const dhcp = i.dhcp_server != null;
      const dienste = gruppe('Router und Dienste', [
        zeile('Router', i.router, E.router),
        ...(dhcp ? [
          zeile('DHCP-Server', i.dhcp_server, E.dhcp),
          stapel('DNS vom DHCP', i.dhcp_dns, E.dhcpDns),
        ] : []),
        stapel('DNS des Systems', i.system_dns, E.systemDns(magic)),
        ...(dhcp ? [zeile('Domain', i.domain || 'keine', E.domain)] : []),
      ], dhcp ? null : 'Es gibt keine DHCP-Antwort, etwa bei einem VPN-Tunnel. Deshalb fehlen Maske, Netzadresse, Broadcast, Hostbereich, DHCP-Server, DNS vom DHCP und Domain.');
      wurzel.append(dienste);
      proxyZeile(dienste);
      const v6 = ipv6Zeile(dienste);

      // Verschlüsseltes DNS (#33, #88): Bei einem gemerkten Netz zeigt die Karte zuerst die gespeicherte
      // Messung und misst nur auf Knopfdruck oder wenn sie älter als ein Tag ist. Sonst misst sie beim Öffnen.
      // Die letzte Zeile nennt den Stand und trägt „Neu messen“.
      const w = { Erreichbar: 'erreichbar', Blockiert: 'gesperrt', Abgefangen: 'abgefangen' };
      const bew = { Erreichbar: 'gut', Abgefangen: 'schlecht' };
      const dnsZeilen = (m) => [
        bewertet(zeile('DNS über TLS (Port 853)', w[m.dot], E.dot[m.dot]), bew[m.dot]),
        bewertet(zeile('DNS über HTTPS (Port 443)', w[m.doh], E.doh), bew[m.doh]),
        stapel('Resolver vom Netz (DDR)', m.ddr || ['keiner angeboten'], E.ddr),
      ];
      const stand = (satz, knopf) => el('div', { class: 'zeile stand' }, el('dt', { class: 'label', text: satz }), knopf && el('dd', { class: 'wert' }, knopf));
      const neuKnopf = (vorher) => el('button', { class: 'knopf', type: 'button', text: 'Neu messen', 'aria-label': 'Verschlüsseltes DNS neu messen', onclick: () => messen(vorher) });
      const gespeichert = d.netz ? await Netz.invoke('dns_gespeichert', { name: d.netz.name }).catch(() => null) : null;
      let dnsOrt = gruppe('Verschlüsseltes DNS', [zeile('DNS über TLS und HTTPS', 'wird geprüft …', E.verschl)]);
      // Das Ergebnis meldet VoiceOver einmal (höflich), nicht bei jedem Neuzeichnen der Gruppe.
      const ansage = el('p', { class: 'nur-vorlesen', 'aria-live': 'polite' });
      const zeigeDns = (zeilen, sage) => {
        const neu = gruppe('Verschlüsseltes DNS', zeilen);
        neu.classList.add('frisch');
        const fokusDrin = dnsOrt.contains(document.activeElement);
        dnsOrt.replaceWith(neu);
        dnsOrt = neu;
        if (sage) ansage.textContent = sage;
        if (fokusDrin) { const h = neu.querySelector('h3'); h.tabIndex = -1; (neu.querySelector('button') || h).focus(); }
      };
      const messen = (vorher) => {
        if (vorher) zeigeDns([...dnsZeilen(vorher.messung), stand(`Messung vom ${zeit(vorher.messung.zeit)}, wird neu gemessen …`)]);
        Netz.invoke('dns_verschluesselt').then((r) => zeigeDns([...dnsZeilen(r.messung), stand(
          `Gemessen am ${zeit(r.messung.zeit)}${r.netz ? '' : '. Dieses Netz ist nicht gemerkt, das Ergebnis wird nicht gespeichert'}.`,
          neuKnopf({ messung: r.messung }))],
        `Verschlüsseltes DNS gemessen: DNS über TLS ${w[r.messung.dot]}, DNS über HTTPS ${w[r.messung.doh]}.`))
          .catch((err) => zeigeDns([...(vorher ? dnsZeilen(vorher.messung) : []), stand(`Messung fehlgeschlagen. ${Netz.text(err)}`, neuKnopf(vorher))],
            'Messung des verschlüsselten DNS fehlgeschlagen.'));
      };
      wurzel.append(dnsOrt, ansage);
      if (gespeichert?.frisch) {
        zeigeDns([...dnsZeilen(gespeichert.messung), stand(`Gespeichert, gemessen am ${zeit(gespeichert.messung.zeit)}.`, neuKnopf(gespeichert))]);
      } else {
        messen(gespeichert);
      }
      // WLAN-Karte: ohne Funkverbindung (Kabel) oder bei einem Fehler bleibt sie weg.
      if (funk) {
        const ort = ortKarte(await Netz.invoke('ort_status').catch(() => 'Erlaubt'));
        if (ort) wurzel.append(ort);
        wurzel.append(wlanKarte(funk));
      }
      await v6;
    },
  });
})();
