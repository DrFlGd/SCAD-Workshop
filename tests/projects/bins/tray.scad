// A shallow tray: one compartment per grid square.
include <modules/all.scad>

// Width in grid units
units_wide = 2; // [1:1:6]
// Depth in grid units
units_deep = 2; // [1:1:6]
// Wall height in mm
wall_height = 14; // [6:1:40]

x = units_wide * grid_pitch - clearance;
y = units_deep * grid_pitch - clearance;

grid_base(units_wide, units_deep);
translate([clearance / 2, clearance / 2, base_height]) walls(x, y, wall_height, units_wide - 1);
