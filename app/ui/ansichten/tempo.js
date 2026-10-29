// „Tempo“: beantwortet oben „Bekomme ich, wofür ich zahle, und wo bremst es?“ (#208). Vorverdrahtung: bettet qualitaet und live ein,
// Welle 15 baut daraus die eigentliche Ansicht.
(() => {
  Netz.registriere({
    id: 'tempo',
    titel: 'Tempo',
    icon: Netz.icons.qualitaet,
    async render(wurzel) {
      wurzel.append(...(await Promise.all(['qualitaet', 'live'].map(Netz.einbetten))));
    },
  });
})();
