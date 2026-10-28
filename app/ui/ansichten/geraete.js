// „Geräte“: empty until a later issue fills it. Touch only this file.
Netz.registriere({
  id: 'geraete',
  titel: 'Geräte',
  icon: Netz.icons.geraete,
  render(wurzel) {
    wurzel.append(Netz.leerZustand(Netz.icons.geraete, 'Die Gerätesuche kommt mit der nächsten Version. In der Kommandozeile gibt es das schon.'));
  },
});
