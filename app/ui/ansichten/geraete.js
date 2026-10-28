// „Geräte“: leer, bis sie mit Issue #19 gefüllt wird. Nur diese Datei anfassen.
Netz.registriere({
  id: 'geraete',
  titel: 'Geräte',
  icon: Netz.icons.geraete,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.geraete, 'Die Gerätesuche kommt mit Issue #19.'));
  },
});
