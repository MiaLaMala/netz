// „Prüfung“: was in diesem Netz anders ist als beim letzten Besuch (wie `netzinfo pruefen`).
// Die Erklärsätze sind die `explain`-Zeilen aus src/pruefen.rs.
(() => {
  const { el, zeit, status, meldung, gruppe } = Netz;

  const E = {
    router: 'Der Router (Gateway) ist die Tür aus dem eigenen Subnetz. Alles, was nicht im Subnetz liegt, schickt dein Rechner an ihn.',
    dhcp: 'Der DHCP-Server vergibt IP, Maske, Router und DNS. Im Heimnetz ist er fast immer der Router. Ist er es plötzlich nicht mehr, verteilt vielleicht ein fremdes Gerät Adressen.',
    dns: 'DNS übersetzt Namen in IP-Adressen. Diese Server hat das Netz per DHCP angeboten.',
    sperre: 'Ich frage github.com (A) beim DNS-Server deines Netzes und bei 1.1.1.1. Sagt nur dein Netz „gibt es nicht“ oder 0.0.0.0, blockiert es den Namen (Werbefilter, Jugendschutz oder Zensur).',
    umleitung: 'Dann frage ich nach einem Namen, den es sicher nicht gibt (eine Zufallskette unter example.com). Ehrlich ist die Antwort „NXDOMAIN“. Erfindet dein Netz trotzdem eine Adresse, während 1.1.1.1 NXDOMAIN sagt, leitet es falsche Namen um. Das tun Captive Portals und manche Provider, es kann aber auch DNS-Hijacking sein.',
    tls: 'Bei github.com und apple.com baue ich eine TLS-Verbindung auf und prüfe, ob das Zertifikat von einer öffentlich bekannten Stelle stammt. Sonst liest vielleicht jemand mit (SSL-Inspection, wie in Firmennetzen oder bei einem Angreifer). Zusätzlich merke ich mir den Fingerabdruck der ausstellenden Stelle.',
    ip: 'Meine öffentliche IP erfahre ich von 1.1.1.1 (TXT „whoami.cloudflare“, Klasse CH). Sie gehört dem Provider und wechselt oft, das ist normal.',
    portal: 'Ein Captive Portal ist die Anmeldeseite in Hotel-, Café- oder Gäste-WLAN. Ich rufe dazu feste Testseiten von Apple, Google und Firefox auf, die immer dieselbe Antwort geben. Kommt etwas anderes zurück, fängt das Netz die Anfrage ab. Die App lädt die Portalseite nie selbst: Der Browser öffnet sie.',
    sni: 'Beim Verbindungsaufbau steht der Name der Seite im Klartext (SNI). Ich verbinde mich mit 1.1.1.1 einmal mit einem neutralen Namen als Kontrolle, dann mit drei Beispielnamen. Bricht das Netz nur bei einem Namen ab, ist das ein Webfilter, kein Fehler deines Geräts. In Schulen und Firmen ist er gewollt.',
    weg: 'Ich messe den Weg zu 1.1.1.1 und 9.9.9.9 Station für Station. Führt er später über andere Stationen, sage ich es. Das passiert nur in Netzen, die als „meins“ gelten.',
    erst: 'Beim nächsten Besuch vergleiche ich mit diesen Werten und sage dir, was anders ist.',
  };
  const KLASSE = {
    Alarm: ['rot', 'Alarm', 'Alarm: nutze das Netz nicht für Passwörter oder Bankgeschäfte, bis du es geklärt hast.'],
    Auffaellig: ['orange', 'Auffällig', 'Auffällig: schau nach, ob das so gewollt ist.'],
    Harmlos: ['gruen', 'Harmlos', 'Harmlos heißt: kommt im Alltag vor.'],
  };

  let aktErg = null;
  const KLASSE_RANG = { Harmlos: 0, Auffaellig: 1, Alarm: 2 };
  const ja = (v) => (v == null ? null : v ? 'ja' : 'nein');
  const tlsWort = (x, host) => ({ Normal: 'normal', Aufgebrochen: 'aufgebrochen' })[x.tls.find((t) => t.host === host)?.verdict] ?? null;

  // Findings from pfadspur.rs arrive as sentences only. „Unverändert“ and „nicht beurteilbar“ are the
  // quiet ones: they belong to the row „Weg ins Internet“, not to the list of differences.
  // ponytail: matches the sentence start, so a rewording in src/pfadspur.rs needs a change here; upgrade: a field in PruefErgebnis.
  const istWeg = (u) => /^(Der Weg zu |Zwischen Router und Anbieter steht auf dem Weg zu )/.test(u.text);
  const wegRuhig = (u) => istWeg(u) && u.klasse === 'Harmlos' && /dieselben Stationen|nicht beurteilbar/.test(u.text);
  const THEMA = [
    [/DHCP/, () => 'DHCP-Server'],
    [/Zertifikat von ([\w.-]+)/, (m) => `Zertifikat von ${m[1]}`],
    [/erfindet|falsche Namen/, () => 'Umleitung'],
    [/DNS/, () => 'DNS'],
    [/^(Der Weg zu |Zwischen Router)/, () => 'Weg ins Internet'],
    [/blockiert/, () => 'Sperre'],
    [/öffentliche IP/, () => 'Öffentliche IP'],
    [/WLAN|Wi-Fi/, () => 'WLAN'],
  ];
  const thema = (t) => { for (const [re, f] of THEMA) { const m = t.match(re); if (m) return f(m); } return 'Netz'; };

  // The answer outlives the view: `pruefen` stores the new baseline, so showing the tab
  // again must not measure again.
  const z = { laeuft: false, offen: {}, erg: null, fehler: null, art: null, portal: null, sni: null };
  let ui = null;

  // One row: current value right, the old one below it when it differs.
  const reihe = (label, m, o, wert, erkl) => {
    const neu = wert(m);
    const alt = o ? wert(o) : null;
    // The changed row wears the severity of its sentence above.
    const t = /DHCP/.test(label) ? 'DHCP-Server' : /^DNS/.test(label) ? 'DNS' : /^Erfundene/.test(label) ? 'Umleitung'
      : /blockiert/.test(label) ? 'Sperre' : label === 'Öffentliche IP' ? label : `Zertifikat von ${label}`;
    const rang = Math.max(-1, ...aktErg.unterschiede.filter((u) => thema(u.text) === t).map((u) => KLASSE_RANG[u.klasse]));
    const farbe = ['gruen', 'orange', 'rot'][rang < 0 ? 1 : rang]; // same colour as the sentence in „Unterschiede“
    return el('div', { class: 'zeile' },
      el('dt', { class: 'label', text: label }),
      el('dd', { class: `wert${neu == null ? ' leer' : ''}` },
        neu ?? 'nicht messbar',
        neu != null && alt != null && alt !== neu && el('span', { class: 'alt' }, el('span', { class: `punkt ${farbe}` }), `geändert, vorher ${alt}`)),
      erkl && el('dd', { class: 'erklaerung', text: erkl }));
  };

  // What the two cards found, as head lines (dot plus word). The red box below stays for real changes since the last visit.
  const befunde = () => {
    const s = z.sni;
    const fremd = s?.namen?.some(([, st]) => st === 'fremdes_zertifikat');
    return [
      z.portal?.status === 'PortalVermutet' && ['orange', 'Auffällig', 'Anmeldeseite vermutet.'],
      s?.gestoert?.length && (fremd
        ? ['rot', 'Alarm', `Webfilter: fremdes Zertifikat bei ${s.gestoert.join(', ')}. Jemand liest vielleicht mit.`]
        : ['orange', 'Auffällig', `Webfilter stört ${s.gestoert.join(', ')}.`]),
    ].filter(Boolean);
  };
  const kartenBefunde = (ausser) => befunde().filter((b) => b !== ausser).map(([f, w, t]) => status(f, `${w}. ${t}`));
  // The verdict: one card, severity as dot, big word and colour; the sentence below.
  const urteilKarte = (farbe, wort, satz) => el('div', { class: `urteil ${farbe}` },
    el('strong', {}, el('span', { class: `punkt ${farbe}` }), wort), el('p', { text: satz }));

  // The head sums up the whole page: comparison with the last visit plus both cards. Green only when all say nothing.
  function urteil(erg) {
    const worst = diffs(erg)[0]?.klasse;
    const seit = `Seit dem letzten Besuch am ${zeit(erg.vorher.time)}`;
    const karten = kartenBefunde();
    const misst = z.offen.sni || z.offen.portal;
    const was = [z.offen.sni && 'Webfilter', z.offen.portal && 'Anmeldeseite'].filter(Boolean).join(' und ');
    const wartet = misst && status('grau', `${was} ${z.offen.sni && z.offen.portal ? 'werden' : 'wird'} noch gemessen.`);
    if (worst === 'Alarm') return [urteilKarte('rot', 'Alarm', `${seit} hat sich etwas geändert, das du klären solltest.`), ...karten, wartet];
    if (worst === 'Auffaellig') return [urteilKarte('orange', 'Auffällig', `${seit} ist etwas anders, schau nach, ob das so gewollt ist.`), ...karten, wartet];
    if (karten.length) {
      const b = befunde();
      const haupt = b.find((x) => x[0] === 'rot') ?? b[0];
      return [urteilKarte(haupt[0], haupt[1], haupt[2]), ...kartenBefunde(haupt), wartet];
    }
    if (misst) return [status('grau', `${seit} ist nichts anders. ${was} ${z.offen.sni && z.offen.portal ? 'werden' : 'wird'} noch gemessen.`)];
    const unklar = z.sni?.unklar && status('grau', 'Der Webfilter ließ sich nicht beurteilen.');
    return [urteilKarte('gruen', 'Nichts Auffälliges', `${seit} ist alles im Rahmen.`), unklar];
  }

  const diffs = (erg) => erg.unterschiede.filter((u) => !wegRuhig(u));

  // Leads: topic and severity (dot plus word) on top, the sentence below. The detail groups only mark the changed row.
  function unterschiede(erg) {
    const erklaert = new Set();
    return gruppe('Unterschiede', diffs(erg).map((u) => {
      const [farbe, wort, erkl] = KLASSE[u.klasse];
      const nochNicht = !erklaert.has(u.klasse);
      erklaert.add(u.klasse);
      return el('div', { class: 'zeile' },
        el('dt', { class: 'label thema', text: thema(u.text) }),
        el('dd', { class: 'wert klasse' }, el('span', { class: `punkt ${farbe}` }), el('span', { class: 'wort', text: wort })),
        el('dd', { class: 'wert text', text: u.text }),
        nochNicht && el('dd', { class: 'erklaerung', text: erkl }));
    }));
  }

  // Row „Weg ins Internet“ (#66): unchanged, changed, alarm, from the findings of pfadspur.
  function wegZeile(erg) {
    const w = erg.unterschiede.filter(istWeg);
    const laut = w.filter((u) => !wegRuhig(u)).sort((a, b) => KLASSE_RANG[b.klasse] - KLASSE_RANG[a.klasse])[0];
    const [farbe, wort] = laut ? (laut.klasse === 'Harmlos' ? ['gruen', 'leicht geändert'] : [KLASSE[laut.klasse][0], laut.klasse === 'Alarm' ? 'Alarm' : 'geändert'])
      : w.some((u) => /dieselben Stationen/.test(u.text)) ? ['gruen', 'unverändert']
      : w.length ? ['grau', 'nicht beurteilbar'] : [null, 'nicht gemessen'];
    return el('div', { class: 'zeile' },
      el('dt', { class: 'label', text: 'Weg ins Internet' }),
      el('dd', { class: `wert klasse${farbe ? '' : ' leer'}` }, farbe && el('span', { class: `punkt ${farbe}` }), wort),
      el('dd', { class: 'erklaerung', text: E.weg }));
  }

  function werte(erg) {
    aktErg = erg;
    const m = erg.messung;
    const o = erg.vorher;
    const r = (label, wert, erkl) => reihe(label, m, o, wert, erkl);
    const n = erg.nicht_messbar;
    return [
      gruppe('Netz', [
        r('Router', (x) => x.router, E.router),
        r('DHCP-Server', (x) => x.dhcp_server, E.dhcp),
        r('DNS-Server vom Netz', (x) => (x.dhcp_dns.length ? x.dhcp_dns.join(', ') : 'keine'), E.dns),
        r('Öffentliche IP', (x) => x.public_ip, E.ip),
        wegZeile(erg),
      ], n > 0 && (n === 1
        ? 'Eine Prüfung ließ sich nicht messen (Internet? Zeitüberschreitung?). Sie zählt nicht als Änderung.'
        : `${n} Prüfungen ließen sich nicht messen (Internet? Zeitüberschreitung?). Sie zählen nicht als Änderung.`)),
      gruppe('Sperren und Umleitungen', [
        r('github.com blockiert, im Netz', (x) => ja(x.blocked_local), E.sperre),
        r('github.com blockiert, bei 1.1.1.1', (x) => ja(x.blocked_public)),
        r('Erfundene Antworten für falsche Namen', (x) => ja(x.hijack), E.umleitung),
      ]),
      gruppe('Zertifikate', m.tls.map((t, i) => r(t.host, (x) => tlsWort(x, t.host), i === 0 ? E.tls : null))),
    ];
  }

  // Card „Webfilter am Servernamen“ (`netzinfo sni`, #52). `null`: not measured yet or failed, no card.
  const SNI_WORT = { ok: 'ok', zurueckgesetzt: 'Verbindung abgebrochen', zeitueberschreitung: 'Zeitüberschreitung', fremdes_zertifikat: 'fremdes Zertifikat' };
  const sniKarte = () => {
    const s = z.sni;
    if (!s) return null;
    const zeile = (label, wert, farbe, erkl) => el('div', { class: 'zeile' },
      el('dt', { class: 'label klasse' }, farbe && el('span', { class: `punkt ${farbe}` }), label),
      el('dd', { class: 'wert', text: wert }),
      erkl && el('dd', { class: 'erklaerung', text: erkl }));
    if (s.unklar) return gruppe('Webfilter am Servernamen', [zeile('Ergebnis', 'unklar', 'grau', 'Schon die Kontrolle kam nicht durch. Der Testserver ist nicht erreichbar oder das Netz ist gestört, ein Urteil wäre geraten.')]);
    const farbe = (st) => (st === 'ok' ? 'gruen' : st === 'fremdes_zertifikat' ? 'rot' : 'orange');
    return gruppe('Webfilter am Servernamen', [
      zeile('Ergebnis', s.gestoert.length ? `gestört für ${s.gestoert.join(', ')}` : 'kein Name gestört', s.gestoert.length ? 'orange' : 'gruen', E.sni),
      ...s.namen.map(([n, st]) => zeile(n, SNI_WORT[st], farbe(st))),
    ]);
  };

  // Shown above everything else when the login-page check (`netzinfo portal`) suspects a captive portal.
  const portalHinweis = () => z.portal?.status === 'PortalVermutet' && el('section', { class: 'portal-hinweis', role: 'alert' },
    el('h3', { text: 'Anmeldung nötig' }),
    el('p', { text: `Dieses Netz leitet vermutlich auf eine Anmeldeseite um${z.portal.ziel_host ? ` (${z.portal.ziel_host})` : ''}. Solange du dich nicht angemeldet hast, sind die übrigen Ergebnisse unzuverlässig.` }),
    el('button', { class: 'knopf haupt', type: 'button', text: 'Im Browser öffnen', onclick: (ev) => Netz.invoke('portal_im_browser_oeffnen').catch((f) => ev.target.after(meldung(Netz.text(f)))) }),
    el('p', { class: 'erklaerung', text: E.portal }));

  function zeichne() {
    if (!ui) return;
    const { kopf, rest, knopf } = ui;
    const erg = z.erg;
    const unbekannt = z.art === 'nicht_gemerkt';
    knopf.disabled = z.laeuft;
    // One main action per area: with the portal hint its button leads.
    knopf.classList.toggle('haupt', z.portal?.status !== 'PortalVermutet');
    knopf.parentElement.hidden = unbekannt || (z.fehler && !z.laeuft); // there the empty state or the error card carries the button
    knopf.replaceChildren(z.laeuft ? 'Prüfe …' : 'Jetzt prüfen', ...(z.laeuft ? [] : [el('span', { class: 'kbd', 'aria-hidden': 'true', text: '⌘R' })]));
    const fuell = (ort, ...k) => ort.replaceChildren(...k.flat().filter((x) => x != null && x !== false));
    fuell(kopf,
      portalHinweis(),
      el('h2', { text: erg ? erg.netz : unbekannt ? 'Unbekanntes Netz' : 'Prüfung' }),
      z.offen.vergleich ? status('grau', 'Prüfe …')
        : erg ? (erg.erster_besuch ? [status('grau', 'Erste Messung, ab jetzt die Vergleichsbasis'), ...kartenBefunde()] : urteil(erg))
        : null);
    fuell(rest,
      ...(z.fehler && !z.offen.vergleich ? (unbekannt ? [
        Netz.leerZustand('', 'Dieses Netz ist nicht gemerkt, es gibt nichts zum Vergleichen. Merke es unter „Dieses Netz“ und prüfe dann noch mal.', 'pruefung'),
        el('div', { class: 'aktion mitte' },
          el('button', { class: 'knopf haupt', type: 'button', text: 'Zu „Dieses Netz“ …', onclick: () => document.getElementById('tab-dieses-netz').click() }),
          el('button', { class: 'knopf', type: 'button', text: 'Noch mal prüfen', disabled: z.laeuft, onclick: lauf })),
      ] : [Netz.fehlerKarte(z.fehler, lauf)]) : []),
      ...(erg && !z.offen.vergleich ? [
        erg.erster_besuch && el('p', { class: 'fuss erklaerung', text: E.erst }),
        !erg.erster_besuch && diffs(erg).length > 0 && unterschiede(erg),
        ...werte(erg),
        sniKarte(),
        el('p', { class: 'fuss', text: `Gemessen am ${zeit(erg.messung.time)}` }),
      ] : []));
  }

  async function lauf() {
    if (z.laeuft) return;
    z.laeuft = true;
    z.fehler = null;
    z.art = null;
    z.sni = z.portal = null;
    z.offen = { vergleich: true, sni: true, portal: true };
    zeichne();
    // All three run alongside; each one redraws the head when it is in, so the head never says „fine“ before a card has spoken.
    const fertig = (name, f) => (w) => { z.offen[name] = false; f(w); zeichne(); };
    const portal = Netz.invoke('portal_pruefen').catch(() => null).then(fertig('portal', (w) => { z.portal = w; })); // a failed check just shows no hint
    const sni = Netz.invoke('sni_pruefen').catch(() => null).then(fertig('sni', (w) => { z.sni = w; })); // same: no card
    try {
      z.erg = await Netz.invoke('pruefen');
    } catch (e) {
      z.erg = null;
      z.fehler = Netz.text(e);
      z.art = e?.art;
    }
    z.offen.vergleich = false;
    zeichne();
    await Promise.all([sni, portal]);
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
    aktualisieren: false, // ⌘R is „Jetzt prüfen“ here
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
