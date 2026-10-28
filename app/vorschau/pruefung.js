// Fake answers for the view „pruefung“ in vorschau.html.
// Marks: pruefung (auffällig), pruefung-ok, pruefung-nm, pruefung-alarm, pruefung-erst,
// pruefung-unbekannt, pruefung-fehler, pruefung-laeuft.
(() => {
  const vorher = {
    time: 1793050200 - 86400 * 3,
    router: '192.168.178.1', dhcp_server: '192.168.178.1', dhcp_dns: ['192.168.178.1'],
    blocked_local: false, blocked_public: false, hijack: false,
    tls: [
      { host: 'github.com', verdict: 'Normal', issuer: 'aa01' },
      { host: 'apple.com', verdict: 'Normal', issuer: 'bb02' },
    ],
    public_ip: '192.0.2.44',
  };
  const jetzt = { ...vorher, time: 1793050200 };
  const basis = { netz: 'WG', messung: jetzt, vorher, unterschiede: [], erster_besuch: false, nicht_messbar: 0 };

  const fall = {
    ok: basis,
    nm: {
      ...basis, nicht_messbar: 2,
      messung: { ...jetzt, public_ip: null, tls: [jetzt.tls[0], { host: 'apple.com', verdict: null, issuer: null }] },
    },
    erst: { ...basis, vorher: null, erster_besuch: true },
    alarm: {
      ...basis,
      messung: {
        ...jetzt, dhcp_server: '192.168.178.77', hijack: true, dhcp_dns: ['192.0.2.53'],
        tls: [{ host: 'github.com', verdict: 'Aufgebrochen', issuer: 'cc03' }, jetzt.tls[1]],
      },
      unterschiede: [
        { klasse: 'Alarm', text: 'Der DHCP-Server (192.168.178.77) ist nicht mehr der Router (192.168.178.1). Vorher waren beide dasselbe Gerät: Vielleicht verteilt jetzt ein fremdes Gerät Adressen.' },
        { klasse: 'Alarm', text: 'Das Zertifikat von github.com stammt jetzt von einer unbekannten Stelle, vorher war es normal. Jemand im Netz liest vielleicht verschlüsselten Verkehr mit.' },
        { klasse: 'Auffaellig', text: 'Das Netz nennt andere DNS-Server als beim letzten Besuch: jetzt 192.0.2.53, vorher 192.168.178.1.' },
      ],
    },
    auffaellig: {
      ...basis,
      messung: { ...jetzt, blocked_local: true, public_ip: '192.0.2.99', tls: [{ ...jetzt.tls[0], issuer: 'dd04' }, jetzt.tls[1]] },
      unterschiede: [
        { klasse: 'Auffaellig', text: 'github.com wird von deinem Netz jetzt blockiert, vorher nicht.' },
        { klasse: 'Harmlos', text: 'Die Stelle, die das Zertifikat von github.com ausstellt, hat gewechselt, das Urteil bleibt normal. Zertifikate werden regelmäßig erneuert.' },
        { klasse: 'Harmlos', text: 'Neue öffentliche IP: 192.0.2.99, vorher 192.0.2.44. Provider vergeben sie immer wieder neu.' },
      ],
    },
  };

  VORSCHAU.pruefen = async () => {
    if (marken.includes('pruefung-laeuft')) return new Promise(() => {});
    if (marken.includes('pruefung-unbekannt')) throw 'Dieses Netz ist nicht gemerkt, ich habe nichts zum Vergleichen. Speichere es mit `netz merken <Name>` und prüfe dann noch mal.';
    if (marken.includes('pruefung-fehler')) throw 'Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.';
    const key = ['ok', 'nm', 'erst', 'alarm'].find((k) => marken.includes(`pruefung-${k}`));
    return fall[key ?? 'auffaellig'];
  };
})();
