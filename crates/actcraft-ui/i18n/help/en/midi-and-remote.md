# MIDI, foot switches and pads

## MIDI out: clock and timecode

Settings > MIDI picks a MIDI output. ActCraft can send **MIDI Clock** (tempo for drum machines
and effects) and **MIDI Timecode**. A song can also play a MIDI file of its own in time with the
audio.

## Remote actions

A remote action runs any command when a MIDI note or controller comes in: a foot switch for
Next song, Leave Loop or the next light cue.

1. Settings > MIDI: choose the MIDI input.
2. **Add remote action**, choose the command, then **Learn** and press the foot switch.

## Pads

Pads are buttons at the bottom of the window (**Add pads…**). A pad runs a command, a macro, or
switches something on and off (a sequence, the loop). Pads have pages (Pads > Next Page).

## OSC and game controllers

OSC messages from a tablet app run commands, macros and pads (Settings > MIDI). A game
controller's buttons run commands, and its sticks can move a volume or the lighting master.

## Macros

A macro is a list of commands under one name (Settings > Macros), run from a pad, a remote action
or a line of the lyrics (`$NAME`).
