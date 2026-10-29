// Fake answers for the view „qualitaet“ in vorschau.html.
// Marks: #qualitaet (curve with some losses), ,qualitaet-luecken (20 min pause), ,qualitaet-leer,
// ,qualitaet-fehler, ,qualitaet-unbekannt, Karte „Weg zum Ziel“: ,pfad-laeuft ,pfad-verlust ,pfad-nicht-erreicht ,pfad-ok
// (misst sofort), optional ,qualitaet-6h and ,erklaeren.
(() => {
  const NAMEN = ['Router (DNS)', '1.1.1.1 (DNS)', '1.1.1.1 (TCP 443)'];
  const jetzt = () => Math.floor(Date.now() / 1000);
  const rausch = (i, k) => Math.abs(Math.sin(i * 12.9898 + k * 78.233) * 43758.5453) % 1;

  // Deterministic fake series: every 30 s over 3 h, spikes now and then, a few losses.
  function reihen() {
    const ende = jetzt();
    return NAMEN.map((ziel, k) => {
      const punkte = [];
      for (let i = 0, t = ende - 3 * 3600; t < ende; i++, t += 30) {
        if (marken.includes('qualitaet-luecken') && t > ende - 2400 && t < ende - 1200) continue;
        const r = rausch(i, k);
        const spitze = i % 97 < 4 ? 25 + k * 20 : 0;
        const verlust = r > 0.985 || (k === 2 && i % 131 < 2);
        punkte.push({ zeit: t, ms: verlust ? null : [3, 14, 18][k] + r * 6 + spitze, verlust });
      }
      return { ziel, punkte };
    });
  }

  function bericht() {
    const r = reihen();
    const alle = r.flatMap((x) => x.punkte);
    const ziele = r.map(({ ziel, punkte }, k) => {
      const w = punkte.filter((p) => p.ms != null).map((p) => p.ms).sort((a, b) => a - b);
      const v = punkte.filter((p) => p.verlust).length;
      return {
        ziel, messungen: punkte.length, verloren: v, verlust_prozent: (v / punkte.length) * 100,
        median_ms: w[Math.floor(w.length / 2)], p95_ms: w[Math.floor(w.length * 0.95)], max_ms: w[w.length - 1],
        laengster_aussetzer_s: 30 * (k + 2),
        schlechteste_stunde: { stunde: Math.floor(punkte[0].zeit / 3600) * 3600, verlust_prozent: Math.round((v / punkte.length) * 2200) / 10, durchschnitt_ms: w[Math.floor(w.length / 2)] + 2 },
      };
    });
    return { netz: 'WG', von: alle[0].zeit, bis: jetzt(), ziele, reihen: r };
  }

  VORSCHAU.bericht = async () => {
    if (marken.includes('qualitaet-fehler')) throw 'Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.';
    if (marken.includes('qualitaet-unbekannt')) throw { art: 'nicht_gemerkt', text: 'Dieses Netz ist nicht gemerkt.' };
    if (marken.includes('qualitaet-leer')) return { netz: 'WG', von: 0, bis: 0, ziele: [], reihen: [] };
    return bericht();
  };
  VORSCHAU.messen_einmal = async () => ({
    zeit: jetzt(),
    werte: NAMEN.map((ziel, k) => ({ ziel, ms: [3, 14, 18][k] + Math.random() * 6, verlust: false })),
  });

  // ,umfeld (Router auf Kanal 6, Rat: Kanal 11) ,umfeld-leer ,umfeld-fehler: klickt „Umgebung ansehen“.
  // Erfundene Netze; Auslastung rechnet wie src/kanal.rs (2,4 GHz: Kanäle bis 3 daneben stören, 5 und 6 GHz nur derselbe Kanal).
  VORSCHAU.wlan_umfeld = async () => {
    if (marken.includes('umfeld-fehler')) throw 'Der WLAN-Scan ging schief (keine Berechtigung). Prüfe, ob das WLAN eingeschaltet ist, und versuche es noch mal.';
    if (marken.includes('umfeld-leer')) return { beobachtungen: [], aktuell: null, land: 'DE', belegung: [], empfehlung: { text: '', ziel: null } };
    const o = (band, kanal, rssi) => ({ band, kanal, breite_mhz: 20, rssi });
    const beob = [
      ...[-52, -58, -61, -64, -67, -70, -73, -77, -81].map((r) => o('2,4 GHz', 6, r)),
      o('2,4 GHz', 1, -66), o('2,4 GHz', 3, -74), o('2,4 GHz', 4, -79), o('2,4 GHz', 9, -83),
      o('5 GHz', 36, -58), o('5 GHz', 36, -72), o('5 GHz', 44, -76), o('5 GHz', 100, -80), o('5 GHz', 149, -69),
      o('6 GHz', 37, -84),
    ];
    const last = (band, kanal) => {
      const t = beob.filter((b) => b.band === band && (b.kanal === kanal || (band === '2,4 GHz' && Math.abs(b.kanal - kanal) < 4)));
      return { band, kanal, netze: t.length, dbm: 10 * Math.log10(t.reduce((s, b) => s + 10 ** (b.rssi / 10), 0)) };
    };
    const belegung = [...new Set(beob.map((b) => `${b.band}|${b.kanal}`))].map((k) => k.split('|')).sort((a, b) => a[0].localeCompare(b[0]) || a[1] - b[1]).map(([b, k]) => last(b, Number(k)));
    const jetzt = last('2,4 GHz', 6);
    const ziel = last('2,4 GHz', 11); // 1, 6 und 11 sind die Kandidaten; 11 ist am leersten
    return {
      beobachtungen: beob, aktuell: { band: '2,4 GHz', kanal: 6 }, land: 'DE', belegung,
      empfehlung: { text: `Auf Kanal 6 (2,4 GHz) senden ${jetzt.netze} Netze. Kanal 11 ist leerer (${ziel.netze} Netz dort). Den Kanal stellst du in den WLAN-Einstellungen deines Routers ein.`, ziel: { band: '2,4 GHz', kanal: 11 } },
    };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('umfeld'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.umfeld-karte .knopf')?.click(), 900);
      setTimeout(() => document.querySelector('.umfeld-karte')?.scrollIntoView(), 1200);
    }
    if (marken.some((m) => m.startsWith('qualitaet') || m.startsWith('pfad-'))) setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
    if (marken.includes('qualitaet-6h')) setTimeout(() => { const s = document.querySelector('.werkzeuge select'); if (s) { s.value = '6'; s.dispatchEvent(new Event('change')); } }, 400);
  });

  // Weg zum Ziel: invented hops, addresses only from 192.0.2.x and 203.0.113.x.
  const hop = (ttl, adresse, median, verlust, extra = {}) => ({
    ttl, adressen: adresse ? [adresse] : [], name: null, gesendet: 10, beantwortet: Math.round(10 - verlust / 10), verlust_prozent: verlust,
    min_ms: median && median - 1, median_ms: median, p95_ms: median && median + 3, jitter_ms: median && 0.8,
    lokal: ttl < 3, erster_oeffentlicher: ttl === 3, ziel: false, ...extra,
  });
  const stationen = [hop(1, '192.0.2.1', 1.2, 0), hop(2, '192.0.2.129', 8.4, 0), hop(3, '203.0.113.9', 11.9, 0), hop(4, '203.0.113.33', 13.5, 10), hop(5, '203.0.113.200', 14.8, 0)];
  VORSCHAU.pfad_messen = async ({ ziel }) => {
    if (marken.includes('pfad-laeuft')) return new Promise(() => {});
    const base = { ziel, ziel_ip: '203.0.113.200', zeitpunkt: 1793050200, erreicht: true };
    if (marken.includes('pfad-nicht-erreicht')) {
      return { ...base, erreicht: false, hops: [...stationen.slice(0, 3), hop(4, null, null, 100), hop(5, null, null, 100)] };
    }
    const letzter = marken.includes('pfad-verlust') ? hop(5, '203.0.113.200', 15.2, 30, { ziel: true }) : hop(5, '203.0.113.200', 14.8, 0, { ziel: true });
    return { ...base, hops: [...stationen.slice(0, 4), letzter] };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('pfad-'))) setTimeout(() => { document.querySelector('.weg-karte .werkzeuge .knopf:last-child')?.click(); document.querySelector('.weg-karte')?.scrollIntoView(); }, 500);
  });
})();

// „Wo hakt es?“ (issue #31, Funk #37). Marks: ,diagnose-ok (default), ,diagnose-router, ,diagnose-dns, ,diagnose-laeuft,
// Funk (#37): ,diagnose-funk (weak radio + bad router + history sentence), ,diagnose-funk-gut (radio fine, router bad),
// ,diagnose-kabel (no Wi-Fi), plus ,qualitaet; the button is clicked for you. All numbers are made up.
(() => {
  const z = (name, ms, v = 0, status = 'ok') => ({ name, gesendet: 20, beantwortet: ms == null ? 0 : Math.round(20 * (1 - v / 100)), verlust_prozent: v, median_ms: ms, p95_ms: ms && ms * 1.8, jitter_ms: ms && 3, status, gruende: [] });
  const gut = [z('Router', 3.2), z('Erster Rechner des Anbieters', 9.8), z('1.1.1.1', 12.1), z('9.9.9.9', 13.4)];
  const funk = (status, rssi_dbm, snr_db) => ({ status, rssi_dbm, snr_db, band: '5 GHz', kanal: 36 });
  const routerSchlecht = [z('Router', 41.0, 15, 'schlecht'), ...gut.slice(1)];
  const routerGruende = ['Router verliert 15,0 % der Pakete.', 'Router: Im schlechten Moment (95. Perzentil) 73,8 ms statt sonst 6,1 ms, mehr als das Dreifache. Im Mittel (Median) antwortet Router hier sonst in 3,0 ms, jetzt in 41,0 ms.'];
  const basis = { tcp_443: null, mit_basis: true, zeitpunkt: 0, verlauf: null };
  const antwort = () => {
    if (marken.includes('diagnose-funk')) return { ...basis, ziele: routerSchlecht, funk: funk('schwach', -78, 12), urteil: 'funk', satz: 'Das Problem liegt sehr wahrscheinlich im Funk (schwaches WLAN-Signal), nicht beim Internet.', begruendung: ['Das WLAN-Signal ist schwach (Signal -78 dBm, Abstand zum Rauschen 12 dB), Grenzen: unter -75 dBm oder unter 15 dB.', ...routerGruende], verlauf: 'Hier war es zuletzt 3 von 5 Mal der Funk.' };
    if (marken.includes('diagnose-funk-gut')) return { ...basis, ziele: routerSchlecht, funk: funk('gut', -52, 41), urteil: 'wlan_oder_router', satz: 'Das Problem liegt sehr wahrscheinlich am Router oder am Kabel dahinter, nicht am Funk.', begruendung: [...routerGruende, 'Der Funk ist gut (Signal -52 dBm, Abstand zum Rauschen 41 dB), also liegt es sehr wahrscheinlich am Router oder am Kabel dahinter.'] };
    if (marken.includes('diagnose-kabel')) return { ...basis, ziele: gut, funk: funk('kabel', null, null), urteil: 'in_ordnung', satz: 'Keine Auffälligkeit gemessen.', begruendung: ['Router und beide Internet-Ziele antworten ohne Auffälligkeit.'] };
    if (marken.includes('diagnose-router')) return { ...basis, ziele: routerSchlecht, funk: funk('nicht_geprueft', null, null), urteil: 'wlan_oder_router', satz: 'Das Problem liegt wahrscheinlich im WLAN oder am Router.', begruendung: routerGruende };
    if (marken.includes('diagnose-dns')) return { ...basis, mit_basis: false, ziele: [gut[0], z('Erster Rechner des Anbieters', 62, 20, 'schlecht'), z('1.1.1.1', 180, 10, 'schlecht'), z('9.9.9.9', 190, 25, 'schlecht')], funk: funk('gut', -55, 38), urteil: 'anbieter', satz: 'Das Problem liegt wahrscheinlich beim Anbieter.', begruendung: ['Der Router antwortet in Ordnung.', 'Erster Rechner des Anbieters verliert 20,0 % der Pakete.', '1.1.1.1: Median 180,0 ms, über der festen Grenze von 150,0 ms.'] };
    return { ...basis, ziele: gut, funk: funk('gut', -55, 38), urteil: 'in_ordnung', satz: 'Keine Auffälligkeit gemessen.', begruendung: ['Router und beide Internet-Ziele antworten ohne Auffälligkeit.'] };
  };
  VORSCHAU.diagnose = () => new Promise((ok) => setTimeout(() => ok(antwort()), verzoegerung()));
  const verzoegerung = () => (marken.includes('diagnose-laeuft') ? 60000 : 300);
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('diagnose'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.diagnose button')?.click(), 500);
      setTimeout(() => document.querySelector('.diagnose')?.scrollIntoView(), 1200);
    }
  });
})();

// Karte „WLAN-Verlauf“ (#36). Marks: ,qualitaet (default: Verlauf mit Roaming-Marke und Abbruch), ,wlan-kabel (kein WLAN),
// ,wlan-ohne-hash (Roaming nicht erkennbar), ,wlan-vergleich (zusätzlich Besuchsvergleich ist immer dabei). Alle Zahlen erfunden.
(() => {
  const jetzt = () => Math.floor(Date.now() / 1000);
  VORSCHAU.wlan_verlauf_abtasten = async () => {
    if (marken.includes('wlan-kabel')) return { verbunden: false, proben: [{ zeit: jetzt(), verbunden: false }], ereignisse: [], besuch: null, vergleich: null, gemerkt: true, roaming_erkennbar: false, hinweis: null };
    const ohne = marken.includes('wlan-ohne-hash');
    const ende = jetzt();
    const proben = [];
    for (let i = 0; i < 120; i++) {
      const zeit = ende - (119 - i) * 5;
      const weg = i >= 88 && i < 91;
      const nach = i >= 70;
      const rssi = Math.round((nach ? -48 : -63) - (i > 40 && i < 55 ? 8 : 0) + Math.sin(i / 3) * 3);
      proben.push(weg ? { zeit, verbunden: false } : { zeit, verbunden: true, rssi, rauschen: -92, rate: Math.round((nach ? 780 : 420) + Math.sin(i) * 40), band: '5 GHz', bssid_hash: ohne ? null : (nach ? 'b' : 'a') });
    }
    const ereignisse = [{ zeit: proben[70].zeit, art: 'Roaming' }, { zeit: proben[88].zeit, art: 'Getrennt' }, { zeit: proben[91].zeit, art: 'Verbunden' }];
    return {
      verbunden: true, proben, gemerkt: true, roaming_erkennbar: !ohne, hinweis: null,
      ereignisse: ohne ? ereignisse.slice(1) : ereignisse,
      besuch: { von: proben[0].zeit, dauer_s: 595, signal: { min: -71, median: -55, max: -45 }, rate: { min: 380, median: 600, max: 820 }, wechsel: ohne ? 0 : 1, abbrueche: 1, roaming_erkennbar: !ohne },
      vergleich: 'Dieses Mal im Median 6 dB schwächer als sonst hier (4 frühere Besuche).',
    };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('wlan-'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.wlan-karte')?.scrollIntoView(), 900);
    }
  });
})();

// Karte „Paketgröße“ (#47). Marks: ,mtu-ok (1500) ,mtu-pppoe (1492, default) ,mtu-klein (1280, VPN) ,mtu-unsicher (Stille)
// ,mtu-anders (Besuch weicht ab) ,mtu-ungemerkt ,mtu-laeuft ,mtu-fehler; misst sofort. Alle Zahlen erfunden.
(() => {
  const satz = {
    1500: 'Dein Netz lässt Pakete bis 1500 Byte durch. Das ist der Normalwert.',
    1492: 'Dein Netz lässt Pakete bis 1492 Byte durch statt 1500. Das ist bei DSL mit PPPoE üblich und normalerweise kein Fehler.',
    1280: 'Bei 1280 Byte ist ein Tunnel oder Mobilfunk im Weg. Große Dateien über manche Seiten können dann hängen. Die Grenze liegt schon an deiner eigenen Schnittstelle, zum Beispiel einem VPN.',
  };
  VORSCHAU.mtu_messen = async ({ ziel }) => {
    if (marken.includes('mtu-laeuft')) return new Promise(() => {});
    if (marken.includes('mtu-fehler')) throw 'Schon 576 Byte große Pakete kommen nicht durch. Entweder blockt das Ziel Ping, oder der Weg ist gestört; die Paketgröße lässt sich so nicht bestimmen. Probiere ein anderes Ziel, zum Beispiel 1.1.1.1.';
    const mtu = marken.includes('mtu-ok') ? 1500 : marken.includes('mtu-klein') ? 1280 : 1492;
    const unsicher = marken.includes('mtu-unsicher');
    return {
      ziel, ziel_ip: '203.0.113.200', pfad_mtu: mtu, lokale_mtu: mtu === 1280 ? 1280 : 1500,
      quelle: unsicher ? 'stille' : 'antwort',
      einstufung: { 1500: 'normal', 1492: 'pppoe', 1280: 'klein' }[mtu],
      satz: satz[mtu] + (unsicher ? ' Das Ergebnis ist unsicher: Größere Pakete bekamen keine Antwort, und manche Netze verwerfen die Meldung „Paket zu groß“ ganz.' : ''),
      zeitpunkt: 1793050200,
      netz: marken.includes('mtu-ungemerkt') ? null : 'WG',
      vorher: marken.includes('mtu-anders') ? 1500 : marken.includes('mtu-ungemerkt') ? null : mtu,
    };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('mtu-'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => { document.querySelector('.mtu-karte .werkzeuge .knopf:last-child')?.click(); document.querySelector('.mtu-karte')?.scrollIntoView(); }, 600);
    }
  });
})();

// Karte „IPv6“ (#48). Marks: ,ipv6-ok (default, IPv6 4 ms schneller), ,ipv6-langsamer, ,ipv6-kaputt, ,ipv6-kein; the button is clicked for you.
// Services are real public names, all numbers are made up.
(() => {
  const proto = (median, fehler = 0) => ({ aufgeloest: true, dns_ms: 9, median_ms: median, versuche: 5, fehler });
  const ziele = (v4, v6) => ['cloudflare.com', 'google.com', 'wikipedia.org', 'dns.quad9.net', 'www.apple.com'].map((name) => ({ name, v4: proto(v4), v6: v6 == null ? proto(null, 5) : proto(v6), rennen: null }));
  const lokal = (global) => ({ globale_adresse: global, standardroute: global, router_ankuendigung: global });
  const antwort = () => {
    if (marken.includes('ipv6-kein')) return { urteil: 'kein_ipv6', satz: 'Dieses Netz gibt dir kein IPv6, alles läuft über IPv4. Das ist heute noch weit verbreitet und kein Fehler.', lokal: lokal(false), ziele: [], median_v4_ms: null, median_v6_ms: null };
    if (marken.includes('ipv6-kaputt')) return { urteil: 'kaputt', satz: 'Dein Netz hat eine IPv6-Adresse, aber Verbindungen darüber scheitern. Programme warten dann erst und weichen dann auf IPv4 aus. Das macht Seiten spürbar langsamer.', lokal: lokal(true), ziele: ziele(20, null), median_v4_ms: null, median_v6_ms: null };
    if (marken.includes('ipv6-langsamer')) return { urteil: 'langsamer', satz: 'IPv6 funktioniert, ist hier aber 40 ms langsamer als IPv4. Dein Rechner startet IPv6 zuerst und nimmt nach 250 ms zusätzlich IPv4, es gewinnt die Verbindung, die zuerst steht.', lokal: lokal(true), ziele: ziele(20, 60), median_v4_ms: 20, median_v6_ms: 60 };
    return { urteil: 'gleich', satz: 'IPv6 ist hier 4 ms schneller als IPv4. Dein Rechner nimmt es automatisch.', lokal: lokal(true), ziele: ziele(20, 16), median_v4_ms: 20, median_v6_ms: 16 };
  };
  VORSCHAU.ipv6_messen = () => new Promise((ok) => setTimeout(() => ok(antwort()), 300));
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('ipv6-'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.ipv6-karte button')?.click(), 500);
      setTimeout(() => document.querySelector('.ipv6-karte')?.scrollIntoView(), 1200);
    }
  });
})();

// Zeile „Uhrzeit“ (issue #69). Marks: ,zeit-ok (default), ,zeit-netz (Router 4 min falsch), ,zeit-mac (Mac-Uhr 3 min falsch),
// ,zeit-gesperrt (NTP blockiert). Versätze in ms zur Mac-Uhr.
(() => {
  const b = (quelle, v, netz = false) => ({ quelle, netz, versatz_ms: v, fehler: v == null ? 'keine Antwort, NTP ist in manchen Netzen gesperrt' : null });
  VORSCHAU.zeit_pruefen = async () => {
    if (marken.includes('zeit-netz')) return { befunde: [b('Router (NTP)', 240_300, true), b('time.apple.com (NTP)', 120), b('pool.ntp.org (NTP)', -85), b('www.cloudflare.com (HTTPS Date)', 310)], unterschied_netz_zu_oeffentlich_ms: 240_180, klasse: 'Alarm', satz: 'Die Uhr, die dieses Netz anbietet, geht 4 Minuten vor. Dadurch können sichere Verbindungen fehlschlagen.' };
    if (marken.includes('zeit-mac')) return { befunde: [b('time.apple.com (NTP)', 180_050), b('pool.ntp.org (NTP)', 180_120), b('www.cloudflare.com (HTTPS Date)', 179_900)], unterschied_netz_zu_oeffentlich_ms: null, klasse: 'Auffaellig', satz: 'Die Uhr deines Macs geht 3 Minuten nach. Dadurch können sichere Verbindungen fehlschlagen.' };
    if (marken.includes('zeit-gesperrt')) return { befunde: [b('time.apple.com (NTP)', null), b('pool.ntp.org (NTP)', null), b('www.cloudflare.com (HTTPS Date)', null)], unterschied_netz_zu_oeffentlich_ms: null, klasse: null, satz: 'Nicht geprüft: Keine öffentliche Zeitquelle antwortete. NTP ist in manchen Netzen gesperrt.' };
    return { befunde: [b('Router (NTP)', 35, true), b('time.apple.com (NTP)', 42), b('pool.ntp.org (NTP)', -18), b('www.cloudflare.com (HTTPS Date)', 260)], unterschied_netz_zu_oeffentlich_ms: 0, klasse: 'Harmlos', satz: 'Deine Uhr stimmt mit 4 unabhängigen Quellen überein.' };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('zeit-'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => document.querySelector('.zeit-karte')?.scrollIntoView(), 900);
    }
  });
})();

// Karte „DNS-Tempo“ (#50). Marks: ,dns-tempo-wechsel (Empfehlung), sonst „kein Wechsel nötig“; ,dns-tempo-fehler. Alle Zahlen erfunden.
(() => {
  const z = (name, oeffentlich, bm, b95, um, u95, f = 0) => ({ name, oeffentlich, bekannt_median: bm, bekannt_p95: b95, unbekannt_median: um, unbekannt_p95: u95, fehlerrate: f, ausreisser: 0 });
  VORSCHAU.dns_tempo = async () => {
    if (marken.includes('dns-tempo-fehler')) throw 'Kein DNS-Server gefunden: Das System nennt keinen Resolver. Verbinde den Mac mit einem Netz und versuche es noch mal.';
    const wechsel = marken.includes('dns-tempo-wechsel');
    const zeilen = [wechsel ? z('deinem Netz (192.0.2.1)', false, 24, 40, 80, 120) : z('deinem Netz (192.0.2.1)', false, 2, 5, 38, 60), z('1.1.1.1 (Cloudflare)', true, 12, 20, 30, 45), z('8.8.8.8 (Google)', true, 14, 22, 34, 50), z('9.9.9.9 (Quad9)', true, 15, 25, 36, 52)];
    return {
      zeilen, empfehlung: wechsel ? 1 : null,
      satz: wechsel ? 'Empfehlung: 1.1.1.1 (Cloudflare) antwortet auf bekannte und neue Namen mindestens 30 % schneller als dein Netz.' : 'Kein Wechsel nötig.',
      erklaerung: wechsel ? 'Ein Wechsel lohnt, weil der Server auf beiden Gruppen deutlich schneller ist und nicht öfter ausfällt. Bedenke: Schneller zu sein ist nicht das Einzige, was zählt. Quad9 filtert zum Beispiel bekannte Schadseiten, Cloudflare nicht, und jeder öffentliche Anbieter sieht deine Namen.' : 'Kein öffentlicher Server ist auf beiden Gruppen mindestens 30 % schneller als dein Netz oder er fällt öfter aus. Ein kleiner Unterschied von wenigen Millisekunden fällt beim Surfen nicht auf.',
      datenschutz: 'Datenschutz: Die öffentlichen Anbieter (1.1.1.1 (Cloudflare), 8.8.8.8 (Google), 9.9.9.9 (Quad9)) sehen die Namen, die du abfragst.',
    };
  };
  if (marken.some((m) => m.startsWith('dns-tempo'))) addEventListener('DOMContentLoaded', () => setTimeout(() => document.querySelector('.dns-tempo-karte button')?.click(), 600));
})();

// Karte „Was darf raus?“ (issue #54). Marks: ,egress-blockiert (22, 25, 3389 zu), ,egress-leer (kein Port offen). Misst auf Klick.
(() => {
  const NAMEN = { 22: 'SSH', 25: 'Mailversand zwischen Servern', 53: 'DNS über TCP', 80: 'HTTP', 123: 'NTP über TCP', 143: 'IMAP', 443: 'HTTPS', 465: 'SMTPS', 587: 'Mailversand', 853: 'DNS über TLS', 993: 'IMAPS', 1194: 'OpenVPN', 3389: 'Remotedesktop', 5222: 'XMPP-Chat', 8080: 'HTTP, Ausweichport', 8443: 'HTTPS, Ausweichport' };
  const zu = { 22: 'verschluckt', 25: 'abgelehnt', 3389: 'verschluckt' };
  VORSCHAU.egress_pruefen = async () => {
    const leer = marken.includes('egress-leer');
    const sperre = marken.includes('egress-blockiert');
    const ports = Object.entries(NAMEN).map(([p, name]) => {
      const port = Number(p);
      const status = leer ? 'verschluckt' : sperre && zu[port] ? zu[port] : 'offen';
      const ms = status === 'offen' ? 18 + (port % 7) : status === 'abgelehnt' ? 9 : 3000;
      const n = `${port} (${name})`;
      const satz = port === 443 && status === 'offen' ? 'Port 443 (HTTPS) ist offen. Normale Webseiten und die meisten VPNs über HTTPS funktionieren.'
        : port === 22 && status !== 'offen' ? 'Dieses Netz lässt Port 22 (SSH) nicht hinaus. Verbindungen per SSH nach außen funktionieren hier nicht.'
        : status === 'offen' ? `Port ${n} ist offen.`
        : status === 'abgelehnt' ? `Port ${n} wird abgelehnt: Die Antwort kam sofort, typisch für eine Firewall, die ausdrücklich ablehnt.`
        : `Port ${n} wird verschluckt: Es kam keine Antwort, typisch für eine Firewall, die still verwirft.`;
      return { port, name, status, ms, satz };
    });
    return { ziel: '203.0.113.7', ports, hinweise: leer ? ['Kein Port war offen. Läuft dein Testserver, und hast du Internet? Das Ergebnis wird nicht gespeichert.'] : sperre ? ['Dieses Netz lässt jetzt Port 22 (SSH), 25 (Mailversand zwischen Servern), 3389 (Remotedesktop) nicht mehr hinaus, vorher ging es. Mit `netzinfo ports --raus` siehst du alle Ports.'] : [] };
  };
  addEventListener('DOMContentLoaded', () => {
    if (marken.some((m) => m.startsWith('egress-'))) {
      setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
      setTimeout(() => { document.querySelector('.egress-karte .knopf')?.click(); document.querySelector('.egress-karte')?.scrollIntoView(); }, 900);
    }
  });
})();

// Karte „Unter Last“ (#49). Marken: ,last-hotspot (Rückfrage) ,last-fehler ,last-verlauf (zwei frühere Läufe); der Knopf wird geklickt. Misst auf Klick. Alle Zahlen erfunden.
(() => {
  const lauf = (zeit, rpm, down) => ({ zeit, ergebnis: { down_bps: down, up_bps: 48e6, ruhe_ms: 14, last_down_ms: 60, last_up_ms: 210, rpm, dauer_s: 30 } });
  const hot = marken.includes('last-hotspot');
  VORSCHAU.last_vorab = async () => ({
    hinweis: 'Misst Download, Upload und die Antwortzeit unter Last. Die Leitung ist dabei etwa 30 Sekunden voll ausgelastet.',
    hotspot: hot,
    hotspot_frage: hot ? 'Dieses Netz sieht nach einem Mobilfunk-Hotspot aus. Der Test kann mehrere hundert Megabyte deines Datenvolumens verbrauchen.' : '',
    verlauf: marken.includes('last-verlauf') ? [lauf(1793000000, 90, 310e6), lauf(1793040000, 650, 470e6)] : [],
  });
  VORSCHAU.last_messen = async () => {
    if (marken.includes('last-fehler')) throw 'networkQuality ist fehlgeschlagen: Zeitüberschreitung. Prüfe die Verbindung und versuche es noch mal.';
    return lauf(1793050200, 420, 480e6);
  };
  if (marken.some((m) => m.startsWith('last-'))) {
    setTimeout(() => document.querySelector('#tab-qualitaet')?.click(), 150);
    setTimeout(() => { const k = document.querySelector('.last-karte .knopf'); k?.click(); k?.scrollIntoView(); }, 700);
  }
})();

// Gruppe „Ausfälle“ (#51). Marken: ,ausfaelle (zwei Ausfälle, mehrere Aussetzer) ,ausfaelle-leer. Alle Werte erfunden.
(() => {
  const jetzt = () => Math.floor(Date.now() / 1000);
  VORSCHAU.ausfaelle = async () => {
    const t = jetzt();
    if (marken.includes('ausfaelle-leer')) return { liste: [], aussetzer: [t - 4 * 86400], abschnitt: '', text: 'In den letzten 7 Tagen wurde kein Ausfall gemessen.' };
    if (!marken.includes('ausfaelle')) return { liste: [], aussetzer: [], abschnitt: '', text: '' };
    return {
      liste: [
        { beginn: t - 5 * 86400 - 3600, ende: t - 5 * 86400 - 3600 + 360, dauer_s: 360, schicht: 'anbieter_oder_weiter', aussetzer: 1 },
        { beginn: t - 2 * 86400 - 7200, ende: t - 2 * 86400 - 7200 + 45, dauer_s: 45, schicht: 'router_oder_wlan', aussetzer: 0 },
      ],
      aussetzer: [t - 6 * 86400, t - 5 * 86400 - 9000, t - 3 * 86400, t - 3 * 86400 + 4000, t - 86400 * 1.5, t - 40000],
      abschnitt: '',
      text: 'Störungsmeldung (Vorschau)',
    };
  };
})();
