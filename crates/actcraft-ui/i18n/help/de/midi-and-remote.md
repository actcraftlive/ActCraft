# MIDI, Fußschalter und Pads

## MIDI raus: Clock und Timecode {#midi-out}

Einstellungen > MIDI wählt einen MIDI-Ausgang. ActCraft kann **MIDI Clock** (Tempo für
Drumcomputer und Effekte) und **MIDI Timecode** senden. Ein Song kann auch eine eigene MIDI-Datei
im Takt mit dem Audio abspielen.

## Fernaktionen {#remote-actions}

Eine Fernaktion führt einen Befehl aus, wenn eine MIDI-Note oder ein Controller ankommt: ein
Fußschalter für Nächster Song, Schleife verlassen oder den nächsten Licht-Cue.

1. Einstellungen > MIDI: den MIDI-Eingang wählen.
2. **Fernaktion hinzufügen**, den Befehl wählen, dann **Lernen** und den Fußschalter drücken.

Manche Befehle wirken auf ein Ding der Show: **Ein Preset abrufen**, **Stoß auslösen**, **Zu einem Cue
gehen**, der nächste Cue oder Stopp einer Cue-Liste, einen Ablauf starten oder stoppen, ein Makro
ausführen. Bei diesen wählt **Wirkt auf** das Preset, den Stoß, die Cue-Liste, den Ablauf oder
das Makro nach Namen (und **Cue** den Cue). Die Aktion sucht es beim Eintreffen der Note nach
Namen, sie funktioniert also weiter, wenn sich die Liste ändert.

Umstieg von einem anderen Lichtprogramm: Schickt dein Backing-Track-Programm schon eine Note pro
Song oder Teil, gib hier jeder Note mit **Lernen** eine Aktion, und dieselben Noten funktionieren
weiter.

## Pads {#pads}

Pads sind Knöpfe unten im Fenster (**Pads hinzufügen …**). Ein Pad führt einen Befehl oder ein
Makro aus oder schaltet etwas ein und aus (einen Ablauf, die Schleife). Pads haben Seiten
(Pads > Nächste Seite).

## OSC und Gamecontroller {#osc}

OSC-Nachrichten aus einer Tablet-App führen Befehle, Makros und Pads aus (Einstellungen > MIDI).
Die Knöpfe eines Gamecontrollers führen Befehle aus, seine Sticks können eine Lautstärke oder den
Licht-Master bewegen.

## Makros {#macros}

Ein Makro ist eine Liste von Befehlen unter einem Namen (Einstellungen > Makros), ausgelöst von
einem Pad, einer Fernaktion oder einer Zeile im Songtext (`$NAME`).
