// „Qualität“: empty until a later issue fills it. Touch only this file.
Netz.registriere({
  id: 'qualitaet',
  titel: 'Qualität',
  icon: Netz.icons.qualitaet,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.qualitaet, 'Die Messungen und Berichte kommen mit der nächsten Version. In der Kommandozeile gibt es sie schon.'));
  },
});
