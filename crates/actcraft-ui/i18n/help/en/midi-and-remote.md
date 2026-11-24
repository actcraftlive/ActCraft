# MIDI, foot switches and pads

## MIDI out: clock and timecode {#midi-out}

Settings > MIDI picks a MIDI output. ActCraft can send **MIDI Clock** (tempo for drum machines
and effects) and **MIDI Timecode**. A song can also play a MIDI file of its own in time with the
audio.

## Remote actions {#remote-actions}

A remote action runs any command when a MIDI note or controller comes in: a foot switch for
Next song, Leave Loop or the next light cue.

1. Settings > MIDI: choose the MIDI input.
2. **Add remote action**, choose the command, then **Learn** and press the foot switch.

Some commands act on one thing of the show: **Recall a preset**, **Fire a hit**, **Go to a cue**,
the next cue or Stop of a cue list, starting or stopping a sequence, running a macro. For these,
**Acts on** picks the preset, hit, cue list, sequence or macro by name (and **Cue** the cue).
The action finds it by name when the note comes in, so it keeps working when the list changes.

Coming from another lighting program: if your backing-track program already sends a note per
song or part, give each note an action here with **Learn**, and the same notes go on working.

## Pads {#pads}

Pads are buttons at the bottom of the window (**Add pads…**). A pad runs a command, a macro, or
switches something on and off (a sequence, the loop). Pads have pages (Pads > Next Page).

## OSC and game controllers {#osc}

OSC messages from a tablet app run commands, macros and pads (Settings > MIDI). A game
controller's buttons run commands, and its sticks can move a volume or the lighting master.

## Macros {#macros}

A macro is a list of commands under one name (Settings > Macros), run from a pad, a remote action
or a line of the lyrics (`$NAME`).
