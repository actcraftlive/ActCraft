# MIDI, Fußschalter und Pads

## MIDI raus: Clock und Timecode

Einstellungen > MIDI wählt einen MIDI-Ausgang. ActCraft kann **MIDI Clock** (Tempo für
Drumcomputer und Effekte) und **MIDI Timecode** senden. Ein Song kann auch eine eigene MIDI-Datei
im Takt mit dem Audio abspielen.

## Fernaktionen

Eine Fernaktion führt einen Befehl aus, wenn eine MIDI-Note oder ein Controller ankommt: ein
Fußschalter für Nächster Song, Schleife verlassen oder den nächsten Licht-Cue.

1. Einstellungen > MIDI: den MIDI-Eingang wählen.
2. **Fernaktion hinzufügen**, den Befehl wählen, dann **Lernen** und den Fußschalter drücken.

## Pads

Pads sind Knöpfe unten im Fenster (**Pads hinzufügen …**). Ein Pad führt einen Befehl oder ein
Makro aus oder schaltet etwas ein und aus (einen Ablauf, die Schleife). Pads haben Seiten
(Pads > Nächste Seite).

## OSC und Gamecontroller

OSC-Nachrichten aus einer Tablet-App führen Befehle, Makros und Pads aus (Einstellungen > MIDI).
Die Knöpfe eines Gamecontrollers führen Befehle aus, seine Sticks können eine Lautstärke oder den
Licht-Master bewegen.

## Makros

Ein Makro ist eine Liste von Befehlen unter einem Namen (Einstellungen > Makros), ausgelöst von
einem Pad, einer Fernaktion oder einer Zeile im Songtext (`$NAME`).
