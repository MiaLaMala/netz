// „Übersicht“: beantwortet oben „Ist das noch mein Netz?“ (#208). Vorverdrahtung: bettet pruefung und dieses-netz ein,
// Welle 15 baut daraus die eigentliche Ansicht.
(() => {
  Netz.registriere({
    id: 'uebersicht',
    titel: 'Übersicht',
    icon: Netz.icons.wlan,
    async render(wurzel) {
      wurzel.append(...(await Promise.all(['pruefung', 'dieses-netz'].map(Netz.einbetten))));
    },
  });
})();
