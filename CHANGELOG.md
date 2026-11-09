# Changelog

All notable changes to **ScreenTools** will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v0.10.0] - 2026-11-09
### Added
- **Multi-Selection & Box Marquee (`ActiveTool::Transform`)**:
  - Dragging on empty canvas creates an interactive marquee selection box (translucent cyan fill with dotted outline) selecting all intersected shapes upon mouse release.
  - Holding `Ctrl` or `Shift` while clicking shapes or dragging a marquee toggles and appends to the current multi-selection set.
  - Unified selection bounding box encompassing all selected shapes across multiple monitors and virtual displays.
  - Pressing `Delete` or `Backspace` deletes all currently selected shapes in a single atomic undoable action.
- **8 Interactive Scale Handles with Opposite-Anchor Scaling**:
  - 4 corner scale handles plus 4 edge midpoint handles around the selection bounding box.
  - Dragging any handle scales all selected shapes relative to the opposite anchor corner or edge.
  - Holding `Shift` during scaling constrains aspect ratio uniformly.
- **Rotation Handle with Angle Snapping**:
  - Protruding circular handle with stem line positioned 24px above top center of selection.
  - Dragging rotates all selected shapes smoothly around the selection center point.
  - Holding `Shift` snaps rotation to $15^\circ$ ($\pi / 12$) angular increments.
- **Contextual Win32 Transform Cursors**:
  - Real-time cursor feedback in `WM_SETCURSOR`: `IDC_HAND` for rotation, `IDC_SIZENWSE` and `IDC_SIZENESW` for diagonal scaling, `IDC_SIZENS` and `IDC_SIZEWE` for axial scaling, and `IDC_SIZEALL` for 4-way translation.
- **Zero-Drift Transform Engine & Undo Integrity**:
  - Snapshot-based transform updates eliminating cumulative floating-point drift during continuous drag motions.
  - Full single-step undo (`Ctrl+Z`) and redo (`Ctrl+Y`) for all transform, move, scale, rotate, and delete operations.
  - Pressing `Escape` or Right-Click during drag immediately cancels the active transform and restores original shape coordinates.

## [v0.9.0] - 2026-11-09
### Added
- **Text Tool (`ActiveTool::Text`)**:
  - Re-clicking the Pen button toggles to the Text tool, switching the icon dynamically to `FeatherIcon::Type`.
  - Left-clicking on the canvas creates an active text input box at that exact position with a live blinking caret bar and dashed frame.
  - Full keyboard typing support (`WM_CHAR` / `WM_KEYDOWN`) with Backspace deletion, Enter to commit, and Escape to cancel.
  - Text shapes are rendered crisp and clear via DirectWrite `CreateTextFormat` using customizable font families and sizes.
- **Transform Tool (`ActiveTool::Transform`)**:
  - Re-clicking the Eraser button toggles to the Transform tool, switching the icon dynamically to `FeatherIcon::Move`.
  - Move annotations, text blocks, and emoji stamps across monitors and virtual desktop screens seamlessly with `translate(dx, dy)`.
  - Real-time hover and selection highlight featuring a dashed neon cyan bounding box with 4 corner anchor handles.
  - All object moves are fully recorded in the undo stack (`undo` / `redo`).
- **Text Tool Font Family & Size Settings**:
  - Added font family pill selector ("Segoe UI", "Arial", "Consolas", "Segoe Script", "Times New Roman") in the Settings UI Annotation tab.
  - Added Font Size slider (12 px .. 72 px) for precise typography scaling.

### Changed
- **Square 35x35px Toolbar Buttons**:
  - Resized all dual-column buttons to exactly $35 \times 35\text{px}$ square dimensions across all toolbar rows.
  - Adjusted overall panel height to 320.0px for balanced vertical proportions and clean spacing.
- **Vector Icons**:
  - Added pure Feather vector icons for `Type` (T text icon) and `Move` (4-way directional cross arrows).
- **Settings Window Layout**:
  - Scaled Settings window height to 750px to cleanly fit the new Typography controls alongside stroke dimensions, swatches, and custom emoji controls.

### Fixed
- **Toolbar Sliders Dragging & Responsiveness**:
  - Resolved slider input dropping by invoking Win32 `SetCapture(hwnd)` when slider dragging begins (`active_slider.is_some()`).
  - Sliders now smoothly track mouse motion across the entire desktop, even when drifting outside the 85px panel bounds.

## [v0.8.0] - 2026-11-09
### Added
- **Auto Screen-Boundary Flyout Positioning**:
  - Dynamically detects display width (`SM_CXSCREEN`) to flip emoji and color palette flyout grids to the left of the toolbar when near the right edge of the screen, preventing out-of-screen clipping.
- **Wavy Line Tool Sub-Mode**:
  - Added continuous sine wave drawing to the Straight Line tool (`line_wavy`), cycling through Solid -> Dotted -> Wavy -> Solid on repeated clicks.

### Changed
- **Compact Toolbar 85px Geometry**:
  - Symmetrically fitted buttons and slider tracks into the 85px wide floating control panel without altering vertical layout or panel height (300px).
  - Adjusted dual-column buttons to 35px width and slider tracks to 73px width with centered drag handle pill.
- **Flyout Re-Click Collapse**:
  - Clicking the Emoji or Color button while its flyout grid is open now cleanly collapses the expanded flyout.
- **Smooth Continuous Spatial-Phase Sine Waves**:
  - Replaced variable cycle jumping with constant spatial wavelength ($\lambda = 26.0\text{px}$) and cubic smoothstep entry/exit tapers.
  - Prevents wave jumping, jittering, or crest squashing when drawing lines or arrows of varying lengths.
- **Context Menu Hierarchy**:
  - Relocated `🎨 Drawing Panel` below the Presentation Modes divider in the system tray menu.
  - Automatically grays out and disables (`MF_GRAYED | MF_DISABLED`) the panel toggle when in Mouse Highlighter or Laser Pointer modes.

### Removed
- **Tapered Drawing Pen**:
  - Completely removed tapered pen calligraphy mode to ensure clean, consistent stroke drawing across all resolutions.

## [v0.7.1] - 2026-11-07
### Changed
- **Shift Key OSD Display Enhancement**:
  - Alphabet keys with Shift now format as `Shift + f(F)` instead of raw capital letters.
  - Numbers and OEM symbols format with shifted equivalents (e.g. `1(!)`).
  - Standalone modifier tokens are replaced by full chords when followed immediately by combinations.
- **Persistent Write Panel Position & Selected Tool**:
  - Saved last floating toolbar `(toolbar_x, toolbar_y)` client coordinates upon drag release via Win32 capture.
  - Added `selected_tool` field to `AnnotationConfig`, persisting and restoring active tool across sessions and drawing mode toggles.
- **Wavy Sine Arrow Refinements**:
  - Re-engineered wavy arrow stem calculation with full integer sine periods and smooth quadratic easing at both ends.
  - Stem transitions directly into the razor-sharp chevron arrowhead notch without kinks or tilts.
- **Streamlined Tray Context Menu**:
  - Removed cluttered `🛠️ Drawing Tools` submenu.
  - Added direct toggle item `🎨 Drawing Panel` to show/hide the floating toolbar.

### Removed
- **Wipe Eraser Mode**:
  - Completely removed `wipe_at` and wipe mode toggles to ensure rock-solid stability and prevent application crashes.
  - Restored clean, high-performance object eraser mode.

## [v0.7.0] - 2026-11-07
### Added
- **Razor-Sharp Filled Arrowheads**:
  - Replaced overlapping line wing strokes with closed chevron arrowhead polygons filled with anti-aliasing.
  - The arrowhead tip is mathematically razor-sharp, with stems terminating at the chevron notch to prevent apex bleed-through.
- **Tapered Pen Sub-Mode**:
  - Re-clicking the Pen button toggles tapered calligraphy mode (`pen_tapered`).
  - Strokes dynamically taper to sharp points at both ends using Catmull-Rom spline width modulation ($\sin(\pi \cdot t)$).
- **Wipe Eraser Mode**:
  - Re-clicking the Eraser button toggles between Object Eraser and Wipe Eraser (`eraser_mode: "object" <-> "wipe"`).
  - Wipe mode continuously slices and clears freehand strokes and shapes directly under the cursor drag.
  - Displays a dedicated translucent red indicator with dotted circular boundary sized to the wipe radius.
- **Wavy Sine Arrow Sub-Mode**:
  - Re-clicking the Arrow button toggles wavy snake arrow mode (`arrow_wavy`), rendering an undulating sine wave stem leading into a razor-sharp apex.
- **Dotted Line Sub-Mode**:
  - Re-clicking the Line button toggles dotted line mode (`line_dotted`), rendering clean circular dots via custom Direct2D round dash styles.
- **Shift Key Geometric Constraints & Snapping**:
  - Holding Shift while drawing Line or Arrow snaps angles to 45° increments ($\pi / 4$).
  - Holding Shift while drawing Rectangle snaps rotation to 45° increments.
  - Holding Shift while drawing Ellipse constrains it to a perfect circle (`rx == ry`).
- **Toolbar Sub-Mode Visual Feedback**:
  - Dynamic Feather icon indicators for active sub-modes: Tapered Pen with nib flair, Wipe Eraser with wipe motion cues, Wavy Arrow with sine stem, and Dotted Line with 4 circular dots.

### Fixed
- **Shift Key Keystroke OSD Display**:
  - Removed erroneous chord suppression in `src/keystroke.rs`, enabling standalone Shift keypresses and chords like `Shift + Click` or `Shift + Key` to display reliably in the OSD badge overlay.

## [v0.6.0] - 2026-11-07
### Added
- **Feather Icons Aesthetic Toolbar**:
  - Implemented crisp vector path rendering inspired by Feather Icons (https://feathericons.com/) with uniform visual bounding box (18x18px) and round caps/joins.
  - Icons rendered: `edit-2` (Brush), `eraser`, `circle` (Round), `square` (Rectangle), `arrow-up-right` (Arrow), `minus` (Line), `rotate-ccw` (Undo), `rotate-cw` (Redo), `trash-2` (Clear), `x` (Exit), `help-circle` (2-step Confirm).
- **Mode-Matched Squircle System Tray Icons**:
  - Dynamically generated 32x32 superellipse (squircle) tray icons distinguished by mode:
    - **Highlighter Mode**: Golden amber squircle with luminous sun beacon glyph.
    - **Laser Mode**: Neon crimson squircle with electric lightning zap glyph.
    - **Drawing Mode**: Azure blue squircle with precision drawing pencil glyph.
  - Updates in real-time as the user switches or cycles presentation modes.
- **Topmost Settings Window with Canvas Click-Through**:
  - Added `WS_EX_TOPMOST` and dynamic layering to Settings Window, keeping it above fullscreen overlay drawings.
  - Added mouse hit-testing exclusion so hovering and clicking over Settings Window passes through cleanly without drawing marks underneath.

### Changed
- **Compact 75% Width Toolbar**:
  - Reduced floating write panel width from 140px to 105px (75% width) and height from 382px to 308px.
  - Minimized padding to tiny up the panel while keeping comfortable 44x32px click targets.
- **Set Current User Settings as Default**:
  - Config defaults updated in `src/config.rs`: Highlighter size 55px (opacity 50%, softness 1.0), Laser radius 5.0px, Annotation stroke width 6.0px (min 2.0px, max 50.0px, toolbar opacity 80%), and white color presets.

## [v0.5.2] - 2026-11-07
### Changed
- **Freehand Pen/Pencil Hardware Cursor**:
  - Bound custom 32x32 Pencil hardware cursor (`pen_cursor`) to Freehand drawing tool with graphite tip at `(0, 0)` matching the live preview dot.
- **Flyout Outside-Click Dismissal**:
  - Clicking anywhere outside open toolbar flyouts (emoji or color selector grids) cleanly dismisses the flyout without creating accidental marks or stamps on the canvas.
- **Toolbar Drag Safety Clamping**:
  - Clamped toolbar drag coordinates so the top bar handle cannot be pushed off-screen above the display boundary.
- **Dynamic Emoji Cursor Synchronization**:
  - Ensured hardware emoji cursor updates immediately whenever active emoji selection changes via the toolbar flyout.

## [v0.5.1] - 2026-11-07
### Fixed
- **Settings Page Color Selection & Swatch Edit**:
  - Corrected color swatch click and right-click Y-coordinate hit-testing in Highlighter tab (`352.0`) and Laser tab (`272.0`) to exactly align with rendered positions.
  - Enabled right-click color swatch editing (Windows Color Picker) across all tabs, ensuring swatches update and sync consistently.
- **Sync Color Across Modes Toggle**:
  - Fixed "Sync Color Across Modes" toggle button hit-testing region in Highlighter tab (`y = 392.0 .. 424.0`), restoring full interactive toggling between unified and per-mode color management.

## [v0.5.0] - 2026-11-07
### Added
- **Toggle Filled and Hollow Shapes on Re-click**:
  - Clicking the Ellipse / Circle icon again while active toggles between filled (`⬤`) and hollow (`⭕`).
  - Clicking the Rectangle / Box icon again while active toggles between filled (`⬛`) and hollow (`🔲`).
  - Full interior fill hit-testing support for filled shapes in Object Eraser mode.
- **Color Selector Button on Floating Toolbar (Line 5)**:
  - Toolbar Line 5 split into two side-by-side buttons:
    - Left: Emoji stamp selector displaying current active emoji and `▾` expander.
    - Right: Color swatch button displaying active drawing color dot and `▾` expander opening an interactive 3x3 swatches flyout.
- **Relative Emoji Stamp Scaling & Opacity**:
  - Emoji stamps now dynamically scale with the stroke size slider (`20.0 + size_pct * 70.0`) and inherit the active stroke opacity.
- **Direct2D Live Cursor Preview Matching Exact Size and Opacity**:
  - Live cursor preview dot matching the exact stroke width diameter, drawing color, and opacity with high-contrast dual ring and pinpoint center.
  - Live emoji cursor preview rendered via Direct2D color fonts matching the exact relative stamp size and opacity.
  - Precision crosshair (`IDC_CROSS`) hardware cursor active over canvas so the solid pointer never obscures the live preview.
- **Settings Custom Emoji Text Input (Max 9)**:
  - Added native Win32 `EDIT` input box styled for dark theme (`WM_CTLCOLOREDIT`) with `Segoe UI Emoji` font and placeholder cue banner.
  - Added `[+ Add Emoji]` button and `Enter` key shortcut to add custom emojis (capped at 9 slots).
  - Left-click to select active emoji stamp, right-click to delete any slot from the preset list.
  - Seamless synchronization across Settings UI, Floating Toolbar emoji flyout, and Tray right-click context menu.

### Changed
- **Pure Icon-Only Toolbar**:
  - Removed all text labels from toolbar buttons for a sleek, compact visual presentation.
  - Sliders display compact percentage indicators (`💧 80%`, `📏 25%`) without verbose text.
- **Grayed-Out Undo & Redo**:
  - Undo (`↶`) and Redo (`↷`) buttons automatically dim with reduced opacity and ignore clicks when history stacks are empty.
- **Floating Drawing Control Panel**:
  - Always-on-top interactive floating toolbar displayed exclusively during drawing mode.
  - Line-by-line 9-tier layout:
    - **Line 1**: Drag handle bar allowing seamless repositioning anywhere on screen with automatic display boundary clamping.
    - **Line 2**: Brush (Pen / Freehand) and Eraser tool toggle buttons.
    - **Line 3**: Round (Ellipse) and Rectangle shape tool buttons.
    - **Line 4**: Arrow and Line shape tool buttons.
    - **Line 5**: Selected emoji stamp button with drop-down expander (`▾`) opening an interactive flyout grid to quickly select any of the 12 preset emojis.
    - **Line 6**: Stroke opacity slider dynamically controlling transparency (5% to 100%) for future drawings, geometric shapes, and emoji stamps.
    - **Line 7**: Stroke size percentage slider (0% to 100%), dynamically mapped between user-configurable minimum and maximum stroke pixel widths.
    - **Line 8**: Dedicated Undo (`↶`) and Redo (`↷`) buttons.
    - **Line 9**: Clear (`🗑️`) and Exit (`✕`) buttons with two-step confirmation (`❓ Clear?` / `❓ Exit?`) to prevent accidental wipe or mode termination; exit seamlessly returns to highlighter mode.
  - Top-layer Z-ordering ensuring drawing strokes never paint over or obscure the toolbar.
  - Hover cursor hit-testing automatically showing standard pointer arrow over the toolbar instead of drawing cursors.
- **Horizontal Tabs Settings Window Redesign**:
  - Completely replaced vertical sidebar layout with sleek, centered horizontal top navigation tabs (`💡 Highlighter`, `⚡ Laser Pointer`, `✏️ Drawing & Tools`, `⌨️ Keystroke OSD`).
  - Top-right `↺ Reset Defaults` action button.
  - New drawing configuration controls in Settings:
    - Toggle to show/hide the floating control panel.
    - Floating control panel background opacity slider (40% to 100%).
    - Minimum stroke width slider (0.5 to 10.0 px).
    - Maximum stroke width slider (15.0 to 60.0 px).
    - Future stroke & emoji opacity slider (5% to 100%).

### Changed
- Drawing mode `WM_LBUTTONDOWN` and `WM_MOUSEMOVE` prioritize toolbar interaction before initiating canvas strokes.
- Canvas clicks automatically dismiss open emoji flyouts and reset pending toolbar confirmation states.

## [v0.3.0] - 2026-11-06
### Added
- Dynamic custom hardware cursors for drawing mode:
  - Custom Pen / Pencil cursor with precision tip for freehand, lines, arrows, rectangles, and ellipses.
  - Custom beveled Eraser cursor with bottom-left contact hotspot for object eraser mode.
  - Dynamic full-color Emoji stamp cursor rendered via Direct2D and DirectWrite color fonts matching the currently active emoji.
- Windows system color picker dialog (`ChooseColorW`) accessible by right-clicking any color swatch in Settings.
- Automatic human-readable color naming algorithm (`color_names` module) computing perceptual RGB distance to identify and display nearest named color and emoji.
- Dynamic color labels in tray context menu presets and settings UI showing auto-detected color name and hex tag.
- Color synchronization for swatches: updating any swatch syncs to all presentation modes when `sync_colors` is enabled.
- Visual disabled and locked state (reduced opacity and lock indicator banner) for Laser and Drawing color swatches when color synchronization is active.

### Changed
- Standardized color presets across Highlighter, Laser, and Drawing settings to use the unified customizable palette.

## [v0.2.0] - 2026-11-06
### Added
- Independent color configuration for each presentation mode (Highlighter, Laser, Drawing).
- Setting toggle `sync_colors` to synchronize colors across all tools or allow separate per-mode palettes.
- Context menu color picker dynamically targeting the active presentation mode when color sync is disabled.
- Persistent development guidelines in `GEMINI.md` enforcing SemVer (`v0.0.0`) and changelog maintenance.

### Removed
- Expanding click ripple effect and associated settings UI controls.

## [v0.1.0] - 2026-11-03
### Added
- Core multi-mode overlay engine with mouse cursor highlighter, laser pointer, and drawing tools.
- Low-latency 30 FPS rendering loop with Direct2D hardware acceleration.
- Automatic dark/light theme detection for Windows tray context menu.
- Keystroke HUD OSD badge display.
- Bezier smoothing for pen strokes and laser trail.

