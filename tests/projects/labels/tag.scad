// A tag with a hole to hang it by.

// The text
tag_text = "Spare";
// Length in mm
length = 40; // [25:1:80]
// Hole diameter in mm
hole = 4; // [2:0.5:8]

difference() {
    hull() {
        cube([length - 8, 16, 1.2]);
        translate([length - 8, 8, 0]) cylinder(r = 8, h = 1.2, $fn = 48);
    }
    translate([length - 8, 8, -1]) cylinder(d = hole, h = 4, $fn = 24);
}
translate([3, 8, 1.2]) linear_extrude(0.6) text(tag_text, size = 6, font = "Liberation Sans", valign = "center");
