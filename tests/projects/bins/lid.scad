// A lid that sits on a bin. The sizes have no Customizer comment and a default
// that is a fraction: the app accepts them at their defaults.
include <constants.scad>

// Width in grid units
lid_width = 1.5;
// Depth in grid units
lid_depth = 2.5;
// Thickness in mm
thickness = 2; // [1:0.5:5]
// Put a finger notch on the front edge
notch = false;

x = lid_width * grid_pitch - clearance;
y = lid_depth * grid_pitch - clearance;
difference() {
    cube([x, y, thickness]);
    if (notch) translate([x / 2, 0, -1]) cylinder(r = 8, h = thickness + 2, $fn = 32);
}
