# Test bins

Three models on a 42 mm grid, written for SCAD Workshop's tests:

- **Bin**: feet, walls, dividers and a stacking lip. Its width, depth and height are pairs (grid units, or millimetres when the second number isn't 0).
- **Tray**: one compartment per grid square.
- **Lid**: its sizes have fractions as defaults and no Customizer comment.

`constants.scad` is included by every file in `modules/`, and the models include all of them, so it is included several times in one render.

Public domain (CC0 1.0).
