// „Prüfung“: leer, bis sie mit Issue #19 gefüllt wird. Nur diese Datei anfassen.
Netz.registriere({
  id: 'pruefung',
  titel: 'Prüfung',
  icon: Netz.icons.pruefung,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.pruefung, 'Der Vergleich mit dem letzten Besuch kommt mit Issue #19.'));
  },
});
