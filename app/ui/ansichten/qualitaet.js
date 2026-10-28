// „Qualität“: leer, bis sie mit Issue #19 gefüllt wird. Nur diese Datei anfassen.
Netz.registriere({
  id: 'qualitaet',
  titel: 'Qualität',
  icon: Netz.icons.qualitaet,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.qualitaet, 'Die Messungen und Berichte kommen mit Issue #19.'));
  },
});
