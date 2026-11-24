# Lights: fixtures

A fixture is one light at a DMX address. Its **type** says what each of its channels does.

## Patch a light

1. Lighting > Fixtures.
2. **Kind**: a simple kind (dimmer, RGB...), a type of your show, or one **from the library**.
3. **Mode** (for a type with several), **Universe**, the address (or first free one) and how
   many.
4. **Patch**.

The light then shows with a slider for each channel. A channel that does several things (off,
dimmer, strobe) has a list of its **ranges**: pick one, then set the level within it.

## The fixture library

ActCraft brings fixture types with every range from the maker's manual, for example the
**Betopper LM70SR** (14 and 9 channels). Choose it under Kind, "(library)".

## Fixture types of your own

- **Import fixture type…** reads ActCraft fixture files, Open Fixture Library files (.json) and
  `.r20` files.
- **New fixture type** or **Change this fixture type** opens the editor: channels, modes, and
  **Ranges** beside each channel (from, to, name).
- **Export fixture type…** saves a type as a file for another show or computer.

## The stage view

Lighting > Stage shows the fixtures from above in their colours, with the beams of moving heads.
Drag them to where they hang.
