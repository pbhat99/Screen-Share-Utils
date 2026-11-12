use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlighterConfig {
    pub enabled: bool,
    /// Radius of the highlighter circle in pixels
    pub size: f32,
    /// Softness / blur feather (0.0 = crisp edge, 1.0 = completely soft radial falloff)
    pub edge_softness: f32,
    /// RGB color [r, g, b] in range 0.0 .. 1.0
    pub color: [f32; 3],
    /// Opacity in range 0.0 .. 1.0
    pub opacity: f32,
}

impl Default for HighlighterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size: 55.0,
            edge_softness: 1.0,
            color: [1.0, 1.0, 1.0], // White
            opacity: 0.50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaserConfig {
    pub enabled: bool,
    /// RGB color of the laser pointer
    pub color: [f32; 3],
    /// Radius of the laser point
    pub radius: f32,
    /// How long the glowing laser trail persists in milliseconds before fading
    pub trail_duration_ms: u64,
}

impl Default for LaserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [1.0, 1.0, 1.0], // White
            radius: 5.0,
            trail_duration_ms: 650,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ToolBrushSettings {
    pub width: f32,
    pub opacity: f32,
    pub softness: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationConfig {
    /// Active drawing color [r, g, b]
    pub current_color: [f32; 3],
    /// Stroke line width in pixels
    pub stroke_width: f32,
    /// Stroke opacity in range 0.05 .. 1.0 (affects future strokes and emoji stamps)
    #[serde(default = "default_one")]
    pub stroke_opacity: f32,
    /// Edge softness / blurriness in range 0.0 .. 1.0 (brush, burn, dodge)
    #[serde(default)]
    pub stroke_softness: f32,
    /// Minimum stroke width in pixels (for 0% on the size slider)
    #[serde(default = "default_min_width")]
    pub min_stroke_width: f32,
    /// Maximum stroke width in pixels (for 100% on the size slider)
    #[serde(default = "default_max_width")]
    pub max_stroke_width: f32,
    /// Show floating drawing control panel on overlay
    #[serde(default = "default_true")]
    pub show_toolbar: bool,
    /// Opacity of the floating drawing control panel
    #[serde(default = "default_toolbar_opacity")]
    pub toolbar_opacity: f32,
    /// Horizontal position of the floating toolbar (client coordinates)
    #[serde(default = "default_toolbar_x")]
    pub toolbar_x: f32,
    /// Vertical position of the floating toolbar (client coordinates)
    #[serde(default = "default_toolbar_y")]
    pub toolbar_y: f32,
    /// Radius for the eraser brush (used in wipe mode)
    pub eraser_radius: f32,
    /// "object"
    pub eraser_mode: String,
    /// Last selected drawing tool
    #[serde(default = "default_selected_tool")]
    pub selected_tool: String,
    /// Currently selected emoji to stamp
    pub selected_emoji: String,
    /// List of recently used emojis
    pub recent_emojis: Vec<String>,
    /// Custom emojis (up to 9 emojis)
    #[serde(default = "default_custom_emojis")]
    pub custom_emojis: Vec<String>,
    /// Whether ellipse/round tool is drawn filled (true) or hollow (false)
    #[serde(default)]
    pub ellipse_filled: bool,
    /// Whether rectangle tool is drawn filled (true) or hollow (false)
    #[serde(default)]
    pub rect_filled: bool,
    /// Whether straight line tool is drawn as dotted line
    #[serde(default)]
    pub line_dotted: bool,
    /// Whether straight line tool is drawn as wavy line
    #[serde(default)]
    pub line_wavy: bool,
    /// Whether arrow tool is drawn as wave / sine arrow
    #[serde(default)]
    pub arrow_wavy: bool,
    /// Font family for text annotations (e.g. "Segoe UI", "Arial", "Consolas")
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Font size for text annotations in pixels
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Hyphen / dash length for dashed/dotted lines (0.0 = round dots, > 0.2 = dashes)
    #[serde(default = "default_dash_length")]
    pub dash_length: f32,
    /// Style for emoji stamps: "flat" (single-color, default) or "color" (multicolor)
    #[serde(default = "default_emoji_style")]
    pub emoji_style: String,
    /// Whether flat emojis are drawn solid filled (true, default) or hollow outline (false)
    #[serde(default = "default_true")]
    pub emoji_filled: bool,
    /// Whether drawing uses real-time screen Invert mode instead of a flat color
    #[serde(default)]
    pub invert_color: bool,
    /// Last selected geometric shape in the combined Shapes button ("line", "arrow", "rect", "ellipse")
    #[serde(default = "default_last_shape_tool")]
    pub last_shape_tool: String,
    /// Last selected Burn/Dodge brush mode ("burn" or "dodge")
    #[serde(default = "default_last_burn_dodge")]
    pub last_burn_dodge: String,
    /// Last selected Pen/Text toggle mode ("freehand" or "text")
    #[serde(default = "default_last_pen_text")]
    pub last_pen_text: String,
    /// Last selected Eraser/Transform toggle mode ("eraser" or "transform")
    #[serde(default = "default_last_eraser_transform")]
    pub last_eraser_transform: String,
    /// Sync Size, Opacity, and Softness across all drawing tools (false = per-tool independent settings)
    #[serde(default)]
    pub sync_tool_settings: bool,
    /// Stored per-tool settings (Size, Opacity, Softness) keyed by tool string
    #[serde(default)]
    pub per_tool_settings: HashMap<String, ToolBrushSettings>,
    /// Quick color presets available in the palette (8 outer swatches surrounding center Invert)
    pub color_presets: Vec<[f32; 3]>,
}

fn default_custom_emojis() -> Vec<String> {
    vec![
        "⭐".into(),
        "❤️".into(),
        "✔️".into(),
        "❌".into(),
        "💡".into(),
        "🔥".into(),
        "🚀".into(),
        "👍".into(),
        "🎯".into(),
    ]
}

fn default_font_family() -> String {
    "Segoe UI".to_string()
}
fn default_font_size() -> f32 {
    28.0
}

fn default_one() -> f32 {
    1.0
}
fn default_min_width() -> f32 {
    2.0
}
fn default_max_width() -> f32 {
    50.0
}
fn default_toolbar_opacity() -> f32 {
    0.80
}
fn default_toolbar_x() -> f32 {
    30.0
}
fn default_toolbar_y() -> f32 {
    100.0
}
fn default_selected_tool() -> String {
    "freehand".to_string()
}
fn default_dash_length() -> f32 {
    3.0
}
fn default_emoji_style() -> String {
    "flat".to_string()
}
fn default_last_shape_tool() -> String {
    "rect".to_string()
}
fn default_last_burn_dodge() -> String {
    "burn".to_string()
}
fn default_last_pen_text() -> String {
    "freehand".to_string()
}
fn default_last_eraser_transform() -> String {
    "eraser".to_string()
}

impl Default for AnnotationConfig {
    fn default() -> Self {
        Self {
            current_color: [1.0, 1.0, 1.0], // White
            stroke_width: 6.0,
            stroke_opacity: 1.0,
            stroke_softness: 0.0,
            min_stroke_width: 2.0,
            max_stroke_width: 50.0,
            show_toolbar: true,
            toolbar_opacity: 0.80,
            toolbar_x: 30.0,
            toolbar_y: 100.0,
            eraser_radius: 28.0,
            eraser_mode: "object".to_string(),
            selected_tool: "freehand".to_string(),
            selected_emoji: "⭐".to_string(),
            custom_emojis: default_custom_emojis(),
            ellipse_filled: true,
            rect_filled: true,
            line_dotted: false,
            line_wavy: false,
            arrow_wavy: false,
            font_family: "Segoe UI".to_string(),
            font_size: 28.0,
            dash_length: 3.0,
            emoji_style: "flat".to_string(),
            emoji_filled: true,
            invert_color: false,
            last_shape_tool: "rect".to_string(),
            last_burn_dodge: "burn".to_string(),
            last_pen_text: "freehand".to_string(),
            last_eraser_transform: "eraser".to_string(),
            sync_tool_settings: false,
            per_tool_settings: HashMap::new(),
            recent_emojis: vec![
                "⭐".into(),
                "❤️".into(),
                "✔️".into(),
                "❌".into(),
                "💡".into(),
                "🔥".into(),
                "🚀".into(),
                "👍".into(),
                "🎯".into(),
                "❓".into(),
                "📌".into(),
                "💯".into(),
            ],
            color_presets: vec![
                [0.95, 0.25, 0.25], // Red
                [1.00, 0.55, 0.10], // Orange
                [1.00, 0.88, 0.15], // Yellow
                [0.22, 0.85, 0.35], // Green
                [0.25, 0.55, 1.00], // Blue
                [0.70, 0.35, 0.95], // Purple
                [1.00, 1.00, 1.00], // White
                [0.12, 0.12, 0.14], // Dark
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsdConfig {
    pub enabled: bool,
    /// Font size for keystroke display
    pub font_size: f32,
    /// Background color [r, g, b, a]
    pub bg_color: [f32; 4],
    /// Foreground text color [r, g, b, a]
    pub fg_color: [f32; 4],
    /// How long the keystroke badge stays visible (in milliseconds)
    pub stay_duration_ms: u64,
    /// Fade-out animation duration (in milliseconds)
    pub fade_duration_ms: u64,
    /// Corner roundness radius
    pub border_radius: f32,
    /// Optional background image path (supports .png, .jpg)
    pub bg_image_path: Option<String>,
    /// Horizontal screen anchor (legacy fallback)
    pub anchor_x: f32,
    /// Vertical screen anchor (legacy fallback)
    pub anchor_y: f32,
    /// Screen alignment slot ("top_left", "top_center", "top_right",
    /// "middle_left", "center", "middle_right", "bottom_left", "bottom_center", "bottom_right")
    #[serde(default = "default_osd_position")]
    pub position: String,
    /// Margin in pixels from screen edges
    #[serde(default = "default_osd_edge_offset")]
    pub edge_offset: f32,
    /// Target display ("active" = cursor monitor, "primary", or "monitor_0", "monitor_1", ...)
    #[serde(default = "default_osd_monitor")]
    pub monitor: String,
}

fn default_osd_position() -> String {
    "bottom_center".to_string()
}
fn default_osd_edge_offset() -> f32 {
    36.0
}
fn default_osd_monitor() -> String {
    "active".to_string()
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            font_size: 26.0,
            bg_color: [0.08, 0.08, 0.10, 0.88], // Sleek modern translucent dark
            fg_color: [0.98, 0.98, 1.00, 1.00], // Crisp white
            stay_duration_ms: 1800,
            fade_duration_ms: 350,
            border_radius: 14.0,
            bg_image_path: None,
            anchor_x: 0.50,
            anchor_y: 0.88,
            position: default_osd_position(),
            edge_offset: default_osd_edge_offset(),
            monitor: default_osd_monitor(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub highlighter: HighlighterConfig,
    pub laser: LaserConfig,
    pub annotation: AnnotationConfig,
    pub osd: OsdConfig,
    /// Synchronize color across highlighter, laser, and drawing modes
    #[serde(default = "default_true")]
    pub sync_colors: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            highlighter: HighlighterConfig::default(),
            laser: LaserConfig::default(),
            annotation: AnnotationConfig::default(),
            osd: OsdConfig::default(),
            sync_colors: true,
        }
    }
}

impl AppConfig {
    pub const CONFIG_FILENAME: &'static str = "screen_tools_config.json";

    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join(Self::CONFIG_FILENAME)))
            .unwrap_or_else(|| PathBuf::from(Self::CONFIG_FILENAME))
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                    // Migrate legacy 9-slot preset arrays cleanly by removing middle (5th) slot
                    // now reserved for fixed center Invert in the 3x3 color flyout.
                    if config.annotation.color_presets.len() == 9 {
                        config.annotation.color_presets.remove(4);
                    } else if config.annotation.color_presets.is_empty() {
                        config.annotation.color_presets = AnnotationConfig::default().color_presets;
                    }
                    return config;
                }
            }
        }
        let default_config = AppConfig::default();
        let _ = default_config.save();
        default_config
    }

    pub fn save(&self) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(Self::config_path(), content)
    }
}

impl AnnotationConfig {
    pub fn is_drawable_tool_key(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "freehand" | "burn" | "dodge" | "line" | "arrow" | "rect" | "ellipse" | "emoji" | "text"
        )
    }

    pub fn default_settings_for_tool(&self, tool_name: &str) -> ToolBrushSettings {
        let min_w = self.min_stroke_width.max(0.5);
        let max_w = self.max_stroke_width.max(min_w + 1.0);
        let base_w = match tool_name {
            "burn" | "dodge" => 24.0f32.clamp(min_w, max_w),
            _ => 6.0f32.clamp(min_w, max_w),
        };
        let base_softness = match tool_name {
            "burn" | "dodge" => 0.35,
            _ => 0.0,
        };
        ToolBrushSettings {
            width: base_w,
            opacity: 1.0,
            softness: base_softness,
        }
    }

    /// Load active tool's (width, opacity, softness) into active fields when per-tool sync is off
    pub fn load_tool_settings(&mut self, tool_name: &str) {
        if self.sync_tool_settings || !Self::is_drawable_tool_key(tool_name) {
            return;
        }
        let settings = self
            .per_tool_settings
            .get(tool_name)
            .copied()
            .unwrap_or_else(|| self.default_settings_for_tool(tool_name));
        let min_w = self.min_stroke_width.max(0.5);
        let max_w = self.max_stroke_width.max(min_w + 1.0);
        self.stroke_width = settings.width.clamp(min_w, max_w);
        self.stroke_opacity = settings.opacity.clamp(0.05, 1.0);
        self.stroke_softness = settings.softness.clamp(0.0, 1.0);
    }

    /// Persist active (width, opacity, softness) into per_tool_settings map when sync is off
    pub fn save_tool_settings(&mut self, tool_name: &str) {
        if self.sync_tool_settings || !Self::is_drawable_tool_key(tool_name) {
            return;
        }
        self.per_tool_settings.insert(
            tool_name.to_string(),
            ToolBrushSettings {
                width: self.stroke_width,
                opacity: self.stroke_opacity.clamp(0.05, 1.0),
                softness: self.stroke_softness.clamp(0.0, 1.0),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_tool_settings_independent_and_synced() {
        let mut cfg = AnnotationConfig::default();
        assert!(!cfg.sync_tool_settings);

        // Save custom size/opacity/softness for freehand
        cfg.stroke_width = 12.0;
        cfg.stroke_opacity = 0.65;
        cfg.stroke_softness = 0.5;
        cfg.save_tool_settings("freehand");

        // Switch to burn and customize independently
        cfg.load_tool_settings("burn");
        assert!((cfg.stroke_width - 24.0).abs() < 0.01);
        cfg.stroke_width = 36.0;
        cfg.stroke_opacity = 0.40;
        cfg.stroke_softness = 0.80;
        cfg.save_tool_settings("burn");

        // Switch back to freehand -> restores freehand values
        cfg.load_tool_settings("freehand");
        assert!((cfg.stroke_width - 12.0).abs() < 0.01);
        assert!((cfg.stroke_opacity - 0.65).abs() < 0.01);
        assert!((cfg.stroke_softness - 0.50).abs() < 0.01);

        // Enable sync -> switching tools keeps active shared settings intact
        cfg.sync_tool_settings = true;
        cfg.stroke_width = 18.0;
        cfg.load_tool_settings("burn");
        assert!((cfg.stroke_width - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_osd_config_defaults_and_legacy_deserialization() {
        let cfg = OsdConfig::default();
        assert_eq!(cfg.position, "bottom_center");
        assert_eq!(cfg.edge_offset, 36.0);
        assert_eq!(cfg.monitor, "active");

        let legacy_json = r#"{
            "enabled": true,
            "font_size": 24.0,
            "bg_color": [0.1, 0.2, 0.3, 0.75],
            "fg_color": [1.0, 1.0, 1.0, 1.0],
            "stay_duration_ms": 1500,
            "fade_duration_ms": 300,
            "border_radius": 10.0,
            "bg_image_path": null,
            "anchor_x": 0.5,
            "anchor_y": 0.9
        }"#;
        let parsed: OsdConfig = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed.position, "bottom_center");
        assert_eq!(parsed.edge_offset, 36.0);
        assert_eq!(parsed.monitor, "active");
        assert!((parsed.bg_color[3] - 0.75).abs() < 0.001);
    }
}
