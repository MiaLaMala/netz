// Fake answers for the view „pruefung“ in vorschau.html.
// Marks: pruefung (auffällig), pruefung-ok, pruefung-nm, pruefung-alarm, pruefung-erst,
// pruefung-unbekannt, pruefung-fehler, pruefung-laeuft, portal (Hinweis „Anmeldung nötig“),
// sni-filter, sni-fremd, sni-unklar (Karte „Webfilter am Servernamen“, sonst: kein Name gestört).
(() => {
  const vorher = {
    time: 1793050200 - 86400 * 3,
    router: '192.0.2.1', dhcp_server: '192.0.2.1', dhcp_dns: ['192.0.2.1'],
    blocked_local: false, blocked_public: false, hijack: false,
    tls: [
      { host: 'github.com', verdict: 'Normal', issuer: 'aa01' },
      { host: 'apple.com', verdict: 'Normal', issuer: 'bb02' },
    ],
    public_ip: '203.0.113.44',
  };
  const jetzt = { ...vorher, time: 1793050200 };
  const gleich = (z) => ({ klasse: 'Harmlos', text: `Der Weg zu ${z} führt über dieselben Stationen wie bei deinem letzten Besuch.` });
  const ruhig = [gleich('1.1.1.1'), gleich('9.9.9.9')]; // what `pruefen` sends in the calm state (#66)
  const basis = { netz: 'WG', messung: jetzt, vorher, unterschiede: ruhig, erster_besuch: false, nicht_messbar: 0 };

  const fall = {
    ok: basis,
    nm: {
      ...basis, nicht_messbar: 2,
      unterschiede: [{ klasse: 'Harmlos', text: 'Der Weg zu 1.1.1.1 ist nicht beurteilbar: Es antworteten weniger als 2 öffentliche Stationen.' }],
      messung: { ...jetzt, public_ip: null, tls: [jetzt.tls[0], { host: 'apple.com', verdict: null, issuer: null }] },
    },
    erst: { ...basis, vorher: null, erster_besuch: true, unterschiede: [] },
    alarm: {
      ...basis,
      messung: {
        ...jetzt, dhcp_server: '192.0.2.77', hijack: true, dhcp_dns: ['198.51.100.53'],
        tls: [{ host: 'github.com', verdict: 'Aufgebrochen', issuer: 'cc03' }, jetzt.tls[1]],
      },
      unterschiede: [
        { klasse: 'Alarm', text: 'Zwischen Router und Anbieter steht auf dem Weg zu 1.1.1.1 jetzt ein zusätzliches Gerät. Das kann ein neuer Tunnel oder ein zweiter Router sein, oder jemand leitet deinen Verkehr um.' },
        { klasse: 'Alarm', text: 'Das Netz erfindet Antworten für Namen, die es nicht gibt, vorher tat es das nicht. Das kann ein Captive Portal sein, aber auch DNS-Hijacking.' },
        { klasse: 'Alarm', text: 'Der DHCP-Server (192.0.2.77) ist nicht mehr der Router (192.0.2.1). Vorher waren beide dasselbe Gerät: Vielleicht verteilt jetzt ein fremdes Gerät Adressen.' },
        { klasse: 'Alarm', text: 'Das Zertifikat von github.com stammt jetzt von einer unbekannten Stelle, vorher war es normal. Jemand im Netz liest vielleicht verschlüsselten Verkehr mit.' },
        { klasse: 'Auffaellig', text: 'Das Netz nennt andere DNS-Server als beim letzten Besuch: jetzt 198.51.100.53, vorher 192.0.2.1.' },
        gleich('9.9.9.9'),
      ],
    },
    auffaellig: {
      ...basis,
      messung: { ...jetzt, blocked_local: true, public_ip: '203.0.113.99', tls: [{ ...jetzt.tls[0], issuer: 'dd04' }, jetzt.tls[1]] },
      unterschiede: [
        { klasse: 'Auffaellig', text: 'Der Weg zu 1.1.1.1 hat sich geändert: 9 statt 6 Stationen, erste öffentliche Station jetzt 203.0.113.0/24 statt 198.51.100.0/24. Ein anderer Anschluss oder ein Umweg kann das erklären.' },
        gleich('9.9.9.9'),
        { klasse: 'Auffaellig', text: 'github.com wird von deinem Netz jetzt blockiert, vorher nicht.' },
        { klasse: 'Harmlos', text: 'Die Stelle, die das Zertifikat von github.com ausstellt, hat gewechselt, das Urteil bleibt normal. Zertifikate werden regelmäßig erneuert.' },
        { klasse: 'Harmlos', text: 'Neue öffentliche IP: 203.0.113.99, vorher 203.0.113.44. Provider vergeben sie immer wieder neu.' },
      ],
    },
  };

  VORSCHAU.pruefen = async () => {
    if (marken.includes('pruefung-laeuft')) return new Promise(() => {});
    if (marken.includes('pruefung-unbekannt')) throw { art: 'nicht_gemerkt', text: 'Dieses Netz ist nicht gemerkt, ich habe nichts zum Vergleichen. Speichere es mit `netzinfo merken <Name>` und prüfe dann noch mal.' };
    if (marken.includes('pruefung-fehler')) throw 'Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.';
    const key = ['ok', 'nm', 'erst', 'alarm'].find((k) => marken.includes(`pruefung-${k}`));
    return fall[key ?? 'auffaellig'];
  };

  VORSCHAU.sni_pruefen = async () => {
    const ok = [['bbc.com', 'ok'], ['youtube.com', 'ok'], ['telegram.org', 'ok']];
    if (marken.includes('sni-unklar')) return { kontrolle: 'zeitueberschreitung', namen: [], gestoert: [], unklar: true };
    if (marken.includes('sni-fremd')) return { kontrolle: 'ok', namen: [ok[0], ['youtube.com', 'fremdes_zertifikat'], ok[2]], gestoert: ['youtube.com'], unklar: false };
    if (marken.includes('sni-filter')) return { kontrolle: 'ok', namen: [ok[0], ['youtube.com', 'zurueckgesetzt'], ['telegram.org', 'zurueckgesetzt']], gestoert: ['youtube.com', 'telegram.org'], unklar: false };
    return { kontrolle: 'ok', namen: ok, gestoert: [], unklar: false };
  };

  VORSCHAU.portal_pruefen = async () => ({
    status: marken.includes('portal') ? 'PortalVermutet' : 'Frei',
    ziel_host: marken.includes('portal') ? 'login.gast.example' : null,
    quellen: [], dns_verdaechtig: null,
  });
  VORSCHAU.portal_im_browser_oeffnen = async () => {};
})();
