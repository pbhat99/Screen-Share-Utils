use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationConfig {
    /// Active drawing color [r, g, b]
    pub current_color: [f32; 3],
    /// Stroke line width in pixels
    pub stroke_width: f32,
    /// Stroke opacity in range 0.05 .. 1.0 (affects future strokes and emoji stamps)
    #[serde(default = "default_one")]
    pub stroke_opacity: f32,
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
    /// Quick color presets available in the palette
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

impl Default for AnnotationConfig {
    fn default() -> Self {
        Self {
            current_color: [1.0, 1.0, 1.0], // White
            stroke_width: 6.0,
            stroke_opacity: 1.0,
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
                [0.15, 0.80, 0.95], // Cyan
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
    /// Horizontal screen anchor (0.5 = center, 0.1 = left, 0.9 = right)
    pub anchor_x: f32,
    /// Vertical screen anchor (0.88 = near bottom)
    pub anchor_y: f32,
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

    pub fn load() -> Self {
        if Path::new(Self::CONFIG_FILENAME).exists() {
            if let Ok(content) = fs::read_to_string(Self::CONFIG_FILENAME) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
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
        fs::write(Self::CONFIG_FILENAME, content)
    }
}
