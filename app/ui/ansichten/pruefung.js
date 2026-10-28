// „Prüfung“: empty until a later issue fills it. Touch only this file.
Netz.registriere({
  id: 'pruefung',
  titel: 'Prüfung',
  icon: Netz.icons.pruefung,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.pruefung, 'Der Vergleich mit dem letzten Besuch kommt mit der nächsten Version. In der Kommandozeile gibt es das schon.'));
  },
});
