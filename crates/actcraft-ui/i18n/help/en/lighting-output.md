# Lights: DMX output

ActCraft sends DMX over the network (Art-Net, sACN) or a USB cable.

## A USB-DMX cable

For a simple USB-RS485 cable (FTDI, "Open DMX" style):

1. Plug in the cable and the light. Note the light's DMX address (on its display).
2. Settings > Lighting > **Add output**: choose **USB cable** and the cable's port (for example
   COM5 on Windows).
3. Lighting > Fixtures: patch the light at its address (see [fixtures](fixtures)).

An Enttec DMX USB Pro or similar interface: choose **USB interface** instead.

## Art-Net and sACN

**Add output** with Art-Net or sACN sends a universe to a node on the network. **Find Art-Net nodes**
looks for Art-Net nodes.

## Checking the levels

Lighting > DMX monitor shows every channel of a universe and lets you set one by hand. Lighting >
Blackout darkens everything at once; the master in the Presets window dims all.

## DMX in

Settings > Lighting can also receive DMX over Art-Net and sACN from a desk. The DMX monitor shows
what comes in, and a channel can run a command when it goes up past half (a trigger).

**Safety:** a moving head moves as soon as it gets levels. Make sure nobody stands in its way.
