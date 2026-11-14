// A bin on a grid: its feet, walls, dividers and a stacking lip.
include <modules/all.scad>

/* [General] */
// Width: grid units, or mm when the second number isn't 0
width = [2, 0]; //0.1
// Depth: grid units, or mm when the second number isn't 0
depth = [1, 0]; //0.1
// Height: 7 mm units, or mm when the second number isn't 0
height = [3, 0]; //0.1
// Dividers along the width
dividers = 1; // [0:1:5]

/* [Lip] */
// A lip so bins stack
stacking_lip = true;

/* [Model detail] */
$fa = 8;
$fs = 0.5;

function size(v, unit) = v[1] > 0 ? v[1] : v[0] * unit;

x = size(width, grid_pitch) - clearance;
y = size(depth, grid_pitch) - clearance;
h = size(height, height_unit);

grid_base(max(1, round(width[0])), max(1, round(depth[0])));
translate([clearance / 2, clearance / 2, base_height]) {
    walls(x, y, h, dividers);
    if (stacking_lip) translate([0, 0, h]) lip(x, y);
}
