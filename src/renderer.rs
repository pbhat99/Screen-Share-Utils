use crate::annotation::{
    ActiveTool, AnnotationManager, LaserTrailPoint, Shape,
};
use crate::config::{AppConfig, HighlighterConfig, LaserConfig, OsdConfig};
use crate::geometry::Point2D;
use crate::keystroke::KeyDisplayToken;

use std::mem::ManuallyDrop;
use std::path::Path;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;

pub struct D2DRenderer {
    _hwnd: HWND,
    _width: u32,
    _height: u32,
    offset_x: f32,
    offset_y: f32,

    dwrite_factory: IDWriteFactory,
    d2d_factory: ID2D1Factory1,
    d2d_context: ID2D1DeviceContext,
    render_target: ID2D1RenderTarget,
    swap_chain: IDXGISwapChain1,
    _dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,

    text_format_osd: IDWriteTextFormat,
    text_format_emoji: IDWriteTextFormat,
    text_format_toolbar: IDWriteTextFormat,
    text_format_toolbar_small: IDWriteTextFormat,
    pub round_stroke_style: ID2D1StrokeStyle,
    pub dotted_stroke_style: ID2D1StrokeStyle,

    osd_bg_bitmap: Option<ID2D1Bitmap>,
    loaded_bg_path: Option<String>,
}

impl D2DRenderer {
    pub fn new(hwnd: HWND, vx: i32, vy: i32, vw: i32, vh: i32) -> Result<Self> {
        let width = vw.max(100) as u32;
        let height = vh.max(100) as u32;
        let offset_x = vx as f32;
        let offset_y = vy as f32;

        unsafe {
            // 1. D3D11 Device
            let mut d3d_device: Option<ID3D11Device> = None;
            let mut d3d_context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                Some(&mut feature_level),
                Some(&mut d3d_context),
            )?;

            let d3d_device = d3d_device.unwrap();
            let dxgi_device: IDXGIDevice = d3d_device.cast()?;

            // 2. Direct2D Factory and Device
            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let render_target: ID2D1RenderTarget = d2d_context.cast()?;

            // 3. DirectWrite Factory & Formats
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            // Round stroke style for smooth, continuous, non-dotted laser trails and pen lines
            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            };
            let round_stroke_style: ID2D1StrokeStyle = d2d_factory.CreateStrokeStyle(&stroke_props, None)?.into();

            let dotted_props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_CUSTOM,
                dashOffset: 0.0,
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            };
            let dashes = [0.0f32, 2.2f32];
            let dotted_stroke_style: ID2D1StrokeStyle = d2d_factory.CreateStrokeStyle(&dotted_props, Some(&dashes))?.into();

            let text_format_osd = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                26.0,
                w!("en-US"),
            )?;
            let _ = text_format_osd.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_osd.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_emoji = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                32.0,
                w!("en-US"),
            )?;
            let _ = text_format_emoji.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_emoji.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toolbar = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("en-US"),
            )?;
            let _ = text_format_toolbar.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_toolbar.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toolbar_small = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                10.0,
                w!("en-US"),
            )?;

            // 4. DXGI SwapChain for DirectComposition
            let dxgi_adapter: IDXGIAdapter = dxgi_device.GetAdapter()?;
            let dxgi_factory: IDXGIFactory2 = dxgi_adapter.GetParent()?;

            let swap_chain_desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: FALSE,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };

            let swap_chain = dxgi_factory.CreateSwapChainForComposition(&d3d_device, &swap_chain_desc, None)?;
            let back_buffer: IDXGISurface = swap_chain.GetBuffer(0)?;

            let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };

            let target_bitmap = d2d_context.CreateBitmapFromDxgiSurface(&back_buffer, Some(&bitmap_properties))?;
            d2d_context.SetTarget(&target_bitmap);

            // 5. DirectComposition visual tree
            let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(Some(&dxgi_device))?;
            let dcomp_target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp_device.CreateVisual()?;
            visual.SetContent(&swap_chain)?;
            dcomp_target.SetRoot(&visual)?;
            dcomp_device.Commit()?;

            Ok(Self {
                _hwnd: hwnd,
                _width: width,
                _height: height,
                offset_x,
                offset_y,
                dwrite_factory,
                d2d_factory,
                d2d_context,
                render_target,
                swap_chain,
                _dcomp_device: dcomp_device,
                _dcomp_target: dcomp_target,
                text_format_osd,
                text_format_emoji,
                text_format_toolbar,
                text_format_toolbar_small,
                round_stroke_style,
                dotted_stroke_style,
                osd_bg_bitmap: None,
                loaded_bg_path: None,
            })
        }
    }

    /// Screen to Virtual Client Coordinate
    pub fn d2d_factory(&self) -> &ID2D1Factory1 {
        &self.d2d_factory
    }

    pub fn dwrite_factory(&self) -> &IDWriteFactory {
        &self.dwrite_factory
    }

    pub fn screen_to_client(&self, p: Point2D) -> Point2D {
        Point2D::new(p.x - self.offset_x, p.y - self.offset_y)
    }

    /// Load or reload OSD background image if configured
    pub fn update_osd_bg_image(&mut self, path_opt: &Option<String>) {
        if self.loaded_bg_path == *path_opt {
            return;
        }
        self.loaded_bg_path = path_opt.clone();
        self.osd_bg_bitmap = None;

        if let Some(ref path_str) = path_opt {
            if Path::new(path_str).exists() {
                if let Ok(img) = image::open(path_str) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    unsafe {
                        let props = D2D1_BITMAP_PROPERTIES {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                            },
                            dpiX: 96.0,
                            dpiY: 96.0,
                        };
                        let size = D2D_SIZE_U { width: w, height: h };
                        if let Ok(bmp) = self.render_target.CreateBitmap(
                            size,
                            Some(rgba.as_raw().as_ptr() as *const _),
                            w * 4,
                            &props,
                        ) {
                            self.osd_bg_bitmap = Some(bmp);
                        }
                    }
                }
            }
        }
    }

    /// Render full frame with Direct2D
    pub fn render_frame(
        &mut self,
        cursor_screen: Point2D,
        cursor_visible: bool,
        config: &AppConfig,
        annotation_mgr: &AnnotationManager,
        active_tool: ActiveTool,
        is_drawing_mode: bool,
        toolbar: &crate::toolbar::DrawingToolbar,
        osd_tokens: &[KeyDisplayToken],
    ) -> Result<()> {
        let cursor_client = self.screen_to_client(cursor_screen);

        unsafe {
            self.d2d_context.BeginDraw();
            // Clear to transparent black
            self.d2d_context.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

            // 1. Mouse Cursor Highlighter (if enabled and cursor is on-screen)
            if config.highlighter.enabled && cursor_visible {
                self.draw_highlighter(cursor_client, &config.highlighter)?;
            }

            // 2. Laser Pointer Trail
            if config.laser.enabled {
                let laser_color = if config.sync_colors {
                    config.highlighter.color
                } else {
                    config.laser.color
                };
                self.draw_laser(&annotation_mgr.laser_points, cursor_client, &config.laser, laser_color)?;
            }

            // 3. Completed Annotation Shapes
            for shape in &annotation_mgr.shapes {
                self.draw_shape(shape, 1.0)?;
            }

            // 4. In-progress Shape (Live Preview)
            if let Some(ref in_prog) = annotation_mgr.in_progress {
                self.draw_shape(in_prog, 0.85)?;
            }

            // 4b. Active Text Input (Typing live preview with blinking caret)
            if let Some(ref text_input) = annotation_mgr.active_text {
                let text_shape = Shape::Text {
                    pos: text_input.pos,
                    text: text_input.text.clone(),
                    font_family: text_input.font_family.clone(),
                    font_size: text_input.font_size,
                    color: text_input.color,
                };
                self.draw_shape(&text_shape, 1.0)?;

                let font_sz = text_input.font_size;
                let approx_w = (text_input.text.len() as f32 * font_sz * 0.6).max(14.0);
                let approx_h = (font_sz * 1.3).max(24.0);

                let box_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: text_input.pos.x - 4.0,
                        top: text_input.pos.y - 2.0,
                        right: text_input.pos.x + approx_w + 14.0,
                        bottom: text_input.pos.y + approx_h + 2.0,
                    },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                let box_border = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F { r: 0.35, g: 0.65, b: 1.0, a: 0.85 },
                    None,
                )?;
                let box_bg = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F { r: 0.1, g: 0.12, b: 0.18, a: 0.35 },
                    None,
                )?;
                self.d2d_context.FillRoundedRectangle(&box_rect, &box_bg);
                self.d2d_context.DrawRoundedRectangle(&box_rect, &box_border, 1.0, Some(&self.dotted_stroke_style));

                // Blinking cursor
                static START_INSTANT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let start = START_INSTANT.get_or_init(std::time::Instant::now);
                let blink = (start.elapsed().as_millis() % 1000) < 500;
                if blink {
                    let caret_x = text_input.pos.x + approx_w + 3.0;
                    let caret_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 },
                        None,
                    )?;
                    self.d2d_context.DrawLine(
                        D2D_POINT_2F { x: caret_x, y: text_input.pos.y + 2.0 },
                        D2D_POINT_2F { x: caret_x, y: text_input.pos.y + font_sz * 1.15 },
                        &caret_brush,
                        2.0,
                        None,
                    );
                }
            }

            // 4c. Transform Tool: Marquee Box, Selection Box, Handles, and Hover Frames
            if is_drawing_mode && active_tool == ActiveTool::Transform {
                // Marquee drag box
                if let (Some(start), Some(curr)) = (annotation_mgr.marquee_start, annotation_mgr.marquee_current) {
                    let m_min_x = start.x.min(curr.x);
                    let m_max_x = start.x.max(curr.x);
                    let m_min_y = start.y.min(curr.y);
                    let m_max_y = start.y.max(curr.y);

                    let m_rect = D2D_RECT_F {
                        left: m_min_x,
                        top: m_min_y,
                        right: m_max_x,
                        bottom: m_max_y,
                    };
                    let m_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.80, b: 1.0, a: 0.9 },
                        None,
                    )?;
                    let m_fill = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.80, b: 1.0, a: 0.12 },
                        None,
                    )?;
                    self.d2d_context.FillRectangle(&m_rect, &m_fill);
                    self.d2d_context.DrawRectangle(&m_rect, &m_brush, 1.2, Some(&self.dotted_stroke_style));
                }

                // Selection Bounding Box & 8 Scale Handles + Rotation Stem & Handle
                if let Some((min_pt, max_pt)) = annotation_mgr.get_selection_bounding_box() {
                    let pad = 6.0;
                    let box_min = Point2D::new(min_pt.x - pad, min_pt.y - pad);
                    let box_max = Point2D::new(max_pt.x + pad, max_pt.y + pad);
                    let center = Point2D::new((box_min.x + box_max.x) * 0.5, (box_min.y + box_max.y) * 0.5);

                    let sel_rect = D2D_RECT_F {
                        left: box_min.x,
                        top: box_min.y,
                        right: box_max.x,
                        bottom: box_max.y,
                    };
                    let sel_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.85, b: 1.0, a: 0.95 },
                        None,
                    )?;
                    let sel_fill = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.85, b: 1.0, a: 0.06 },
                        None,
                    )?;
                    self.d2d_context.FillRectangle(&sel_rect, &sel_fill);
                    self.d2d_context.DrawRectangle(&sel_rect, &sel_brush, 1.5, Some(&self.dotted_stroke_style));

                    // Rotation Handle + Stem
                    let rot_top = Point2D::new(center.x, box_min.y);
                    let rot_handle = Point2D::new(center.x, box_min.y - 24.0);
                    self.d2d_context.DrawLine(
                        D2D_POINT_2F { x: rot_top.x, y: rot_top.y },
                        D2D_POINT_2F { x: rot_handle.x, y: rot_handle.y },
                        &sel_brush,
                        1.2,
                        None,
                    );

                    let handle_fill = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                        None,
                    )?;
                    let rot_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: rot_handle.x, y: rot_handle.y },
                        radiusX: 5.0,
                        radiusY: 5.0,
                    };
                    self.d2d_context.FillEllipse(&rot_ellipse, &handle_fill);
                    self.d2d_context.DrawEllipse(&rot_ellipse, &sel_brush, 1.5, None);

                    // 8 Scale Handles
                    let handle_pts = [
                        // 4 Corners
                        D2D_POINT_2F { x: box_min.x, y: box_min.y },
                        D2D_POINT_2F { x: box_max.x, y: box_min.y },
                        D2D_POINT_2F { x: box_max.x, y: box_max.y },
                        D2D_POINT_2F { x: box_min.x, y: box_max.y },
                        // 4 Edges
                        D2D_POINT_2F { x: center.x, y: box_min.y },
                        D2D_POINT_2F { x: center.x, y: box_max.y },
                        D2D_POINT_2F { x: box_min.x, y: center.y },
                        D2D_POINT_2F { x: box_max.x, y: center.y },
                    ];

                    for pt in handle_pts {
                        let h_rect = D2D_RECT_F {
                            left: pt.x - 3.5,
                            top: pt.y - 3.5,
                            right: pt.x + 3.5,
                            bottom: pt.y + 3.5,
                        };
                        self.d2d_context.FillRectangle(&h_rect, &handle_fill);
                        self.d2d_context.DrawRectangle(&h_rect, &sel_brush, 1.2, None);
                    }
                }

                // Hover highlight on unselected shape
                if let Some(h_idx) = annotation_mgr.hovered_shape_idx {
                    if !annotation_mgr.selected_indices.contains(&h_idx) && h_idx < annotation_mgr.shapes.len() {
                        let (h_min, h_max) = annotation_mgr.shapes[h_idx].get_bounding_box();
                        let pad = 4.0;
                        let h_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: h_min.x - pad,
                                top: h_min.y - pad,
                                right: h_max.x + pad,
                                bottom: h_max.y + pad,
                            },
                            radiusX: 4.0,
                            radiusY: 4.0,
                        };
                        let h_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.0, g: 0.85, b: 1.0, a: 0.5 },
                            None,
                        )?;
                        self.d2d_context.DrawRoundedRectangle(&h_rect, &h_brush, 1.0, Some(&self.dotted_stroke_style));
                    }
                }
            }

            // 5. Cursor Indicator & Live Brush Preview (Matching exact size and opacity)
            if is_drawing_mode && !toolbar.hit_test(cursor_client) {
                if active_tool == ActiveTool::Eraser {
                    self.draw_eraser_indicator(cursor_client)?;
                } else if active_tool == ActiveTool::Emoji {
                    // Render dynamic emoji preview with exact size and opacity
                    let min_w = config.annotation.min_stroke_width.max(0.5);
                    let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
                    let size_pct = ((config.annotation.stroke_width - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
                    let emoji_size = 20.0 + size_pct * 70.0;
                    let emoji_op = config.annotation.stroke_opacity.clamp(0.05, 1.0);

                    let preview_shape = Shape::Emoji {
                        pos: cursor_client,
                        emoji: config.annotation.selected_emoji.clone(),
                        size: emoji_size,
                        opacity: emoji_op,
                    };
                    self.draw_shape(&preview_shape, 0.90)?;
                } else if active_tool != ActiveTool::None && active_tool != ActiveTool::Transform && active_tool != ActiveTool::Text {
                    // Render dynamic stroke dot preview with exact diameter, color, and opacity
                    let radius = (config.annotation.stroke_width * 0.5).max(1.5);
                    let stroke_col = config.annotation.current_color;
                    let stroke_op = config.annotation.stroke_opacity.clamp(0.05, 1.0);

                    // 1. Fill dot
                    let dot_fill_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: stroke_col[0], g: stroke_col[1], b: stroke_col[2], a: stroke_op },
                        None,
                    )?;
                    let dot_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: radius,
                        radiusY: radius,
                    };
                    self.d2d_context.FillEllipse(&dot_ellipse, &dot_fill_brush);

                    // 2. High-contrast dual outline (dark shadow + bright rim)
                    let shadow_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.65 },
                        None,
                    )?;
                    let rim_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.85 },
                        None,
                    )?;
                    let outer_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: radius + 0.5,
                        radiusY: radius + 0.5,
                    };
                    self.d2d_context.DrawEllipse(&outer_ellipse, &shadow_brush, 1.8, None);
                    self.d2d_context.DrawEllipse(&dot_ellipse, &rim_brush, 1.0, None);

                    // 3. Pinpoint center point
                    let pin_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: 1.0,
                        radiusY: 1.0,
                    };
                    self.d2d_context.FillEllipse(&pin_ellipse, &rim_brush);
                }
            }

            // 6. Floating Drawing Control Panel (Rendered on top of all drawings)
            if is_drawing_mode && config.annotation.show_toolbar {
                let _ = toolbar.render(
                    &self.render_target,
                    &self.text_format_toolbar,
                    &self.text_format_toolbar_small,
                    &self.text_format_emoji,
                    config,
                    active_tool,
                    annotation_mgr.can_undo(),
                    annotation_mgr.can_redo(),
                );
            }

            // 7. Keystroke OSD Badges
            if config.osd.enabled && !osd_tokens.is_empty() {
                self.draw_osd(osd_tokens, &config.osd, cursor_screen)?;
            }

            self.d2d_context.EndDraw(None, None)?;
            self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        }

        Ok(())
    }

    /// Draw Mouse Cursor Highlighter with adjustable size, softness, color, and opacity
    unsafe fn draw_highlighter(&self, pos: Point2D, config: &HighlighterConfig) -> Result<()> {
        let r = config.size.max(5.0);
        let softness = config.edge_softness.clamp(0.01, 0.99);
        let inner_stop_pos = (1.0 - softness).clamp(0.0, 0.98);

        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: D2D1_COLOR_F {
                    r: config.color[0],
                    g: config.color[1],
                    b: config.color[2],
                    a: config.opacity,
                },
            },
            D2D1_GRADIENT_STOP {
                position: inner_stop_pos,
                color: D2D1_COLOR_F {
                    r: config.color[0],
                    g: config.color[1],
                    b: config.color[2],
                    a: config.opacity * 0.95,
                },
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: D2D1_COLOR_F {
                    r: config.color[0],
                    g: config.color[1],
                    b: config.color[2],
                    a: 0.0,
                },
            },
        ];

        let stop_collection = self.render_target.CreateGradientStopCollection(
            &stops,
            D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;

        let brush_props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F { x: pos.x, y: pos.y },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: r,
            radiusY: r,
        };

        let brush = self.render_target.CreateRadialGradientBrush(
            &brush_props,
            None,
            &stop_collection,
        )?;

        let ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: pos.x, y: pos.y },
            radiusX: r,
            radiusY: r,
        };

        self.d2d_context.FillEllipse(&ellipse, &brush);

        Ok(())
    }

    /// Draw Fading Solid Laser Trail (Pure single uniform color without glow, fading size and opacity over time)
    unsafe fn draw_laser(
        &self,
        points: &[LaserTrailPoint],
        cur_pos: Point2D,
        config: &LaserConfig,
        laser_color: [f32; 3],
    ) -> Result<()> {
        let dur = config.trail_duration_ms as f32;
        let base_r = config.radius.max(2.5);

        // 1. Deduplicate & collect raw trail points with age parameter t in [0.0, 1.0]
        let mut raw_pts: Vec<(Point2D, f32)> = Vec::with_capacity(points.len() + 1);
        for pt in points {
            let elapsed = pt.created_at.elapsed().as_millis() as f32;
            let t = (1.0 - (elapsed / dur)).clamp(0.0, 1.0);
            if t > 0.01 {
                if let Some(last) = raw_pts.last() {
                    if last.0.distance_to(pt.pos) > 1.5 {
                        raw_pts.push((pt.pos, t));
                    }
                } else {
                    raw_pts.push((pt.pos, t));
                }
            }
        }
        if let Some(last) = raw_pts.last() {
            if last.0.distance_to(cur_pos) > 1.5 {
                raw_pts.push((cur_pos, 1.0));
            }
        } else {
            raw_pts.push((cur_pos, 1.0));
        }

        let n = raw_pts.len();

        // 2. Single color brush (no glow, single color matching highlighter)
        let brush = self.render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: laser_color[0],
                g: laser_color[1],
                b: laser_color[2],
                a: 1.0,
            },
            None,
        )?;

        // 3. Render continuous ribbon fading BOTH size AND opacity over time (Catmull-Rom Bezier smoothed)
        if n >= 2 {
            struct RibbonSlice {
                left: D2D_POINT_2F,
                right: D2D_POINT_2F,
                t: f32,
            }

            let samples_per_seg = 3;
            let mut slices: Vec<RibbonSlice> = Vec::new();

            for i in 0..(n - 1) {
                let p0 = if i > 0 { raw_pts[i - 1].0 } else { raw_pts[i].0 };
                let p1 = raw_pts[i].0;
                let p2 = raw_pts[i + 1].0;
                let p3 = if i + 2 < n { raw_pts[i + 2].0 } else { p2 };

                let t1 = raw_pts[i].1;
                let t2 = raw_pts[i + 1].1;

                let is_last_seg = i == n - 2;
                let max_k = if is_last_seg { samples_per_seg } else { samples_per_seg - 1 };

                for k in 0..=max_k {
                    let u = k as f32 / samples_per_seg as f32;

                    let u2 = u * u;
                    let u3 = u2 * u;

                    let px = 0.5 * ((2.0 * p1.x)
                        + (-p0.x + p2.x) * u
                        + (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * u2
                        + (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * u3);

                    let py = 0.5 * ((2.0 * p1.y)
                        + (-p0.y + p2.y) * u
                        + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * u2
                        + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * u3);

                    let tx = 0.5 * ((-p0.x + p2.x)
                        + 2.0 * (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * u
                        + 3.0 * (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * u2);

                    let ty = 0.5 * ((-p0.y + p2.y)
                        + 2.0 * (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * u
                        + 3.0 * (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * u2);

                    let len = (tx * tx + ty * ty).sqrt();
                    let (nx, ny) = if len > 0.001 {
                        (-ty / len, tx / len)
                    } else {
                        (0.0, 1.0)
                    };

                    let t = (t1 + u * (t2 - t1)).clamp(0.0, 1.0);
                    // Fade size over time (t): from base_r down to 0 at the tail
                    let hw = (base_r * t).max(0.5);

                    slices.push(RibbonSlice {
                        left: D2D_POINT_2F {
                            x: px + nx * hw,
                            y: py + ny * hw,
                        },
                        right: D2D_POINT_2F {
                            x: px - nx * hw,
                            y: py - ny * hw,
                        },
                        t,
                    });
                }
            }

            if slices.len() >= 2 {
                for j in 0..(slices.len() - 1) {
                    let t_avg = (slices[j].t + slices[j + 1].t) * 0.5;
                    // Fade opacity over time (t): from 0.95 down to 0 at the tail
                    let alpha = (t_avg * 0.95).clamp(0.0, 1.0);
                    if alpha < 0.015 {
                        continue;
                    }

                    let path: ID2D1PathGeometry = self.d2d_factory.CreatePathGeometry()?.into();
                    let sink = path.Open()?;
                    sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                    sink.BeginFigure(slices[j].left, D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(slices[j + 1].left);
                    sink.AddLine(slices[j + 1].right);
                    sink.AddLine(slices[j].right);
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    sink.Close()?;

                    brush.SetOpacity(alpha);
                    self.d2d_context.FillGeometry(&path, &brush, None);
                }
            }
        }

        // 4. Laser pointer dot at the cursor head (Single solid circle, zero glow/halo)
        let dot_ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: cur_pos.x, y: cur_pos.y },
            radiusX: base_r,
            radiusY: base_r,
        };
        brush.SetOpacity(0.95);
        self.d2d_context.FillEllipse(&dot_ellipse, &brush);

        Ok(())
    }

    /// Build a single connected path geometry for a polyline stroke using Catmull-Rom cubic Bezier spline
    unsafe fn build_polyline_geometry(&self, points: &[Point2D]) -> Result<ID2D1PathGeometry> {
        let path: ID2D1PathGeometry = self.d2d_factory.CreatePathGeometry()?.into();
        let sink = path.Open()?;
        sink.BeginFigure(
            D2D_POINT_2F { x: points[0].x, y: points[0].y },
            D2D1_FIGURE_BEGIN_HOLLOW,
        );

        let n = points.len();
        if n == 2 {
            sink.AddLine(D2D_POINT_2F { x: points[1].x, y: points[1].y });
        } else {
            for i in 0..(n - 1) {
                let p0 = if i > 0 { points[i - 1] } else { points[i] };
                let p1 = points[i];
                let p2 = points[i + 1];
                let p3 = if i + 2 < n { points[i + 2] } else { p2 };

                let c1 = D2D_POINT_2F {
                    x: p1.x + (p2.x - p0.x) / 6.0,
                    y: p1.y + (p2.y - p0.y) / 6.0,
                };
                let c2 = D2D_POINT_2F {
                    x: p2.x - (p3.x - p1.x) / 6.0,
                    y: p2.y - (p3.y - p1.y) / 6.0,
                };
                let seg = D2D1_BEZIER_SEGMENT {
                    point1: c1,
                    point2: c2,
                    point3: D2D_POINT_2F { x: p2.x, y: p2.y },
                };
                sink.AddBezier(&seg);
            }
        }

        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        Ok(path)
    }

    /// Draw a smooth continuous-phase sine wave segment connecting `start` to `end`
    unsafe fn draw_wavy_segment(
        &self,
        start: Point2D,
        end: Point2D,
        brush: &ID2D1SolidColorBrush,
        width: f32,
        taper_at_end: bool,
    ) -> Result<()> {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let len = dx.hypot(dy);
        if len < 3.0 {
            return Ok(());
        }
        let ux = dx / len;
        let uy = dy / len;
        let nx = -uy;
        let ny = ux;

        if len <= 12.0 {
            self.d2d_context.DrawLine(
                D2D_POINT_2F { x: start.x, y: start.y },
                D2D_POINT_2F { x: end.x, y: end.y },
                brush,
                width,
                Some(&self.round_stroke_style),
            );
            return Ok(());
        }

        // Fixed spatial wavelength so wave frequency does not jump or stretch as user drags
        let wavelength = 26.0f32;
        let amplitude = (width * 1.5 + 4.0).clamp(5.0, 14.0);

        // Sample points closely (~2px apart) for high visual fidelity
        let steps = ((len / 2.0).ceil() as usize).clamp(24, 600);
        let mut pts = Vec::with_capacity(steps + 1);

        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let s = t * len;
            let along_x = start.x + ux * s;
            let along_y = start.y + uy * s;

            // Smooth cubic smoothstep entry at start
            let f_in = (s / 18.0).clamp(0.0, 1.0);
            let f_in_smooth = f_in * f_in * (3.0 - 2.0 * f_in);

            // Smooth cubic smoothstep exit at end
            let d_end = len - s;
            let f_out = if taper_at_end {
                let f = (d_end / 22.0).clamp(0.0, 1.0);
                f * f * (3.0 - 2.0 * f)
            } else {
                let f = (d_end / 18.0).clamp(0.0, 1.0);
                f * f * (3.0 - 2.0 * f)
            };

            let fade = f_in_smooth * f_out;
            let wave = (s * std::f32::consts::PI * 2.0 / wavelength).sin() * amplitude * fade;

            pts.push(D2D_POINT_2F {
                x: along_x + nx * wave,
                y: along_y + ny * wave,
            });
        }

        if let Ok(stem_geo) = self.d2d_factory.CreatePathGeometry() {
            if let Ok(sink) = stem_geo.Open() {
                sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_HOLLOW);
                for pt in &pts[1..] {
                    sink.AddLine(*pt);
                }
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                let _ = sink.Close();
                self.d2d_context.DrawGeometry(&stem_geo, brush, width, Some(&self.round_stroke_style));
            }
        }

        Ok(())
    }

    /// Draw a completed or in-progress shape
    unsafe fn draw_shape(&self, shape: &Shape, opacity: f32) -> Result<()> {
        match shape {
            Shape::Freehand { points, color, width } => {
                if points.len() < 2 {
                    return Ok(());
                }
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;

                if points.len() == 2 {
                    self.d2d_context.DrawLine(
                        D2D_POINT_2F { x: points[0].x, y: points[0].y },
                        D2D_POINT_2F { x: points[1].x, y: points[1].y },
                        &brush,
                        *width,
                        Some(&self.round_stroke_style),
                    );
                } else if let Ok(path) = self.build_polyline_geometry(points) {
                    self.d2d_context.DrawGeometry(&path, &brush, *width, Some(&self.round_stroke_style));
                } else {
                    for window in points.windows(2) {
                        self.d2d_context.DrawLine(
                            D2D_POINT_2F { x: window[0].x, y: window[0].y },
                            D2D_POINT_2F { x: window[1].x, y: window[1].y },
                            &brush,
                            *width,
                            Some(&self.round_stroke_style),
                        );
                    }
                }
            }
            Shape::Line { start, end, color, width, dotted, wavy } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;

                if *wavy {
                    self.draw_wavy_segment(*start, *end, &brush, *width, false)?;
                } else {
                    let stroke_style = if *dotted {
                        Some(&self.dotted_stroke_style)
                    } else {
                        Some(&self.round_stroke_style)
                    };

                    self.d2d_context.DrawLine(
                        D2D_POINT_2F { x: start.x, y: start.y },
                        D2D_POINT_2F { x: end.x, y: end.y },
                        &brush,
                        *width,
                        stroke_style,
                    );
                }
            }
            Shape::Arrow { start, end, color, width, wavy } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;

                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let len = dx.hypot(dy);
                if len < 3.0 {
                    return Ok(());
                }
                let ux = dx / len;
                let uy = dy / len;
                let nx = -uy;
                let ny = ux;

                // Razor-sharp filled chevron arrowhead calculation
                let head_len = (*width * 3.5 + 14.0).min(len * 0.85);
                let head_width = head_len * 0.55;
                let base_x = end.x - ux * head_len;
                let base_y = end.y - uy * head_len;
                let notch_x = end.x - ux * (head_len * 0.72);
                let notch_y = end.y - uy * (head_len * 0.72);

                let tip = D2D_POINT_2F { x: end.x, y: end.y };
                let w1 = D2D_POINT_2F { x: base_x + nx * head_width, y: base_y + ny * head_width };
                let notch = D2D_POINT_2F { x: notch_x, y: notch_y };
                let w2 = D2D_POINT_2F { x: base_x - nx * head_width, y: base_y - ny * head_width };

                // Draw stem connecting start to notch
                if *wavy {
                    self.draw_wavy_segment(*start, Point2D::new(notch_x, notch_y), &brush, *width, true)?;
                } else {
                    self.d2d_context.DrawLine(
                        D2D_POINT_2F { x: start.x, y: start.y },
                        notch,
                        &brush,
                        *width,
                        Some(&self.round_stroke_style),
                    );
                }

                // Draw razor-sharp filled chevron arrowhead
                if let Ok(head_geo) = self.d2d_factory.CreatePathGeometry() {
                    if let Ok(sink) = head_geo.Open() {
                        sink.BeginFigure(tip, D2D1_FIGURE_BEGIN_FILLED);
                        sink.AddLine(w1);
                        sink.AddLine(notch);
                        sink.AddLine(w2);
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        let _ = sink.Close();
                        self.d2d_context.FillGeometry(&head_geo, &brush, None);
                    }
                }
            }
            Shape::Rect { p1, p2, color, width, fill, rotation_rad } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;

                if rotation_rad.abs() > 0.001 {
                    let cx = (p1.x + p2.x) * 0.5;
                    let cy = (p1.y + p2.y) * 0.5;
                    let hw = (p2.x - p1.x).abs() * 0.5;
                    let hh = (p2.y - p1.y).abs() * 0.5;

                    let cos_r = rotation_rad.cos();
                    let sin_r = rotation_rad.sin();

                    let corners = [
                        (-hw, -hh),
                        (hw, -hh),
                        (hw, hh),
                        (-hw, hh),
                    ];

                    let rot_pts: Vec<D2D_POINT_2F> = corners.iter().map(|(x, y)| {
                        D2D_POINT_2F {
                            x: cx + x * cos_r - y * sin_r,
                            y: cy + x * sin_r + y * cos_r,
                        }
                    }).collect();

                    if let Ok(rect_geo) = self.d2d_factory.CreatePathGeometry() {
                        if let Ok(sink) = rect_geo.Open() {
                            sink.BeginFigure(rot_pts[0], if *fill { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                            for pt in &rot_pts[1..] {
                                sink.AddLine(*pt);
                            }
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();

                            if *fill {
                                self.d2d_context.FillGeometry(&rect_geo, &brush, None);
                            } else {
                                self.d2d_context.DrawGeometry(&rect_geo, &brush, *width, None);
                            }
                        }
                    }
                } else {
                    let rect = D2D_RECT_F {
                        left: p1.x.min(p2.x),
                        top: p1.y.min(p2.y),
                        right: p1.x.max(p2.x),
                        bottom: p1.y.max(p2.y),
                    };
                    if *fill {
                        self.d2d_context.FillRectangle(&rect, &brush);
                    } else {
                        self.d2d_context.DrawRectangle(&rect, &brush, *width, None);
                    }
                }
            }
            Shape::Ellipse { center, rx, ry, color, width, fill } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;
                let ellipse = D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: center.x, y: center.y },
                    radiusX: *rx,
                    radiusY: *ry,
                };
                if *fill {
                    self.d2d_context.FillEllipse(&ellipse, &brush);
                } else {
                    self.d2d_context.DrawEllipse(&ellipse, &brush, *width, None);
                }
            }
            Shape::Emoji { pos, emoji, size, opacity: shape_op } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: *shape_op * opacity },
                    None,
                )?;

                let wide_text: Vec<u16> = emoji.encode_utf16().collect();
                let s = (*size / 32.0).clamp(0.2, 5.0);
                let transform = windows::Foundation::Numerics::Matrix3x2 {
                    M11: s,
                    M12: 0.0,
                    M21: 0.0,
                    M22: s,
                    M31: pos.x * (1.0 - s),
                    M32: pos.y * (1.0 - s),
                };
                let identity = windows::Foundation::Numerics::Matrix3x2 {
                    M11: 1.0,
                    M12: 0.0,
                    M21: 0.0,
                    M22: 1.0,
                    M31: 0.0,
                    M32: 0.0,
                };

                self.d2d_context.SetTransform(&transform);
                let base_half = 20.0;
                let rect = D2D_RECT_F {
                    left: pos.x - base_half,
                    top: pos.y - base_half,
                    right: pos.x + base_half,
                    bottom: pos.y + base_half,
                };

                self.d2d_context.DrawText(
                    &wide_text,
                    &self.text_format_emoji,
                    &rect,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.d2d_context.SetTransform(&identity);
            }
            Shape::Text { pos, text, font_family, font_size, color } => {
                let brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3] * opacity,
                    },
                    None,
                )?;
                let wide_font: Vec<u16> = font_family.encode_utf16().chain(std::iter::once(0)).collect();
                let wide_locale: Vec<u16> = "en-us".encode_utf16().chain(std::iter::once(0)).collect();
                if let Ok(format) = self.dwrite_factory.CreateTextFormat(
                    PCWSTR(wide_font.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    *font_size,
                    PCWSTR(wide_locale.as_ptr()),
                ) {
                    let wide_text: Vec<u16> = text.encode_utf16().collect();
                    let approx_w = (text.len() as f32 * font_size * 0.8).max(500.0);
                    let approx_h = (font_size * 2.0).max(100.0);
                    let rect = D2D_RECT_F {
                        left: pos.x,
                        top: pos.y,
                        right: pos.x + approx_w,
                        bottom: pos.y + approx_h,
                    };
                    self.d2d_context.DrawText(
                        &wide_text,
                        &format,
                        &rect,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }
        Ok(())
    }

    /// Draw Object Eraser Target Cue around cursor
    unsafe fn draw_eraser_indicator(&self, pos: Point2D) -> Result<()> {
        let border_brush = self.render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.35, a: 0.90 },
            None,
        )?;
        let fill_brush = self.render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.35, a: 0.15 },
            None,
        )?;
        let r = 14.0;
        let ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: pos.x, y: pos.y },
            radiusX: r,
            radiusY: r,
        };
        self.d2d_context.FillEllipse(&ellipse, &fill_brush);
        self.d2d_context.DrawEllipse(&ellipse, &border_brush, 2.0, None);

        // Center crosshair
        let len = 5.0;
        self.d2d_context.DrawLine(
            D2D_POINT_2F { x: pos.x - len, y: pos.y },
            D2D_POINT_2F { x: pos.x + len, y: pos.y },
            &border_brush,
            1.5,
            None,
        );
        self.d2d_context.DrawLine(
            D2D_POINT_2F { x: pos.x, y: pos.y - len },
            D2D_POINT_2F { x: pos.x, y: pos.y + len },
            &border_brush,
            1.5,
            None,
        );
        Ok(())
    }

    /// Draw Keystroke OSD (Key Chords with fade, rounded rect, customizable colors & optional bg image)
    unsafe fn draw_osd(&mut self, tokens: &[KeyDisplayToken], config: &OsdConfig, cursor_screen: Point2D) -> Result<()> {
        let pt = POINT {
            x: cursor_screen.x as i32,
            y: cursor_screen.y as i32,
        };
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let (mon_left, mon_top, mon_w, mon_h) = if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            let w = (mi.rcWork.right - mi.rcWork.left) as f32;
            let h = (mi.rcWork.bottom - mi.rcWork.top) as f32;
            (mi.rcWork.left as f32, mi.rcWork.top as f32, w, h)
        } else {
            let w = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as f32;
            let h = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as f32;
            (0.0, 0.0, w, h)
        };

        let screen_target_x = mon_left + mon_w * config.anchor_x;
        let screen_target_y = mon_top + mon_h * config.anchor_y;
        let client_target = self.screen_to_client(Point2D::new(screen_target_x, screen_target_y));

        let center_x = client_target.x;
        let bottom_y = client_target.y;

        let padding_h = 22.0;
        let badge_spacing = 10.0;

        // Calculate layout for active tokens
        let mut total_width = 0.0f32;
        let mut badge_widths = Vec::new();

        for token in tokens {
            let wide: Vec<u16> = token.text.encode_utf16().collect();
            let text_layout = self.dwrite_factory.CreateTextLayout(
                &wide,
                &self.text_format_osd,
                800.0,
                100.0,
            ).ok();

            let mut w = 60.0;
            if let Some(tl) = text_layout {
                let mut metrics = DWRITE_TEXT_METRICS::default();
                if tl.GetMetrics(&mut metrics).is_ok() {
                    w = (metrics.width + padding_h * 2.0).max(50.0);
                }
            }
            badge_widths.push(w);
            total_width += w + badge_spacing;
        }
        total_width -= badge_spacing;

        let mut start_x = center_x - total_width * 0.5;
        let badge_height = 54.0;
        let top_y = bottom_y - badge_height;

        for (i, token) in tokens.iter().enumerate() {
            let alpha = token.alpha(config.stay_duration_ms, config.fade_duration_ms);
            if alpha <= 0.01 {
                continue;
            }

            let w = badge_widths[i];
            let rect = D2D_RECT_F {
                left: start_x,
                top: top_y,
                right: start_x + w,
                bottom: top_y + badge_height,
            };
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: config.border_radius,
                radiusY: config.border_radius,
            };

            // Background: Solid or Image
            if let Some(ref bmp) = self.osd_bg_bitmap {
                self.render_target.DrawBitmap(
                    bmp,
                    Some(&rect),
                    alpha * config.bg_color[3],
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
            } else {
                let bg_brush = self.render_target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: config.bg_color[0],
                        g: config.bg_color[1],
                        b: config.bg_color[2],
                        a: config.bg_color[3] * alpha,
                    },
                    None,
                )?;
                self.d2d_context.FillRoundedRectangle(&rounded, &bg_brush);
            }

            // Outline / Drop glow
            let border_brush = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.18 * alpha,
                },
                None,
            )?;
            self.d2d_context.DrawRoundedRectangle(&rounded, &border_brush, 1.5, None);

            // Text
            let text_brush = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: config.fg_color[0],
                    g: config.fg_color[1],
                    b: config.fg_color[2],
                    a: config.fg_color[3] * alpha,
                },
                None,
            )?;

            let wide_text: Vec<u16> = token.text.encode_utf16().collect();
            self.d2d_context.DrawText(
                &wide_text,
                &self.text_format_osd,
                &rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            start_x += w + badge_spacing;
        }

        Ok(())
    }
}
