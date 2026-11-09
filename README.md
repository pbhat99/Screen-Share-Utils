# ScreenTools 🚀

An ultra-lightweight, high-performance desktop presentation and annotation tool built with native **Rust**, **Win32**, **Direct2D 1.1**, and **DirectComposition**. Designed for tutorials, live webinars, code reviews, presentations, and screen recording with **zero input lag**, **0.0% idle CPU**, and an **8MB RAM footprint**.

Single standalone binary: **811 KB** (download from [GitHub Releases](https://github.com/pbhat99/Screen-Share-Utils/releases)).

---

## 🔔 System Tray Menu (Left-Click & Right-Click)

Clicking the ScreenTools tray icon in the Windows taskbar with **either Left-Click or Right-Click** opens the full control menu:

- ✏️ **Writing Mode (`F8`)** — Toggle drawing mode on/off. When turned OFF, your drawings **remain visible on screen**, and your mouse clicks pass straight through to desktop apps!
- 🛠️ **Tools Sub-menu:**
  - ✏️ **Pen / Freehand** — Smooth anti-aliased vector drawing.
  - 📏 **Straight Line** — Pixel-perfect lines.
  - ➡️ **Arrow** — Crisp directional arrows.
  - 🔲 **Rectangle / Box** — Rectangular callouts.
  - ⭕ **Circle / Ellipse** — Highlights.
  - ⚡ **Fading Laser Pointer (`F11`)** — Glowing laser trail that disappears after ~0.6s.
  - 😀 **Emoji Stamp** — DirectWrite native color emojis (`⭐`, `❤️`, `✔️`, `❌`, `💡`, `🔥`, `🚀`, `👍`, `🎯`, `❓`, `📌`, `💯`).
- 🎨 **Color Palette Sub-menu:**
  - 🔴 Red, 🟠 Orange, 🟡 Yellow, 🟢 Green, 🌐 Cyan, 🔵 Blue, 🟣 Purple, ⚪ White, ⚫ Dark.
- 🧹 **Eraser Sub-menu:**
  - 🎯 **Object Eraser** — Click any stroke to delete the entire shape.
  - 🖌️ **Wipe Eraser** — Drag across strokes to erase in an area.
- 🗑️ **Clear Drawings (`Ctrl+Alt+C`)** — Instant screen wipe.
- ↩️ **Undo (`Ctrl+Z`)** — Undo last stroke.
- 💡 **Mouse Highlighter (`F9`)** — 100% click-through cursor halo spotlight.
- ⌨️ **Keystroke OSD (`F10`)** — Real-time chord & shortcut display.
- ⚙️ **Settings** — Opens [`screen_tools_config.json`](file:///f:/Tools%20dev/screenTools/screen_tools_config.json) in Notepad.
- 🚪 **Exit** — Closes ScreenTools.

---

## ⌨️ Global Shortcuts

| Shortcut | Action |
| :--- | :--- |
| **`F8`** | Toggle Writing Mode |
| **`F9`** | Toggle Cursor Highlighter |
| **`F10`** | Toggle Keystroke OSD |
| **`F11`** | Toggle Laser Pointer Mode |
| **`Ctrl + Alt + C`** | Clear all screen annotations |
| **`Ctrl + Z`** | Undo last stroke |
| **`Ctrl + Y`** | Redo stroke |
| **`ESC` / Right-Click** | Exit drawing mode back to click-through (drawings remain on screen) |

---

## 💡 100% Click-Through Cursor Highlighter

- Built with native Windows **`WS_EX_TRANSPARENT`** pass-through.
- You can freely click desktop icons, select text in your editor, browse websites, and drag windows normally while the highlighter smoothly tracks the cursor with zero interference or lag.
- **Customizable:** Size, Edge Softness ($0.0$ to $1.0$), RGBA colors, and animated click pulse ripples.

---

## ⚙️ Configuration (`screen_tools_config.json`)

```json
{
  "highlighter": {
    "enabled": true,
    "size": 38.0,
    "edge_softness": 0.35,
    "color": [1.0, 0.9, 0.15],
    "opacity": 0.45,
    "click_ripple": true,
    "ripple_color": [1.0, 0.4, 0.2]
  },
  "laser": {
    "enabled": false,
    "color": [1.0, 0.2, 0.2],
    "radius": 7.0,
    "trail_duration_ms": 650
  },
  "annotation": {
    "current_color": [0.95, 0.25, 0.25],
    "stroke_width": 4.5,
    "eraser_radius": 28.0,
    "eraser_mode": "object",
    "selected_emoji": "⭐"
  },
  "osd": {
    "enabled": true,
    "font_size": 26.0,
    "bg_color": [0.08, 0.08, 0.1, 0.88],
    "fg_color": [0.98, 0.98, 1.0, 1.0],
    "stay_duration_ms": 1800,
    "fade_duration_ms": 350,
    "border_radius": 14.0,
    "bg_image_path": null,
    "anchor_x": 0.5,
    "anchor_y": 0.88
  }
}
```
