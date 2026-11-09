/// Human-readable color naming algorithm using weighted perceptual RGB distance

pub struct NamedColor {
    pub name: &'static str,
    pub emoji: &'static str,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

pub const COLOR_TABLE: &[NamedColor] = &[
    NamedColor { name: "Red", emoji: "🔴", r: 0.95, g: 0.22, b: 0.22 },
    NamedColor { name: "Crimson", emoji: "🔴", r: 0.86, g: 0.08, b: 0.24 },
    NamedColor { name: "Ruby", emoji: "🔴", r: 0.70, g: 0.10, b: 0.20 },
    NamedColor { name: "Coral", emoji: "🟠", r: 1.00, g: 0.48, b: 0.32 },
    NamedColor { name: "Orange", emoji: "🟠", r: 1.00, g: 0.55, b: 0.00 },
    NamedColor { name: "Amber", emoji: "🟠", r: 1.00, g: 0.75, b: 0.00 },
    NamedColor { name: "Gold", emoji: "🟡", r: 1.00, g: 0.84, b: 0.00 },
    NamedColor { name: "Yellow", emoji: "🟡", r: 1.00, g: 0.92, b: 0.15 },
    NamedColor { name: "Lime", emoji: "🟢", r: 0.65, g: 0.90, b: 0.15 },
    NamedColor { name: "Neon Green", emoji: "🟢", r: 0.22, g: 0.95, b: 0.35 },
    NamedColor { name: "Green", emoji: "🟢", r: 0.15, g: 0.80, b: 0.30 },
    NamedColor { name: "Emerald", emoji: "🟢", r: 0.10, g: 0.75, b: 0.45 },
    NamedColor { name: "Mint", emoji: "🟢", r: 0.35, g: 0.90, b: 0.65 },
    NamedColor { name: "Teal", emoji: "🌐", r: 0.10, g: 0.70, b: 0.70 },
    NamedColor { name: "Cyan", emoji: "🌐", r: 0.15, g: 0.85, b: 0.95 },
    NamedColor { name: "Sky Blue", emoji: "🌐", r: 0.25, g: 0.75, b: 1.00 },
    NamedColor { name: "Blue", emoji: "🔵", r: 0.20, g: 0.50, b: 0.98 },
    NamedColor { name: "Indigo", emoji: "🔵", r: 0.25, g: 0.25, b: 0.85 },
    NamedColor { name: "Violet", emoji: "🟣", r: 0.55, g: 0.25, b: 0.90 },
    NamedColor { name: "Purple", emoji: "🟣", r: 0.68, g: 0.30, b: 0.92 },
    NamedColor { name: "Magenta", emoji: "🟣", r: 0.90, g: 0.20, b: 0.85 },
    NamedColor { name: "Pink", emoji: "🌸", r: 1.00, g: 0.40, b: 0.70 },
    NamedColor { name: "Rose", emoji: "🌸", r: 1.00, g: 0.60, b: 0.75 },
    NamedColor { name: "White", emoji: "⚪", r: 1.00, g: 1.00, b: 1.00 },
    NamedColor { name: "Silver", emoji: "⚪", r: 0.78, g: 0.78, b: 0.80 },
    NamedColor { name: "Gray", emoji: "⚫", r: 0.50, g: 0.50, b: 0.52 },
    NamedColor { name: "Dark", emoji: "⚫", r: 0.18, g: 0.18, b: 0.20 },
    NamedColor { name: "Black", emoji: "⚫", r: 0.05, g: 0.05, b: 0.06 },
    NamedColor { name: "Brown", emoji: "🟤", r: 0.55, g: 0.35, b: 0.20 },
];

/// Returns the closest named color landmark using perceptual weighted RGB distance
pub fn find_nearest_named_color(c: [f32; 3]) -> &'static NamedColor {
    let mut best_entry = &COLOR_TABLE[0];
    let mut min_dist = f32::MAX;

    for entry in COLOR_TABLE {
        let dr = c[0] - entry.r;
        let dg = c[1] - entry.g;
        let db = c[2] - entry.b;
        // Standard perceptual RGB weighting (human eyes are most sensitive to green, then red, then blue)
        let dist = 2.0 * dr * dr + 4.0 * dg * dg + 3.0 * db * db;
        if dist < min_dist {
            min_dist = dist;
            best_entry = entry;
        }
    }

    best_entry
}

/// Formats a human-readable display string with emoji: "🔴 Red", "🌐 Sky Blue", etc.
pub fn get_color_display_name(c: [f32; 3]) -> String {
    let entry = find_nearest_named_color(c);
    format!("{} {}", entry.emoji, entry.name)
}

/// Formats a hex color string: "#RRGGBB"
pub fn get_hex_string(c: [f32; 3]) -> String {
    let r = (c[0] * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (c[1] * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (c[2] * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_naming() {
        let red = [0.95, 0.25, 0.25];
        assert_eq!(get_color_display_name(red), "🔴 Red");

        let orange = [1.00, 0.55, 0.10];
        assert_eq!(get_color_display_name(orange), "🟠 Orange");

        let green = [0.22, 0.85, 0.35];
        assert_eq!(get_color_display_name(green), "🟢 Green");

        let white = [1.00, 1.00, 1.00];
        assert_eq!(get_color_display_name(white), "⚪ White");
        assert_eq!(get_hex_string(white), "#FFFFFF");

        let dark = [0.12, 0.12, 0.14];
        assert_eq!(get_color_display_name(dark), "⚫ Dark");
    }
}

