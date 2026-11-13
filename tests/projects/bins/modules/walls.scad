include <../constants.scad>
include <base.scad>

// A box of x × y × h mm with walls, a floor and dividers along x.
module walls(x, y, h, dividers = 0) {
    difference() {
        rounded_block(x, y, h, corner_radius);
        translate([wall, wall, floor_thickness]) rounded_block(x - 2 * wall, y - 2 * wall, h, corner_radius - wall);
    }
    if (dividers > 0)
        for (i = [1 : dividers])
            translate([i * x / (dividers + 1) - wall / 2, 0, 0]) cube([wall, y, h]);
}
