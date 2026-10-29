// „Tempo“: beantwortet oben „Bekomme ich, wofür ich zahle, und wo bremst es?“ (#208, #207).
// Oben die Engpass-Karte (Kern: src/engpass.rs, Vertrag im Kopf von vorschau/engpass.js) mit der Hauptaktion „Tempo messen“,
// darunter das Störungsprotokoll und die Live-Kurve, alles Übrige unter „Details“ (der Rest von „Qualität“, qualitaet.js).
// Das CSS dieser Ansicht steht am Ende von qualitaet.css: index.html bindet kein eigenes tempo.css ein.
(() => {
  const { el, status } = Netz;
  const mbit = (v) => v.toLocaleString('de-DE', { maximumFractionDigits: v < 10 ? 1 : 0 });
  const AMPEL = { keiner: 'gruen', wlan: 'orange', leitung: 'orange', router: 'orange', unklar: 'grau' };
  // Wo man am Router die Zahl der Leitung freischaltet; ohne Router (kein_router) gibt es nichts zu tun.
  const UPNP = 'Schalte am Router die Statusinformationen über UPnP ein (bei der Fritz!Box unter Heimnetz, Netzwerk, Netzwerkeinstellungen).';

  // Drei Werte, gleiche Schrift; der Engpass trägt ein Wort und einen Rahmen, nie nur Farbe.
  function zahlen(e) {
    const feld = (label, wert, klein, bremst) => el('div', { class: `zahl-feld${bremst ? ' bremst' : ''}` },
      el('dt', { class: 'label', text: label }),
      el('dd', { class: 'gross num', text: wert }),
      klein && el('dd', { class: 'klein', text: klein }),
      bremst && el('dd', { class: 'engpass-wort' }, el('span', { class: 'punkt orange' }), 'Engpass'));
    const l = e.leitung;
    const leitung = l
      ? feld('Leitung laut Router', `${mbit(l.down_mbit)} Mbit/s`, l.vertrag_down_mbit != null ? `gebucht ${mbit(l.vertrag_down_mbit)} Mbit/s` : 'Vertrag unbekannt', e.engpass === 'leitung')
      : feld('Leitung laut Router', '–', e.leitung_fehlt?.satz ?? 'Der Router nennt keine Zahl.');
    const w = e.wlan;
    const funk = e.kabel
      ? feld('WLAN', 'Kabel', 'kein WLAN dazwischen')
      : feld('WLAN', w ? `${mbit(w.nutz_mbit)} Mbit/s` : '–', w ? `Senderate ${mbit(w.rate_mbit)} Mbit/s${w.band ? `, ${w.band}` : ''}` : 'nicht ermittelt', e.engpass === 'wlan');
    const mac = feld('Am Mac', e.mac_down_mbit != null ? `${mbit(e.mac_down_mbit)} Mbit/s` : '–', e.mac_down_mbit != null ? 'Download, gemessen' : 'noch nicht gemessen');
    return el('dl', { class: 'engpass-zahlen' }, leitung, funk, mac);
  }

  // Wie es weitergeht, wenn der Router keine Zahl nennt: je `art` ein Ausweg.
  function ausweg(f) {
    if (f?.art === 'anmeldung') return el('p', { class: 'engpass-ausweg' }, el('button', { class: 'knopf', type: 'button', text: 'Zugang eintragen', onclick: () => Netz.zeige('gemerkte-netze') }));
    if (f?.art === 'upnp') return el('p', { class: 'engpass-ausweg fuss', text: UPNP });
    return '';
  }

  Netz.registriere({
    id: 'tempo',
    titel: 'Tempo',
    icon: Netz.icons.qualitaet,
    async render(wurzel) {
      const kopfStatus = el('div', { 'aria-live': 'polite' }, status('grau', 'Lädt …'));
      const zahlenOrt = el('div');
      const rat = el('p', { class: 'engpass-rat', hidden: true });
      const ort = el('div', { 'aria-live': 'polite' });
      const verlauf = el('div', { class: 'verlauf' });
      const hinweis = el('p', { class: 'last-hinweis fuss' });
      const knopf = el('button', { class: 'knopf haupt', type: 'button', text: 'Tempo messen' });
      let mac = null; // letzter Download am Mac in Mbit/s, aus dem Verlauf oder der Messung

      async function lies() {
        try {
          const e = await Netz.invoke('engpass_lesen', { macDownMbit: mac });
          kopfStatus.replaceChildren(status(AMPEL[e.engpass] ?? 'grau', e.satz));
          zahlenOrt.replaceChildren(zahlen(e), ausweg(e.leitung_fehlt));
          rat.textContent = e.rat ?? '';
          rat.hidden = !e.rat;
        } catch (f) {
          kopfStatus.replaceChildren(status('rot', 'Das Tempo ist gerade nicht lesbar'));
          zahlenOrt.replaceChildren(Netz.fehlerKarte(Netz.text(f), lies));
          rat.hidden = true;
        }
      }

      // Messung, Hotspot-Rückfrage, Fortschritt und Verlauf teilt sich die Karte mit „Unter Last“ in „Qualität“.
      Netz.qualitaet.lastSteuerung({
        knopf, ort, verlauf, hinweis, tempo: true,
        vorab: (v) => { const l = v?.verlauf?.at(-1); mac = l?.ergebnis.down_bps != null ? l.ergebnis.down_bps / 1e6 : null; lies(); },
        nach: (r) => { mac = r.down_bps != null ? r.down_bps / 1e6 : null; return lies(); },
      });

      const karte = el('section', { class: 'gruppe engpass-karte', 'aria-labelledby': 'g-engpass' },
        el('h3', { id: 'g-engpass', text: 'Wo bremst es?' }),
        el('div', { class: 'karte' }, zahlenOrt, rat,
          el('div', { class: 'engpass-aktion' }, knopf, hinweis), ort,
          el('details', { class: 'details verlauf-details' }, el('summary', { text: 'Frühere Messungen' }), verlauf),
          el('p', { class: 'fuss' }, 'Das ersetzt nicht die Breitbandmessung der Bundesnetzagentur, die für eine Minderung zählt: ',
            el('a', { href: 'https://breitbandmessung.de', target: '_blank', rel: 'noopener', text: 'breitbandmessung.de' }), '.')));

      // „Details“ zuerst bauen: das Störungsprotokoll gehört zu „Qualität“, steht aber hier oben.
      const gemeinsam = { ausfall: null };
      const detailsInhalt = el('div', { class: 'eingebettet', 'data-ansicht': 'qualitaet' });
      await Netz.qualitaet.bauen(detailsInhalt, gemeinsam);
      const live = el('div', { class: 'eingebettet', 'data-ansicht': 'live' });
      await Netz.live.bauen(live, true);

      wurzel.append(
        el('div', { class: 'kopf' }, el('h2', { text: 'Tempo' }), kopfStatus),
        karte,
        gemeinsam.ausfall ? el('div', { class: 'eingebettet', 'data-ansicht': 'qualitaet' }, gemeinsam.ausfall) : '',
        el('section', { class: 'gruppe', 'aria-labelledby': 'g-live-kurve' }, el('h3', { id: 'g-live-kurve', text: 'Live-Kurve' }), live),
        el('details', { class: 'details' }, el('summary', { text: 'Details' }), detailsInhalt));
    },
  });
})();
