include <../constants.scad>
include <base.scad>

// The stacking lip on top of a bin's walls.
module lip(x, y) {
    difference() {
        rounded_block(x, y, lip_height, corner_radius);
        translate([wall * 2, wall * 2, -1]) rounded_block(x - 4 * wall, y - 4 * wall, lip_height + 2, corner_radius - 2 * wall);
    }
}
