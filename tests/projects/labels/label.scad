// A flat label with raised text.

// The text
label_text = "Screws M3";
// Width in mm
label_width = 36; // [20:1:80]
// Height in mm
label_height = 11; // [8:1:20]
// Text size in mm
text_size = 5; // [3:0.5:10]
// Font
font = "Liberation Sans:style=Bold"; // [Liberation Sans:style=Bold, Liberation Sans, Liberation Mono]

cube([label_width, label_height, 0.8]);
translate([label_width / 2, label_height / 2, 0.8])
    linear_extrude(0.6) text(label_text, size = text_size, font = font, halign = "center", valign = "center");
