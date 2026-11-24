# Licht: DMX-Ausgang

ActCraft sendet DMX über das Netzwerk (Art-Net, sACN) oder ein USB-Kabel.

## Ein USB-DMX-Kabel

Für ein einfaches USB-RS485-Kabel (FTDI, „Open DMX“):

1. Kabel und Lampe anschließen. Die DMX-Adresse der Lampe merken (steht auf ihrem Display).
2. Einstellungen > Licht > **Ausgang hinzufügen**: **USB-Kabel** wählen und den Anschluss des
   Kabels (unter Windows zum Beispiel COM5).
3. Licht > Geräte: die Lampe an ihrer Adresse patchen (siehe [Geräte](fixtures)).

Für ein Enttec DMX USB Pro oder ein ähnliches Interface stattdessen **USB-Interface** wählen.

## Art-Net und sACN

![Einstellungen, Reiter Licht: DMX-Ausgänge und DMX-Eingang](lighting-settings.png)

**Ausgang hinzufügen** mit Art-Net oder sACN schickt ein Universum an einen Knoten im Netz.
**Art-Net-Knoten suchen** sucht nach Art-Net-Knoten.

## Pegel prüfen

Licht > DMX-Monitor zeigt alle Kanäle eines Universums und setzt einen von Hand. Licht >
Blackout macht alles auf einmal dunkel; der Master im Fenster Presets dimmt alles.

## DMX-Eingang

Einstellungen > Licht kann auch DMX über Art-Net und sACN von einem Pult empfangen. Der
DMX-Monitor zeigt, was ankommt, und ein Kanal kann einen Befehl auslösen, wenn er über die Hälfte
steigt (ein Auslöser).

**Sicherheit:** Ein Moving Head bewegt sich, sobald er Pegel bekommt. Achte darauf, dass niemand
im Weg steht.
