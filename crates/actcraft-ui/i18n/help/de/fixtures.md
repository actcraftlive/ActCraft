# Licht: Geräte

Ein Gerät ist eine Lampe an einer DMX-Adresse. Ihr **Typ** sagt, was jeder ihrer Kanäle tut.

## Eine Lampe patchen

![Das Fenster Geräte mit einem Moving Head an 1.001](fixtures.png)

1. Licht > Geräte.
2. **Art**: eine einfache Art (Dimmer, RGB …), ein Typ deiner Show oder einer **aus der
   Bibliothek**.
3. **Modus** (bei einem Typ mit mehreren), **Universum**, die Adresse (oder die erste freie) und
   wie viele.
4. **Patchen**.

Die Lampe erscheint dann mit einem Regler pro Kanal. Ein Kanal, der mehreres kann (aus, Dimmer,
Strobe), hat eine Liste seiner **Bereiche**: einen wählen, dann den Wert darin einstellen.

## Die Gerätebibliothek

ActCraft bringt Gerätetypen mit allen Bereichen aus der Anleitung des Herstellers mit, zum
Beispiel den **Betopper LM70SR** (14 und 9 Kanäle). Unter Art „(Bibliothek)“ wählen.

## Eigene Gerätetypen

- **Gerätetyp importieren …** liest ActCraft-Gerätedateien, Dateien der Open Fixture Library
  (.json) und `.r20`-Dateien.
- **Neuer Gerätetyp** oder **Diesen Gerätetyp ändern** öffnet den Editor: Kanäle, Modi und
  **Bereiche** neben jedem Kanal (von, bis, Name).
- **Gerätetyp exportieren …** speichert einen Typ als Datei für eine andere Show oder einen
  anderen Rechner.

## Die Bühnenansicht

Licht > Bühne zeigt die Geräte von oben in ihren Farben, mit den Strahlen der Moving Heads. Zieh
sie dorthin, wo sie hängen.
