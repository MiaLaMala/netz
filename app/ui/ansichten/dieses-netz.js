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

  function offenKnopf() {
    return el('button', { class: 'knopf haupt', type: 'button', text: 'Dieses Netz merken …', onclick: oeffne });
  }
  function oeffne(e) {
    const ort = e.currentTarget.parentElement;
    ort.replaceChildren(merkenFormular(ort, Netz.neuLaden));
    ort.querySelector('input[type="text"]').focus();
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
    },
  });
})();
