// „Dieses Netz“: was der Mac gerade vom Netz weiß, und ob er es kennt.
// Die Erklärsätze folgen den Zeilen von `netzinfo hier --erklaer` (src/hier.rs).
(() => {
  const { el, zeit, gruppe, zeile, status, meldung } = Netz;

  const E = {
    schnittstelle: (vpn) =>
      'Die Default-Route sagt, über welche Schnittstelle Pakete ins Internet gehen. ' +
      (vpn
        ? '„utun“ ist ein Tunnel, also ein VPN wie Tailscale oder WireGuard: Dein Verkehr geht gerade durch den Tunnel, nicht direkt ins WLAN.'
        : '„en0“ ist beim Mac meistens das WLAN, andere „en“-Nummern sind Kabel oder Adapter.'),
    ip: (dhcp) =>
      dhcp
        ? 'Die IPv4-Adresse hat dir der DHCP-Server geliehen. Im Heimnetz ist sie privat (10.x, 172.16 bis 172.31, 192.168.x) und wird per NAT ins Internet übersetzt.'
        : 'Diese Adresse hat kein DHCP-Server vergeben, sondern die Gegenstelle des Tunnels, z. B. Tailscale oder WireGuard.',
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

  const liste = (a) => (a && a.length ? a.join(', ') : null);

  function kopf(d) {
    const { netz } = d;
    if (netz) {
      return el('div', { class: 'kopf' },
        el('h2', { text: netz.name }),
        status('gruen', `Bekanntes Netz · ${netz.meins ? 'meins · ' : ''}zuletzt hier ${zeit(netz.last_seen)}`));
    }
    return el('div', { class: 'kopf' },
      el('h2', { text: 'Unbekanntes Netz' }),
      d.erkennung_fehler
        ? status('grau', 'Ohne DHCP, nicht wiedererkennbar')
        : status('orange', 'Noch nicht gemerkt'));
  }

  // Inline-Formular statt Dialog: Name, „meins“, Merken.
  function merkenFormular(ort, nachher) {
    const name = el('input', { type: 'text', id: 'netz-name', placeholder: 'z. B. WG', autocomplete: 'off', spellcheck: 'false' });
    const meins = el('input', { type: 'checkbox', id: 'netz-meins' });
    const fehler = el('div', { 'aria-live': 'polite' });
    const merken = el('button', { class: 'knopf haupt', type: 'submit', text: 'Merken' });
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
      merken.disabled = true;
      try {
        await Netz.invoke('netz_merken', { name: name.value, meins: meins.checked });
        nachher();
      } catch (err) {
        fehler.replaceChildren(meldung(Netz.text(err)));
        merken.disabled = false;
        name.focus();
      }
    });
    return form;
  }

  // Karte „WLAN“ (#34). Die Sätze folgen `netzinfo wlan --erklaer` (src/wlan.rs).
  const SICHER = {
    Offen: 'offen (unverschlüsselt)', Wep: 'WEP', Wpa: 'WPA', Wpa2: 'WPA2', Wpa3: 'WPA3',
    Wpa2Wpa3Gemischt: 'WPA2/WPA3 gemischt', Unbekannt: 'unbekannt',
  };
  const SICHER_SATZ = {
    Offen: 'Das WLAN ist offen: Jeder in Reichweite kann mitlesen. Nutze nur verschlüsselte Verbindungen oder ein VPN.',
    Wep: 'WEP gilt seit Jahren als geknackt, es schützt kaum. Stelle den Router auf WPA2 oder WPA3 um.',
    Wpa: 'WPA (die erste Version) ist veraltet. Stelle den Router auf WPA2 oder WPA3 um.',
  };
  const wlanKarte = (w) => {
    const sicher = w.sicherheit ? SICHER[w.sicherheit] : null;
    const staerke = w.rssi_dbm == null ? '' : w.rssi_dbm >= -70 ? ' gut' : w.rssi_dbm >= -80 ? ' mittel' : ' schwach';
    const z = zeile('Signal', w.rssi_dbm == null ? null : `${w.rssi_dbm} dBm`,
      w.rssi_dbm == null ? null : `Das Signal liegt bei minus ${-w.rssi_dbm} dBm. Je näher an null, desto besser, ab minus 70 wird es merklich schlechter.`);
    z.querySelector('.wert').className += ` wlan-signal${staerke}`;
    const warn = ['Offen', 'Wep', 'Wpa'].includes(w.sicherheit) ? ` wlan-warn` : '';
    const sz = zeile('Sicherheit', sicher, SICHER_SATZ[w.sicherheit] || 'Die Verschlüsselung ist zeitgemäß.');
    sz.querySelector('.wert').className += warn;
    return gruppe('WLAN', [
      ...(w.ssid ? [
        zeile('Name (SSID)', w.ssid, 'Der Name, unter dem das WLAN sich meldet. Zwei verschiedene Netze können denselben Namen tragen, der Name allein beweist also nichts.'),
        zeile('Kennung (BSSID)', w.bssid, 'Die Hardware-Adresse des Funkgeräts, mit dem du verbunden bist. Gespeichert wird davon nur ein Hash, nie die Adresse selbst.'),
      ] : []),
      zeile('Standard', w.standard, 'Der Standard (Wi-Fi 4, 5, 6) bestimmt, wie schnell die Funkstrecke höchstens sein kann. Neuere Standards sind schneller und gehen besser mit vielen Geräten um.'),
      zeile('Band', w.band, '2,4 GHz reicht weiter durch Wände, 5 und 6 GHz sind schneller, brauchen aber Nähe zum Router.'),
      zeile('Kanal', w.kanal, 'Auf dem Kanal funkt dein WLAN. Teilen sich viele Nachbarnetze denselben Kanal, stören sie sich gegenseitig.'),
      zeile('Kanalbreite', w.breite_mhz == null ? null : `${w.breite_mhz} MHz`, 'Breitere Kanäle sind schneller, aber störanfälliger und belegen mehr vom Band.'),
      z,
      zeile('Rauschen', w.rauschen_dbm == null ? null : `${w.rauschen_dbm} dBm`, 'Rauschen ist der Störpegel auf dem Kanal. Je weiter von null weg, desto besser. Manche Treiber melden es nicht, dann bleibt das Feld leer.'),
      zeile('Abstand zum Rauschen', w.snr_db == null ? null : `${w.snr_db} dB`,
        w.snr_db == null ? null : `Der Abstand zum Rauschen beträgt ${w.snr_db} dB. Über 25 dB gilt als gut für Videoanrufe.`),
      zeile('Senderate', w.rate_mbit == null ? null : `${Math.round(w.rate_mbit)} MBit/s`, 'Die Senderate ist das, was Mac und Router gerade aushandeln, nicht die echte Geschwindigkeit. Echt kommt meist die Hälfte oder weniger an.'),
      sz,
      zeile('Land', w.land, 'Das Land bestimmt, welche Kanäle und welche Sendeleistung erlaubt sind.'),
    ]);
  };

  // Standortfreigabe (#35): macOS gibt Netzname und Kennung nur mit Standortfreigabe heraus.
  const ORT_WARUM = 'macOS behandelt den Namen eines WLANs als Ortsangabe, denn aus ihm lässt sich ableiten, wo du bist. Deshalb zeigt es Name und Kennung nur Apps mit Standortfreigabe. Netzinfo liest damit nur das WLAN, mit dem du verbunden bist, und merkt sich keinen Standort. Ohne Freigabe erkennt Netzinfo Netze etwas ungenauer, alles andere geht weiter.';
  function ortKarte(status) {
    if (status === 'Erlaubt') return null;
    const aktion = (text, fn) => el('button', { class: 'knopf haupt', type: 'button', text, onclick: fn });
    const fehler = el('div', { 'aria-live': 'polite' });
    const nachrichten = {
      NichtGefragt: 'Netzinfo hat noch nicht nach dem Standort gefragt, deshalb zeigt macOS Name und Kennung des WLANs nicht.',
      Abgelehnt: 'Der Standort ist abgelehnt. Netzinfo funktioniert weiter, erkennt Netze aber ungenauer. Ändern geht in den Systemeinstellungen unter Datenschutz & Sicherheit > Ortungsdienste.',
      Eingeschraenkt: 'macOS beschränkt den Standort auf diesem Mac (z. B. durch Bildschirmzeit oder ein Profil). Netzinfo kann ihn nicht selbst freigeben.',
    };
    const knopf = status === 'NichtGefragt'
      ? aktion('Standort freigeben …', async (e) => {
        e.currentTarget.disabled = true;
        try {
          await Netz.invoke('ort_anfragen');
          // Die Antwort kommt erst nach dem Klick im Systemdialog: kurz warten und neu lesen.
          for (let i = 0; i < 60 && (await Netz.invoke('ort_status')) === 'NichtGefragt'; i++) await new Promise((r) => setTimeout(r, 1000));
          Netz.neuLaden();
        } catch (err) {
          fehler.replaceChildren(meldung(Netz.text(err)));
          e.currentTarget.disabled = false;
        }
      })
      : status === 'Abgelehnt'
        ? aktion('Systemeinstellungen öffnen', () => Netz.invoke('ort_einstellungen_oeffnen').catch((err) => fehler.replaceChildren(meldung(Netz.text(err)))))
        : null;
    return el('section', { class: 'gruppe ort-karte', 'aria-labelledby': 'g-ort' },
      el('h3', { id: 'g-ort', text: 'Standortfreigabe' }),
      el('div', { class: 'liste' },
        el('div', { class: 'zeile' }, el('p', { class: 'ort-status', text: nachrichten[status] || nachrichten.NichtGefragt })),
        el('details', { class: 'zeile' }, el('summary', { text: 'Warum fragt Netzinfo nach dem Standort?' }), el('p', { class: 'hilfe', text: ORT_WARUM }))),
      fehler,
      knopf && el('div', { class: 'knoepfe' }, knopf));
  }

  function offenKnopf() {
    return el('button', { class: 'knopf haupt', type: 'button', text: 'Dieses Netz merken …', onclick: oeffne });
  }
  function oeffne(e) {
    const ort = e.currentTarget.parentElement;
    ort.replaceChildren(merkenFormular(ort, Netz.neuLaden));
    ort.querySelector('input[type="text"]').focus();
  }

  // Proxy und WPAD (#53): rein lesend. Die PAC-Datei wird nur auf Klick geladen, gezeigt werden nur Hostnamen.
  function proxyGruppe() {
    const erkl = 'Ein Proxy ist ein Zwischenrechner, über den dein Mac Webseiten laden soll. Der Betreiber könnte deine Webanfragen sehen. In einem Heimnetz gibt es normalerweise keinen, ein neuer Eintrag ist ein Grund zum Nachsehen.';
    let ort = gruppe('Proxy', [zeile('Proxy und WPAD', 'wird geprüft …', erkl)]);
    const ersetze = (neu) => { ort.replaceWith(neu); ort = neu; };
    Netz.invoke('proxy_lesen').then((b) => {
      const zeilen = [
        zeile('Fester Proxy', b.manuell || 'keiner', erkl),
        zeile('PAC-Datei', b.pac_url || 'keine', 'Die PAC-Datei sagt dem Browser, welcher Proxy für welche Adresse gilt. Ich führe sie nie aus.'),
        zeile('WPAD per DHCP', b.wpad_dhcp || 'keiner', 'Das Netz kann dem Mac mit DHCP-Option 252 die Adresse einer PAC-Datei mitgeben (WPAD).'),
        zeile('WPAD per DNS', b.wpad_dns ? 'gefunden' : 'nicht gefunden', 'Ich frage den DNS-Server des Netzes nach wpad.<Domain>. Ein Treffer ist nur ein Hinweis, die Datei lade ich nicht.'),
      ];
      const keiner = !b.manuell && !b.pac_url && !b.wpad_dhcp && !b.wpad_dns;
      const neu = gruppe('Proxy', zeilen, keiner ? 'Kein Proxy.' : null);
      if (b.pac_url) {
        const hosts = el('div', { 'aria-live': 'polite' });
        neu.append(el('div', { class: 'aktion' }, el('button', { class: 'knopf', type: 'button', 'data-pac': '', text: 'PAC-Datei ansehen', onclick: (e) => {
          e.currentTarget.disabled = true;
          Netz.invoke('proxy_pac_ansehen').then((h) => hosts.replaceChildren(el('p', { class: 'fuss', text: h.length ? `Hostnamen in der PAC-Datei: ${h.join(', ')}` : 'In der PAC-Datei stehen keine Proxy-Hostnamen.' })))
            .catch((err) => hosts.replaceChildren(meldung(Netz.text(err), 'orange')));
        } })), hosts);
      }
      ersetze(neu);
    }).catch(() => ort.remove());
    return ort;
  }

  Netz.registriere({
    id: 'dieses-netz',
    titel: 'Dieses Netz',
    icon: Netz.icons.wlan,
    async render(wurzel) {
      const d = await Netz.invoke('dieses_netz');
      const i = d.info;
      const s = i.subnet;
      const vpn = i.interface.startsWith('utun');
      const magic = i.system_dns.includes('100.100.100.100');

      wurzel.append(kopf(d));
      if (d.hinweis) wurzel.append(meldung(d.hinweis, 'orange'));
      if (!d.netz && !d.erkennung_fehler) wurzel.append(el('div', { class: 'aktion' }, offenKnopf()));
      if (d.erkennung_fehler) wurzel.append(el('p', { class: 'fuss', text: d.erkennung_fehler }));

      wurzel.append(gruppe('Adresse', [
        zeile('Schnittstelle', i.interface, E.schnittstelle(vpn)),
        zeile('IP-Adresse', i.ip, E.ip(i.dhcp_server != null)),
        ...(s ? [
          zeile('Maske', `${s.mask} (/${s.prefix})`, E.maske),
          zeile('Netzadresse', s.network, E.netzadresse),
          zeile('Broadcast', s.broadcast, E.broadcast),
          zeile('Hostbereich', `${s.first} bis ${s.last}`, E.hostbereich),
        ] : []),
      ], s ? null : 'Ohne DHCP-Antwort, z. B. bei einem VPN-Tunnel, kennt der Mac die Maske nicht. Deshalb fehlen Maske, Netzadresse, Broadcast und Hostbereich.'));

      const dhcp = i.dhcp_server != null;
      wurzel.append(gruppe('Router und Dienste', [
        zeile('Router', i.router, E.router),
        ...(dhcp ? [
          zeile('DHCP-Server', i.dhcp_server, E.dhcp),
          zeile('DNS vom DHCP', liste(i.dhcp_dns), E.dhcpDns),
        ] : []),
        zeile('DNS des Systems', liste(i.system_dns), E.systemDns(magic)),
        ...(dhcp ? [zeile('Domain', i.domain || 'keine', E.domain)] : []),
      ], dhcp ? null : 'In diesem Netz hat kein DHCP-Server geantwortet. Deshalb fehlen DHCP-Server, DNS vom DHCP und Domain.'));

      // Verschlüsseltes DNS (#33, #88): Bei einem gemerkten Netz zeigt die Karte zuerst die gespeicherte
      // Messung und misst nur auf Knopfdruck oder wenn sie älter als ein Tag ist. Sonst misst sie beim Öffnen.
      const w = { Erreichbar: 'erreichbar', Blockiert: 'gesperrt', Abgefangen: 'abgefangen' };
      const dnsGruppe = (zeilen, fuss) => gruppe('Verschlüsseltes DNS', zeilen, fuss);
      const dnsZeilen = (m) => [
        zeile('DNS über TLS (Port 853)', w[m.dot], E.dot[m.dot]),
        zeile('DNS über HTTPS (Port 443)', w[m.doh], E.doh),
        zeile('Resolver vom Netz (DDR)', m.ddr ? m.ddr.join('; ') : 'keiner angeboten', E.ddr),
      ];
      const gespeichert = d.netz ? await Netz.invoke('dns_gespeichert', { name: d.netz.name }).catch(() => null) : null;
      let dnsOrt = dnsGruppe([zeile('DNS über TLS und HTTPS', 'wird geprüft …', E.verschl)]);
      const messen = () => {
        if (gespeichert) {
          const alt = dnsGruppe(dnsZeilen(gespeichert.messung), `Messung vom ${zeit(gespeichert.messung.zeit)}, wird neu gemessen …`);
          dnsOrt.replaceWith(alt);
          dnsOrt = alt;
        }
        Netz.invoke('dns_verschluesselt').then((r) => {
          const neu = dnsGruppe(dnsZeilen(r.messung), r.netz ? `Gemessen am ${zeit(r.messung.zeit)}.` : 'Dieses Netz ist nicht gemerkt, das Ergebnis wird nicht gespeichert.');
          dnsOrt.replaceWith(neu);
          dnsOrt = neu;
        }).catch((err) => dnsOrt.replaceWith(gruppe('Verschlüsseltes DNS', [], Netz.text(err))));
      };
      if (gespeichert?.frisch) {
        dnsOrt = dnsGruppe(dnsZeilen(gespeichert.messung), `Gespeichert, gemessen am ${zeit(gespeichert.messung.zeit)}.`);
        dnsOrt.append(el('div', { class: 'dns-neu' }, el('button', { class: 'knopf', type: 'button', text: 'Neu messen', onclick: (e) => { e.currentTarget.disabled = true; messen(); } })));
        wurzel.append(dnsOrt);
      } else {
        wurzel.append(dnsOrt);
        messen();
      }
      wurzel.append(proxyGruppe());
      // WLAN-Karte: ohne Funkverbindung (Kabel) oder bei einem Fehler bleibt sie weg.
      const funk = await Netz.invoke('wlan_lesen').catch(() => null);
      if (funk) {
        const ort = ortKarte(await Netz.invoke('ort_status').catch(() => 'Erlaubt'));
        if (ort) wurzel.append(ort);
        wurzel.append(wlanKarte(funk));
      }
    },
  });
})();
