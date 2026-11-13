include <../constants.scad>

// One foot per grid square, under a bin of w × d squares.
module grid_base(w, d) {
    for (x = [0 : w - 1], y = [0 : d - 1])
        translate([x * grid_pitch + clearance / 2, y * grid_pitch + clearance / 2, 0])
            rounded_block(grid_pitch - clearance, grid_pitch - clearance, base_height, corner_radius);
}

module rounded_block(x, y, h, r) {
    hull()
        for (cx = [r, x - r], cy = [r, y - r])
            translate([cx, cy, 0]) cylinder(r = r, h = h, $fn = 24);
}
