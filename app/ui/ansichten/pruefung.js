// „Prüfung“: was in diesem Netz anders ist als beim letzten Besuch (wie `netzinfo pruefen`).
// Die Erklärsätze sind die `explain`-Zeilen aus src/pruefen.rs.
(() => {
  const { el, zeit, status, meldung, gruppe } = Netz;

  const E = {
    router: 'Der Router (Gateway) ist die Tür aus dem eigenen Subnetz. Alles, was nicht im Subnetz liegt, schickt der Mac an ihn.',
    dhcp: 'Der DHCP-Server vergibt IP, Maske, Router und DNS. Im Heimnetz ist er fast immer der Router. Ist er es plötzlich nicht mehr, verteilt vielleicht ein fremdes Gerät Adressen.',
    dns: 'DNS übersetzt Namen in IP-Adressen. Diese Server hat das Netz per DHCP angeboten.',
    sperre: 'Ich frage github.com (A) beim DNS-Server deines Netzes und bei 1.1.1.1. Sagt nur dein Netz „gibt es nicht“ oder 0.0.0.0, blockiert es den Namen (Werbefilter, Jugendschutz oder Zensur).',
    umleitung: 'Dann frage ich nach einem Namen, den es sicher nicht gibt (eine Zufallskette unter example.com). Ehrlich ist die Antwort „NXDOMAIN“. Erfindet dein Netz trotzdem eine Adresse, während 1.1.1.1 NXDOMAIN sagt, leitet es falsche Namen um. Das tun Captive Portals und manche Provider, es kann aber auch DNS-Hijacking sein.',
    tls: 'Bei github.com und apple.com baue ich eine TLS-Verbindung auf und prüfe, ob das Zertifikat von einer öffentlich bekannten Stelle stammt. Sonst liest vielleicht jemand mit (SSL-Inspection, wie in Firmennetzen oder bei einem Angreifer). Zusätzlich merke ich mir den Fingerabdruck der ausstellenden Stelle.',
    ip: 'Meine öffentliche IP erfahre ich von 1.1.1.1 (TXT „whoami.cloudflare“, Klasse CH). Sie gehört dem Provider und wechselt oft, das ist normal.',
    erst: 'Beim nächsten Besuch vergleiche ich mit diesen Werten und sage dir, was anders ist.',
  };
  const KLASSE = {
    Alarm: ['rot', 'Alarm', 'Alarm: nutze das Netz nicht für Passwörter oder Bankgeschäfte, bis du es geklärt hast.'],
    Auffaellig: ['orange', 'Auffällig', 'Auffällig: schau nach, ob das so gewollt ist.'],
    Harmlos: ['gruen', 'Harmlos', 'Harmlos heißt: kommt im Alltag vor.'],
  };

  const ja = (v) => (v == null ? null : v ? 'ja' : 'nein');
  const tlsWort = (x, host) => ({ Normal: 'normal', Aufgebrochen: 'aufgebrochen' })[x.tls.find((t) => t.host === host)?.verdict] ?? null;

  // The answer outlives the view: `pruefen` stores the new baseline, so showing the tab
  // again must not measure again.
  const z = { laeuft: false, erg: null, fehler: null };
  let ui = null;

  // One row: current value right, the old one below it when it differs.
  const reihe = (label, m, o, wert, erkl) => {
    const neu = wert(m);
    const alt = o ? wert(o) : null;
    return el('div', { class: 'zeile' },
      el('dt', { class: 'label', text: label }),
      el('dd', { class: `wert${neu == null ? ' leer' : ''}` },
        neu ?? 'nicht messbar',
        alt != null && alt !== neu && el('span', { class: 'alt', text: `vorher ${alt}` })),
      erkl && el('dd', { class: 'erklaerung', text: erkl }));
  };

  function urteil(erg) {
    const worst = erg.unterschiede[0]?.klasse;
    const seit = `Seit dem letzten Besuch am ${zeit(erg.vorher.time)}`;
    if (worst === 'Alarm') return status('rot', `Alarm. ${seit} hat sich etwas geändert, das du klären solltest.`);
    if (worst === 'Auffaellig') return status('orange', `Auffällig. ${seit} ist etwas anders, schau nach, ob das so gewollt ist.`);
    return status('gruen', `Nichts Auffälliges. ${seit} ist alles im Rahmen.`);
  }

  function unterschiede(erg) {
    return gruppe('Unterschiede', erg.unterschiede.map((u) => {
      const [farbe, wort, erkl] = KLASSE[u.klasse];
      return el('div', { class: 'zeile' },
        el('dt', { class: 'label klasse' }, el('span', { class: `punkt ${farbe}` }), wort),
        el('dd', { class: 'wert text', text: u.text }),
        el('dd', { class: 'erklaerung', text: erkl }));
    }));
  }

  function werte(erg) {
    const m = erg.messung;
    const o = erg.vorher;
    const r = (label, wert, erkl) => reihe(label, m, o, wert, erkl);
    const n = erg.nicht_messbar;
    return [
      gruppe('Router und DHCP', [r('Router', (x) => x.router, E.router), r('DHCP-Server', (x) => x.dhcp_server, E.dhcp)]),
      gruppe('DNS', [r('DNS-Server vom Netz', (x) => (x.dhcp_dns.length ? x.dhcp_dns.join(', ') : 'keine'), E.dns)]),
      gruppe('Sperren und Umleitungen', [
        r('github.com blockiert, im Netz', (x) => ja(x.blocked_local), E.sperre),
        r('github.com blockiert, bei 1.1.1.1', (x) => ja(x.blocked_public)),
        r('Erfundene Antworten für falsche Namen', (x) => ja(x.hijack), E.umleitung),
      ]),
      gruppe('Zertifikate', m.tls.map((t, i) => r(t.host, (x) => tlsWort(x, t.host), i === 0 ? E.tls : null))),
      gruppe('Öffentliche IP', [r('Öffentliche IP', (x) => x.public_ip, E.ip)],
        n > 0 && (n === 1
          ? 'Eine Prüfung ließ sich nicht messen (Internet? Zeitüberschreitung?). Sie zählt nicht als Änderung.'
          : `${n} Prüfungen ließen sich nicht messen (Internet? Zeitüberschreitung?). Sie zählen nicht als Änderung.`)),
    ];
  }

  function zeichne() {
    if (!ui) return;
    const { kopf, rest, knopf } = ui;
    const erg = z.erg;
    const unbekannt = z.fehler?.startsWith('Dieses Netz ist nicht gemerkt');
    knopf.disabled = z.laeuft;
    knopf.replaceChildren(z.laeuft ? 'Prüfe …' : 'Jetzt prüfen', ...(z.laeuft ? [] : [el('span', { class: 'kbd', 'aria-hidden': 'true', text: '⌘R' })]));
    const fuell = (ort, ...k) => ort.replaceChildren(...k.flat().filter((x) => x != null && x !== false));
    fuell(kopf,
      el('h2', { text: erg ? erg.netz : unbekannt ? 'Unbekanntes Netz' : 'Prüfung' }),
      z.laeuft ? status('grau', 'Prüfe …')
        : erg ? (erg.erster_besuch ? status('grau', 'Erste Messung, ab jetzt die Vergleichsbasis') : urteil(erg))
        : unbekannt ? status('orange', 'Noch nicht gemerkt') : null);
    fuell(rest,
      ...(z.fehler && !z.laeuft ? [
        meldung(z.fehler, unbekannt ? 'orange' : 'rot'),
        unbekannt && el('div', { class: 'aktion' },
          el('button', { class: 'knopf', type: 'button', text: 'Zu „Dieses Netz“ …', onclick: () => document.getElementById('tab-dieses-netz').click() })),
      ] : []),
      ...(erg ? [
        erg.erster_besuch && el('p', { class: 'fuss erklaerung', text: E.erst }),
        !erg.erster_besuch && erg.unterschiede.length > 0 && unterschiede(erg),
        ...werte(erg),
      ] : []));
  }

  async function lauf() {
    if (z.laeuft) return;
    z.laeuft = true;
    z.fehler = null;
    zeichne();
    try {
      z.erg = await Netz.invoke('pruefen');
    } catch (e) {
      z.erg = null;
      z.fehler = Netz.text(e);
    }
    z.laeuft = false;
    zeichne();
  }

  // ⌘R means „Jetzt prüfen“ here; runs before the shared handler in app.js.
  document.addEventListener('keydown', (e) => {
    if (e.metaKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'r'
      && document.getElementById('tab-pruefung')?.getAttribute('aria-selected') === 'true') {
      e.preventDefault();
      e.stopImmediatePropagation();
      lauf();
    }
  }, true);

  Netz.registriere({
    id: 'pruefung',
    titel: 'Prüfung',
    icon: Netz.icons.pruefung,
    render(wurzel) {
      // Old view still on screen means the toolbar button asked again; coming from another tab shows the old answer.
      const nochmal = ui?.wurzel.isConnected;
      const knopf = el('button', { class: 'knopf haupt', type: 'button', onclick: lauf });
      const kopf = el('div', { class: 'kopf', 'aria-live': 'polite' });
      const rest = el('div');
      ui = { wurzel, kopf, rest, knopf };
      wurzel.append(kopf, el('div', { class: 'aktion' }, knopf), rest);
      zeichne();
      if (nochmal || (!z.erg && !z.fehler)) lauf();
    },
  });
})();
