use crate::annotation::{
    ActiveTool, AnnotationManager, LaserTrailPoint, ScreenEffectMode, Shape,
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
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{
    MONITORINFOF_PRIMARY, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

#[derive(Debug, Clone)]
pub struct MonitorInfoEntry {
    pub index: usize,
    pub is_primary: bool,
    pub work_rect: RECT,
}

unsafe extern "system" fn enum_osd_monitors_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let entries = &mut *(lparam.0 as *mut Vec<MonitorInfoEntry>);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
        let index = entries.len();
        entries.push(MonitorInfoEntry {
            index,
            is_primary: (mi.dwFlags & MONITORINFOF_PRIMARY) != 0,
            work_rect: mi.rcWork,
        });
    }
    BOOL(1)
}

pub fn enumerate_monitors() -> Vec<MonitorInfoEntry> {
    let mut entries = Vec::new();
    unsafe {
        let lparam = LPARAM(&mut entries as *mut _ as isize);
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(enum_osd_monitors_proc), lparam);
    }
    entries
}

pub fn compute_osd_origin(
    mon_left: f32,
    mon_top: f32,
    mon_w: f32,
    mon_h: f32,
    total_width: f32,
    badge_height: f32,
    position: &str,
    edge_offset: f32,
) -> (f32, f32) {
    let offset = edge_offset.max(0.0);
    let min_x = mon_left;
    let max_x = (mon_left + mon_w - total_width).max(min_x);
    let min_y = mon_top;
    let max_y = (mon_top + mon_h - badge_height).max(min_y);

    let start_x = match position {
        "top_left" | "middle_left" | "bottom_left" => mon_left + offset,
        "top_right" | "middle_right" | "bottom_right" => mon_left + mon_w - offset - total_width,
        _ => mon_left + (mon_w - total_width) * 0.5,
    }
    .clamp(min_x, max_x);

    let top_y = match position {
        "top_left" | "top_center" | "top_right" => mon_top + offset,
        "middle_left" | "center" | "middle_right" => mon_top + (mon_h - badge_height) * 0.5,
        _ => mon_top + mon_h - offset - badge_height,
    }
    .clamp(min_y, max_y);

    (start_x, top_y)
}

pub struct DesktopSnapshot {
    pub id: usize,
    pub _source_bitmap: ID2D1Bitmap,
    pub invert_effect: ID2D1Effect,
    pub burn_effect: ID2D1Effect,
    pub dodge_effect: ID2D1Effect,
}

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
    osd_font_size: f32,
    text_format_emoji: IDWriteTextFormat,
    text_format_toolbar: IDWriteTextFormat,
    text_format_toolbar_small: IDWriteTextFormat,
    pub round_stroke_style: ID2D1StrokeStyle,
    pub dotted_stroke_style: ID2D1StrokeStyle,
    cached_dash_length: f32,

    osd_bg_bitmap: Option<ID2D1Bitmap>,
    loaded_bg_path: Option<String>,

    snapshots: Vec<DesktopSnapshot>,
    next_snapshot_id: usize,
    pub active_snapshot_id: usize,

    accum_bitmap: Option<ID2D1Bitmap1>,
    effect_scratch_bitmap: Option<ID2D1Bitmap1>,
    blur_effect: Option<ID2D1Effect>,
    mask_composite_effect: Option<ID2D1Effect>,
    frame_invert_effect: Option<ID2D1Effect>,
    frame_burn_effect: Option<ID2D1Effect>,
    frame_dodge_effect: Option<ID2D1Effect>,

    live_source_bitmap: Option<ID2D1Bitmap>,
    live_invert_effect: Option<ID2D1Effect>,
    live_mem_dc: HDC,
    live_hbmp: HBITMAP,
    live_bits: *mut core::ffi::c_void,
    live_shadow_buf: Vec<u8>,
    live_last_rect: Option<(u32, u32, u32, u32)>,
}

impl Drop for D2DRenderer {
    fn drop(&mut self) {
        unsafe {
            if !self.live_hbmp.is_invalid() {
                let _ = DeleteObject(self.live_hbmp);
            }
            if !self.live_mem_dc.is_invalid() {
                let _ = DeleteDC(self.live_mem_dc);
            }
        }
    }
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
                osd_font_size: 26.0,
                text_format_emoji,
                text_format_toolbar,
                text_format_toolbar_small,
                round_stroke_style,
                dotted_stroke_style,
                cached_dash_length: 3.0,
                osd_bg_bitmap: None,
                loaded_bg_path: None,
                snapshots: Vec::new(),
                next_snapshot_id: 1,
                active_snapshot_id: 0,
                accum_bitmap: None,
                effect_scratch_bitmap: None,
                blur_effect: None,
                mask_composite_effect: None,
                frame_invert_effect: None,
                frame_burn_effect: None,
                frame_dodge_effect: None,
                live_source_bitmap: None,
                live_invert_effect: None,
                live_mem_dc: HDC::default(),
                live_hbmp: HBITMAP::default(),
                live_bits: std::ptr::null_mut(),
                live_shadow_buf: Vec::new(),
                live_last_rect: None,
            })
        }
    }

    pub fn invalidate_active_snapshot(&mut self) {
        self.active_snapshot_id = 0;
    }

    pub fn prune_snapshots(&mut self, keep_ids: &[usize]) {
        self.snapshots.retain(|s| s.id == self.active_snapshot_id || keep_ids.contains(&s.id));
    }

    /// Capture raw underlying desktop (temporarily excluding our overlay window) and build GPU effects
    pub fn capture_desktop_snapshot(&mut self, hwnd_exclude: HWND) -> usize {
        unsafe {
            let w = self._width as i32;
            let h = self._height as i32;
            if w <= 0 || h <= 0 {
                return 0;
            }

            let aff_ok = SetWindowDisplayAffinity(hwnd_exclude, WDA_EXCLUDEFROMCAPTURE).is_ok();
            if aff_ok {
                let _ = DwmFlush();
            }

            let screen_dc = GetDC(HWND::default());
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down BGRA
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbitmap = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0);
            let mut captured = false;
            if let Ok(hbmp) = hbitmap {
                if !hbmp.is_invalid() && !bits_ptr.is_null() {
                    let old_obj = SelectObject(mem_dc, hbmp);
                    captured = BitBlt(
                        mem_dc,
                        0,
                        0,
                        w,
                        h,
                        screen_dc,
                        self.offset_x as i32,
                        self.offset_y as i32,
                        SRCCOPY,
                    )
                    .is_ok();
                    SelectObject(mem_dc, old_obj);

                    if aff_ok {
                        let _ = SetWindowDisplayAffinity(hwnd_exclude, WDA_NONE);
                    }

                    if captured {
                        let byte_len = (self._width as usize) * (self._height as usize) * 4;
                        let buf = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, byte_len);
                        for px in buf.chunks_exact_mut(4) {
                            px[3] = 255;
                        }

                        let props = D2D1_BITMAP_PROPERTIES {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                            },
                            dpiX: 96.0,
                            dpiY: 96.0,
                        };
                        if let Ok(source_bitmap) = self.render_target.CreateBitmap(
                            D2D_SIZE_U { width: self._width, height: self._height },
                            Some(bits_ptr as *const core::ffi::c_void),
                            self._width * 4,
                            &props,
                        ) {
                            if let Ok(snapshot) = self.build_snapshot_effects(source_bitmap) {
                                let id = snapshot.id;
                                self.snapshots.push(snapshot);
                                self.active_snapshot_id = id;
                                let _ = DeleteObject(hbmp);
                                let _ = DeleteDC(mem_dc);
                                let _ = ReleaseDC(HWND::default(), screen_dc);
                                return id;
                            }
                        }
                    }
                    let _ = DeleteObject(hbmp);
                }
            }
            if aff_ok && !captured {
                let _ = SetWindowDisplayAffinity(hwnd_exclude, WDA_NONE);
            }

            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
            let _ = captured;
            0
        }
    }

    unsafe fn build_snapshot_effects(&mut self, source_bitmap: ID2D1Bitmap) -> Result<DesktopSnapshot> {
        let src_img: ID2D1Image = source_bitmap.cast()?;

        let invert_effect = self.d2d_context.CreateEffect(&CLSID_D2D1Invert)?;
        invert_effect.SetInput(0, &src_img, true);

        let burn_effect = Self::create_exposure_lut_effect(&self.d2d_context, &src_img, true)?;
        let dodge_effect = Self::create_exposure_lut_effect(&self.d2d_context, &src_img, false)?;

        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;

        Ok(DesktopSnapshot {
            id,
            _source_bitmap: source_bitmap,
            invert_effect,
            burn_effect,
            dodge_effect,
        })
    }

    unsafe fn create_exposure_lut_effect(
        ctx: &ID2D1DeviceContext,
        src_img: &ID2D1Image,
        is_burn: bool,
    ) -> Result<ID2D1Effect> {
        let effect = ctx.CreateEffect(&CLSID_D2D1TableTransfer)?;
        effect.SetInput(0, src_img, true);

        let mut lut = [0.0f32; 64];
        for (i, slot) in lut.iter_mut().enumerate() {
            let x = i as f32 / 63.0;
            *slot = if is_burn {
                (0.55 * x.powf(1.6)).clamp(0.0, 1.0)
            } else {
                (1.0 - 0.55 * (1.0 - x).powf(1.6)).clamp(0.0, 1.0)
            };
        }

        let lut_bytes = std::slice::from_raw_parts(
            lut.as_ptr() as *const u8,
            std::mem::size_of_val(&lut),
        );
        effect.SetValue(D2D1_TABLETRANSFER_PROP_RED_TABLE.0 as u32, D2D1_PROPERTY_TYPE_BLOB, lut_bytes)?;
        effect.SetValue(D2D1_TABLETRANSFER_PROP_GREEN_TABLE.0 as u32, D2D1_PROPERTY_TYPE_BLOB, lut_bytes)?;
        effect.SetValue(D2D1_TABLETRANSFER_PROP_BLUE_TABLE.0 as u32, D2D1_PROPERTY_TYPE_BLOB, lut_bytes)?;
        let true_bytes = 1i32.to_le_bytes();
        effect.SetValue(D2D1_TABLETRANSFER_PROP_ALPHA_DISABLE.0 as u32, D2D1_PROPERTY_TYPE_BOOL, &true_bytes)?;
        Ok(effect)
    }

    unsafe fn ensure_blur_effect(&mut self) -> Result<()> {
        if self.blur_effect.is_none() {
            self.blur_effect = Some(self.d2d_context.CreateEffect(&CLSID_D2D1GaussianBlur)?);
        }
        Ok(())
    }

    unsafe fn ensure_effect_resources(&mut self) -> Result<()> {
        self.ensure_blur_effect()?;
        if self.accum_bitmap.is_none() {
            let accum_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                colorContext: ManuallyDrop::new(None),
            };
            self.accum_bitmap = Some(self.d2d_context.CreateBitmap(
                D2D_SIZE_U { width: self._width, height: self._height },
                None,
                0,
                &accum_props,
            )?);
        }

        if self.effect_scratch_bitmap.is_none() {
            let scratch_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                colorContext: ManuallyDrop::new(None),
            };
            let scratch = self.d2d_context.CreateBitmap(
                D2D_SIZE_U { width: self._width, height: self._height },
                None,
                0,
                &scratch_props,
            )?;
            let scratch_img: ID2D1Image = scratch.cast()?;

            let inv = self.d2d_context.CreateEffect(&CLSID_D2D1Invert)?;
            inv.SetInput(0, &scratch_img, true);

            let burn = Self::create_exposure_lut_effect(&self.d2d_context, &scratch_img, true)?;
            let dodge = Self::create_exposure_lut_effect(&self.d2d_context, &scratch_img, false)?;

            let comp = self.d2d_context.CreateEffect(&CLSID_D2D1Composite)?;
            let mode_bytes = (D2D1_COMPOSITE_MODE_DESTINATION_IN.0 as u32).to_le_bytes();
            comp.SetValue(D2D1_COMPOSITE_PROP_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &mode_bytes)?;

            self.effect_scratch_bitmap = Some(scratch);
            self.frame_invert_effect = Some(inv);
            self.frame_burn_effect = Some(burn);
            self.frame_dodge_effect = Some(dodge);
            self.mask_composite_effect = Some(comp);
        }
        Ok(())
    }

    fn shape_effect_and_softness(shape: &Shape) -> (ScreenEffectMode, usize, f32) {
        match shape {
            Shape::Freehand { effect, snapshot_id, softness, .. } => (*effect, *snapshot_id, *softness),
            Shape::Line { effect, snapshot_id, .. }
            | Shape::Arrow { effect, snapshot_id, .. }
            | Shape::Rect { effect, snapshot_id, .. }
            | Shape::Ellipse { effect, snapshot_id, .. }
            | Shape::Emoji { effect, snapshot_id, .. }
            | Shape::Text { effect, snapshot_id, .. } => (*effect, *snapshot_id, 0.0),
        }
    }

    fn shape_stroke_pad(shape: &Shape) -> f32 {
        match shape {
            Shape::Freehand { width, .. }
            | Shape::Line { width, .. }
            | Shape::Rect { width, .. }
            | Shape::Ellipse { width, .. } => width * 0.6 + 8.0,
            Shape::Arrow { width, .. } => width * 3.5 + 18.0,
            Shape::Emoji { .. } | Shape::Text { .. } => 12.0,
        }
    }

    fn shape_dirty_rect(&self, shape: &Shape, extra_pad: f32) -> D2D_RECT_F {
        let (min_pt, max_pt) = shape.get_bounding_box();
        let pad = Self::shape_stroke_pad(shape) + extra_pad;
        let max_w = self._width as f32;
        let max_h = self._height as f32;
        D2D_RECT_F {
            left: (min_pt.x - pad).floor().clamp(0.0, max_w),
            top: (min_pt.y - pad).floor().clamp(0.0, max_h),
            right: (max_pt.x + pad).ceil().clamp(0.0, max_w),
            bottom: (max_pt.y + pad).ceil().clamp(0.0, max_h),
        }
    }

    /// Compute the tight integer union bounding box of all visible effect shapes (Invert/Burn/Dodge)
    pub fn effect_union_rect(&self, annotation_mgr: &AnnotationManager) -> Option<(u32, u32, u32, u32)> {
        if annotation_mgr.annotations_hidden {
            return None;
        }
        let mut min_l = u32::MAX;
        let mut min_t = u32::MAX;
        let mut max_r = 0u32;
        let mut max_b = 0u32;

        let mut accumulate = |shape: &Shape| {
            let (eff, _sid, soft) = Self::shape_effect_and_softness(shape);
            if eff == ScreenEffectMode::Normal {
                return;
            }
            let sigma = soft.clamp(0.0, 1.0) * 14.0;
            let dirty = self.shape_dirty_rect(shape, sigma * 3.0 + 8.0);
            let l = dirty.left.floor().max(0.0) as u32;
            let t = dirty.top.floor().max(0.0) as u32;
            let r = dirty.right.ceil().min(self._width as f32) as u32;
            let b = dirty.bottom.ceil().min(self._height as f32) as u32;
            if r > l && b > t {
                min_l = min_l.min(l);
                min_t = min_t.min(t);
                max_r = max_r.max(r);
                max_b = max_b.max(b);
            }
        };

        for s in &annotation_mgr.shapes {
            accumulate(s);
        }
        if let Some(ref in_prog) = annotation_mgr.in_progress {
            accumulate(in_prog);
        }
        if let Some(ref txt) = annotation_mgr.active_text {
            if txt.effect != ScreenEffectMode::Normal {
                let s = Shape::Text {
                    pos: txt.pos,
                    text: txt.text.clone(),
                    font_family: txt.font_family.clone(),
                    font_size: txt.font_size,
                    color: txt.color,
                    rotation_rad: 0.0,
                    effect: txt.effect,
                    snapshot_id: txt.snapshot_id,
                };
                accumulate(&s);
            }
        }

        if max_r > min_l && max_b > min_t {
            Some((min_l, min_t, max_r, max_b))
        } else {
            None
        }
    }

    unsafe fn ensure_live_capture_resources(&mut self) -> bool {
        if self.live_source_bitmap.is_some() && !self.live_hbmp.is_invalid() && !self.live_bits.is_null() {
            return true;
        }
        let w = self._width as i32;
        let h = self._height as i32;
        if w <= 0 || h <= 0 {
            return false;
        }

        if self.live_mem_dc.is_invalid() {
            let screen_dc = GetDC(HWND::default());
            self.live_mem_dc = CreateCompatibleDC(screen_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        if self.live_hbmp.is_invalid() {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            if let Ok(hbmp) = CreateDIBSection(self.live_mem_dc, &bmi, DIB_RGB_COLORS, &mut self.live_bits, None, 0) {
                self.live_hbmp = hbmp;
            }
        }
        if self.live_hbmp.is_invalid() || self.live_bits.is_null() {
            return false;
        }

        if self.live_shadow_buf.len() != (self._width as usize) * (self._height as usize) * 4 {
            self.live_shadow_buf = vec![0u8; (self._width as usize) * (self._height as usize) * 4];
        }

        if self.live_source_bitmap.is_none() {
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            if let Ok(bmp) = self.render_target.CreateBitmap(
                D2D_SIZE_U { width: self._width, height: self._height },
                None,
                0,
                &props,
            ) {
                if let Ok(inv) = self.d2d_context.CreateEffect(&CLSID_D2D1Invert) {
                    if let Ok(img) = bmp.cast::<ID2D1Image>() {
                        inv.SetInput(0, &img, true);
                        self.live_invert_effect = Some(inv);
                    }
                }
                self.live_source_bitmap = Some(bmp);
            }
        }

        self.live_source_bitmap.is_some()
    }

    pub fn has_live_rect_covering(&self, rect: (u32, u32, u32, u32)) -> bool {
        if let Some((ll, lt, lr, lb)) = self.live_last_rect {
            rect.0 >= ll && rect.1 >= lt && rect.2 <= lr && rect.3 <= lb
        } else {
            false
        }
    }

    /// Incrementally refresh the underlying screen region for live Invert/Burn/Dodge shapes.
    /// Returns true when pixels changed and a re-render is required.
    pub fn refresh_live_effect_backdrop(
        &mut self,
        hwnd_exclude: HWND,
        rect: (u32, u32, u32, u32),
        force: bool,
    ) -> bool {
        let cap_rect = if !force && self.has_live_rect_covering(rect) {
            rect
        } else {
            (
                rect.0.saturating_sub(96),
                rect.1.saturating_sub(96),
                (rect.2 + 96).min(self._width),
                (rect.3 + 96).min(self._height),
            )
        };
        let (left, top, right, bottom) = cap_rect;
        if right <= left || bottom <= top || right > self._width || bottom > self._height {
            return false;
        }
        unsafe {
            if !self.ensure_live_capture_resources() {
                return false;
            }

            let aff_ok = SetWindowDisplayAffinity(hwnd_exclude, WDA_EXCLUDEFROMCAPTURE).is_ok();
            if aff_ok {
                let _ = DwmFlush();
            }

            let screen_dc = GetDC(HWND::default());
            let old_obj = SelectObject(self.live_mem_dc, self.live_hbmp);
            let rw = (right - left) as i32;
            let rh = (bottom - top) as i32;
            let sx = self.offset_x as i32 + left as i32;
            let sy = self.offset_y as i32 + top as i32;
            let blt_ok = BitBlt(
                self.live_mem_dc,
                left as i32,
                top as i32,
                rw,
                rh,
                screen_dc,
                sx,
                sy,
                SRCCOPY,
            )
            .is_ok();
            SelectObject(self.live_mem_dc, old_obj);
            let _ = ReleaseDC(HWND::default(), screen_dc);

            if aff_ok {
                let _ = SetWindowDisplayAffinity(hwnd_exclude, WDA_NONE);
            }

            if !blt_ok {
                return false;
            }

            let total_w = self._width as usize;
            let total_h = self._height as usize;
            let total_bytes = total_w * total_h * 4;
            let bits = std::slice::from_raw_parts_mut(self.live_bits as *mut u8, total_bytes);
            let row_byte_len = (right - left) as usize * 4;
            let start_col_byte = left as usize * 4;

            let mut changed = force || !self.has_live_rect_covering(rect);
            for row in top as usize..bottom as usize {
                let row_off = row * total_w * 4 + start_col_byte;
                let src_row = &mut bits[row_off..row_off + row_byte_len];
                for px in src_row.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                let dst_row = &mut self.live_shadow_buf[row_off..row_off + row_byte_len];
                if src_row != dst_row {
                    changed = true;
                    dst_row.copy_from_slice(src_row);
                }
            }

            if !changed {
                return false;
            }

            if !self.has_live_rect_covering(cap_rect) || self.live_last_rect.is_none() {
                self.live_last_rect = Some(cap_rect);
            }
            if let Some(ref live_bmp) = self.live_source_bitmap {
                let dest_u = D2D_RECT_U { left, top, right, bottom };
                let src_ptr = (self.live_bits as *const u8)
                    .add(top as usize * total_w * 4 + start_col_byte)
                    as *const core::ffi::c_void;
                let _ = live_bmp.CopyFromMemory(Some(&dest_u), src_ptr, self._width * 4);
            }
            true
        }
    }

    pub fn update_dashed_stroke_style(&mut self, dash_length: f32) {
        if (self.cached_dash_length - dash_length).abs() < 0.05 {
            return;
        }
        self.cached_dash_length = dash_length;
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
        let dashes = [dash_length.max(0.0), 2.2f32 + dash_length * 0.3];
        if let Ok(new_style) = unsafe { self.d2d_factory.CreateStrokeStyle(&dotted_props, Some(&dashes)) } {
            self.dotted_stroke_style = new_style.into();
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

        self.update_dashed_stroke_style(config.annotation.dash_length);

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

            // 3. Completed Annotation Shapes & 4. In-progress / Active Text
            if !annotation_mgr.annotations_hidden {
                let first_effect_snap_id = annotation_mgr
                    .shapes
                    .iter()
                    .chain(annotation_mgr.in_progress.iter())
                    .find_map(|s| {
                        let (eff, sid, _) = Self::shape_effect_and_softness(s);
                        if eff != ScreenEffectMode::Normal {
                            Some(sid)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        annotation_mgr.active_text.as_ref().and_then(|t| {
                            if t.effect != ScreenEffectMode::Normal {
                                Some(t.snapshot_id)
                            } else {
                                None
                            }
                        })
                    });

                let has_effect_shapes = first_effect_snap_id.is_some();
                if has_effect_shapes {
                    self.ensure_effect_resources()?;
                    let target_sid = first_effect_snap_id.unwrap_or(self.active_snapshot_id);
                    if let Some(ref live_bmp) = self.live_source_bitmap {
                        if let Some(ref accum_bmp) = self.accum_bitmap {
                            let _ = accum_bmp.CopyFromBitmap(None, live_bmp, None);
                        }
                    } else if let Some(snap) = self
                        .snapshots
                        .iter()
                        .find(|s| s.id == target_sid)
                        .or_else(|| self.snapshots.last())
                    {
                        if let Some(ref accum_bmp) = self.accum_bitmap {
                            let _ = accum_bmp.CopyFromBitmap(None, &snap._source_bitmap, None);
                        }
                    }
                }

                let main_target: ID2D1Image = self.d2d_context.GetTarget()?;

                for shape in &annotation_mgr.shapes {
                    self.draw_shape_composited(shape, 1.0, &main_target, has_effect_shapes)?;
                }

                // 4. In-progress Shape (Live Preview)
                if let Some(ref in_prog) = annotation_mgr.in_progress {
                    self.draw_shape_composited(in_prog, 0.85, &main_target, has_effect_shapes)?;
                }

                // 4b. Active Text Input (Typing live preview with blinking caret)
                if let Some(ref text_input) = annotation_mgr.active_text {
                    let text_shape = Shape::Text {
                        pos: text_input.pos,
                        text: text_input.text.clone(),
                        font_family: text_input.font_family.clone(),
                        font_size: text_input.font_size,
                        color: text_input.color,
                        rotation_rad: 0.0,
                        effect: text_input.effect,
                        snapshot_id: text_input.snapshot_id,
                    };
                    self.draw_shape_composited(&text_shape, 1.0, &main_target, has_effect_shapes)?;

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
            }

            // 4c. Transform Tool: Marquee Box, Selection Box, Handles, and Hover Frames
            if !annotation_mgr.annotations_hidden && is_drawing_mode && active_tool == ActiveTool::Transform {
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
                if let Some(obb) = annotation_mgr.get_selection_box() {
                    let pad = 6.0;
                    let hw = obb.half_w + pad;
                    let hh = obb.half_h + pad;
                    let center = obb.center;

                    let rot_a = obb.rotation_rad;
                    let is_rotated = rot_a.abs() > 0.001;
                    if is_rotated {
                        let cos_a = rot_a.cos();
                        let sin_a = rot_a.sin();
                        let rot_mat = windows::Foundation::Numerics::Matrix3x2 {
                            M11: cos_a,
                            M12: sin_a,
                            M21: -sin_a,
                            M22: cos_a,
                            M31: center.x * (1.0 - cos_a) + center.y * sin_a,
                            M32: center.y * (1.0 - cos_a) - center.x * sin_a,
                        };
                        self.d2d_context.SetTransform(&rot_mat);
                    }

                    let box_min = Point2D::new(center.x - hw, center.y - hh);
                    let box_max = Point2D::new(center.x + hw, center.y + hh);

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

                    if is_rotated {
                        let identity = windows::Foundation::Numerics::Matrix3x2 {
                            M11: 1.0, M12: 0.0,
                            M21: 0.0, M22: 1.0,
                            M31: 0.0, M32: 0.0,
                        };
                        self.d2d_context.SetTransform(&identity);
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
            if is_drawing_mode && !annotation_mgr.annotations_hidden && !toolbar.hit_test(cursor_client) {
                if active_tool == ActiveTool::Eraser {
                    self.draw_eraser_indicator(cursor_client)?;
                } else if active_tool == ActiveTool::Burn || active_tool == ActiveTool::Dodge {
                    let radius = (config.annotation.stroke_width * 0.5).max(3.0);
                    let stroke_op = config.annotation.stroke_opacity.clamp(0.05, 1.0);
                    let tint = if active_tool == ActiveTool::Burn {
                        D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.08, a: stroke_op * 0.35 }
                    } else {
                        D2D1_COLOR_F { r: 1.0, g: 0.98, b: 0.88, a: stroke_op * 0.35 }
                    };
                    let fill_brush = self.render_target.CreateSolidColorBrush(&tint, None)?;
                    let dot_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: radius,
                        radiusY: radius,
                    };
                    self.d2d_context.FillEllipse(&dot_ellipse, &fill_brush);

                    let shadow_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.70 },
                        None,
                    )?;
                    let rim_brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.90 },
                        None,
                    )?;
                    let outer_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: radius + 0.5,
                        radiusY: radius + 0.5,
                    };
                    self.d2d_context.DrawEllipse(&outer_ellipse, &shadow_brush, 1.8, None);
                    self.d2d_context.DrawEllipse(&dot_ellipse, &rim_brush, 1.0, None);
                    let pin_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: 1.0,
                        radiusY: 1.0,
                    };
                    self.d2d_context.FillEllipse(&pin_ellipse, &rim_brush);
                    if config.annotation.stroke_softness > 0.01 {
                        let soft_halo = D2D1_ELLIPSE {
                            point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                            radiusX: radius + config.annotation.stroke_softness.clamp(0.0, 1.0) * 14.0,
                            radiusY: radius + config.annotation.stroke_softness.clamp(0.0, 1.0) * 14.0,
                        };
                        let halo_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.78, g: 0.88, b: 1.0, a: 0.35 },
                            None,
                        )?;
                        self.d2d_context.DrawEllipse(&soft_halo, &halo_brush, 1.0, Some(&self.dotted_stroke_style));
                    }
                } else if active_tool == ActiveTool::Emoji {
                    // Render dynamic emoji preview with exact size and opacity
                    let min_w = config.annotation.min_stroke_width.max(0.5);
                    let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
                    let size_pct = ((config.annotation.stroke_width - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
                    let emoji_size = 20.0 + size_pct * 70.0;
                    let emoji_op = config.annotation.stroke_opacity.clamp(0.05, 1.0);
                    let color = config.annotation.current_color;
                    let effect = if config.annotation.invert_color {
                        ScreenEffectMode::Invert
                    } else {
                        ScreenEffectMode::Normal
                    };

                    let preview_shape = Shape::Emoji {
                        pos: cursor_client,
                        emoji: config.annotation.selected_emoji.clone(),
                        size: emoji_size,
                        opacity: emoji_op,
                        color: [color[0], color[1], color[2], emoji_op],
                        flat: config.annotation.emoji_style == "flat",
                        filled: config.annotation.emoji_filled,
                        rotation_rad: 0.0,
                        effect,
                        snapshot_id: self.active_snapshot_id,
                    };
                    self.draw_shape_internal(&preview_shape, 0.90, false)?;
                } else if active_tool != ActiveTool::None && active_tool != ActiveTool::Transform && active_tool != ActiveTool::Text {
                    // Render dynamic stroke dot preview with exact diameter, color, and opacity
                    let radius = (config.annotation.stroke_width * 0.5).max(1.5);
                    let stroke_col = config.annotation.current_color;
                    let stroke_op = config.annotation.stroke_opacity.clamp(0.05, 1.0);

                    let dot_ellipse = D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                        radiusX: radius,
                        radiusY: radius,
                    };
                    if config.annotation.invert_color {
                        let dark_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.11, a: stroke_op },
                            None,
                        )?;
                        let light_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.98, g: 0.98, b: 1.0, a: stroke_op },
                            None,
                        )?;
                        self.d2d_context.FillEllipse(&dot_ellipse, &dark_brush);
                        let diag = radius * std::f32::consts::FRAC_1_SQRT_2;
                        if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                            if let Ok(sink) = path.Open() {
                                sink.BeginFigure(
                                    D2D_POINT_2F { x: cursor_client.x - diag, y: cursor_client.y + diag },
                                    D2D1_FIGURE_BEGIN_FILLED,
                                );
                                sink.AddArc(&D2D1_ARC_SEGMENT {
                                    point: D2D_POINT_2F { x: cursor_client.x + diag, y: cursor_client.y - diag },
                                    size: D2D_SIZE_F { width: radius, height: radius },
                                    rotationAngle: 0.0,
                                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                    arcSize: D2D1_ARC_SIZE_SMALL,
                                });
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                let _ = sink.Close();
                                self.d2d_context.FillGeometry(&path, &light_brush, None);
                            }
                        }
                    } else {
                        let dot_fill_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: stroke_col[0], g: stroke_col[1], b: stroke_col[2], a: stroke_op },
                            None,
                        )?;
                        self.d2d_context.FillEllipse(&dot_ellipse, &dot_fill_brush);
                    }

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
                    if active_tool.supports_softness() && config.annotation.stroke_softness > 0.01 {
                        let soft_halo = D2D1_ELLIPSE {
                            point: D2D_POINT_2F { x: cursor_client.x, y: cursor_client.y },
                            radiusX: radius + config.annotation.stroke_softness.clamp(0.0, 1.0) * 14.0,
                            radiusY: radius + config.annotation.stroke_softness.clamp(0.0, 1.0) * 14.0,
                        };
                        let halo_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.78, g: 0.88, b: 1.0, a: 0.35 },
                            None,
                        )?;
                        self.d2d_context.DrawEllipse(&soft_halo, &halo_brush, 1.0, Some(&self.dotted_stroke_style));
                    }
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
                    annotation_mgr.annotations_hidden,
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
        brush: &ID2D1Brush,
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

    /// Create either a solid color brush or a GPU effect ImageBrush backed by a captured screen snapshot
    unsafe fn make_shape_brush(
        &self,
        color: [f32; 4],
        opacity: f32,
        effect: ScreenEffectMode,
        snapshot_id: usize,
        world_transform: Option<&windows::Foundation::Numerics::Matrix3x2>,
    ) -> Result<ID2D1Brush> {
        let eff_alpha = (color[3] * opacity).clamp(0.01, 1.0);
        if effect != ScreenEffectMode::Normal {
            let live_eff = if effect == ScreenEffectMode::Invert {
                self.live_invert_effect.as_ref()
            } else {
                None
            };
            let snap_eff = self
                .snapshots
                .iter()
                .find(|s| s.id == snapshot_id)
                .or_else(|| self.snapshots.last())
                .map(|snap| match effect {
                    ScreenEffectMode::Invert => &snap.invert_effect,
                    ScreenEffectMode::Burn => &snap.burn_effect,
                    ScreenEffectMode::Dodge => &snap.dodge_effect,
                    ScreenEffectMode::Normal => unreachable!(),
                });
            if let Some(d2d_effect) = live_eff.or(snap_eff) {
                if let Ok(output_img) = d2d_effect.GetOutput() {
                    let img_props = D2D1_IMAGE_BRUSH_PROPERTIES {
                        sourceRectangle: D2D_RECT_F {
                            left: 0.0,
                            top: 0.0,
                            right: self._width as f32,
                            bottom: self._height as f32,
                        },
                        extendModeX: D2D1_EXTEND_MODE_CLAMP,
                        extendModeY: D2D1_EXTEND_MODE_CLAMP,
                        interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
                    };
                    let brush_transform = world_transform
                        .and_then(invert_matrix3x2)
                        .unwrap_or(windows::Foundation::Numerics::Matrix3x2 {
                            M11: 1.0,
                            M12: 0.0,
                            M21: 0.0,
                            M22: 1.0,
                            M31: 0.0,
                            M32: 0.0,
                        });
                    let brush_props = D2D1_BRUSH_PROPERTIES {
                        opacity: eff_alpha,
                        transform: brush_transform,
                    };
                    if let Ok(img_brush) = self.d2d_context.CreateImageBrush(&output_img, &img_props, Some(&brush_props)) {
                        return img_brush.cast();
                    }
                }
            }
        }

        let solid = self.render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: color[0],
                g: color[1],
                b: color[2],
                a: eff_alpha,
            },
            None,
        )?;
        solid.cast()
    }

    /// Render a shape with optional Gaussian edge softness and stacked screen-effect compositing
    unsafe fn draw_shape_composited(
        &mut self,
        shape: &Shape,
        opacity: f32,
        main_target: &ID2D1Image,
        update_accum: bool,
    ) -> Result<()> {
        let (effect, _snapshot_id, softness) = Self::shape_effect_and_softness(shape);
        let sigma = softness.clamp(0.0, 1.0) * 14.0;

        if effect == ScreenEffectMode::Normal && sigma <= 0.15 {
            self.draw_shape_internal(shape, opacity, false)?;
            if update_accum {
                if let Some(ref accum_bmp) = self.accum_bitmap {
                    self.d2d_context.SetTarget(accum_bmp);
                    let res = self.draw_shape_internal(shape, opacity, false);
                    self.d2d_context.SetTarget(main_target);
                    res?;
                }
            }
            return Ok(());
        }

        if effect == ScreenEffectMode::Normal {
            self.ensure_blur_effect()?;
            let dirty = self.shape_dirty_rect(shape, sigma * 3.0 + 4.0);
            if dirty.right <= dirty.left + 0.5 || dirty.bottom <= dirty.top + 0.5 {
                return Ok(());
            }

            let cmd_list = self.d2d_context.CreateCommandList()?;
            self.d2d_context.SetTarget(&cmd_list);
            let draw_res = self.draw_shape_internal(shape, opacity, false);
            self.d2d_context.SetTarget(main_target);
            draw_res?;
            cmd_list.Close()?;

            let blur = self.blur_effect.as_ref().unwrap();
            let cmd_img: ID2D1Image = cmd_list.cast()?;
            blur.SetInput(0, &cmd_img, true);
            let _ = blur.SetValue(
                D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                D2D1_PROPERTY_TYPE_FLOAT,
                &sigma.to_le_bytes(),
            );
            let blur_out = blur.GetOutput()?;
            let offset = D2D_POINT_2F { x: dirty.left, y: dirty.top };

            self.d2d_context.DrawImage(
                &blur_out,
                Some(&offset),
                Some(&dirty),
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );

            if update_accum {
                if let Some(ref accum_bmp) = self.accum_bitmap {
                    self.d2d_context.SetTarget(accum_bmp);
                    self.d2d_context.DrawImage(
                        &blur_out,
                        Some(&offset),
                        Some(&dirty),
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        D2D1_COMPOSITE_MODE_SOURCE_OVER,
                    );
                    self.d2d_context.SetTarget(main_target);
                }
            }
            return Ok(());
        }

        // Stacked Effect Mode (Burn / Dodge / Invert): mask + accum_bitmap + Gaussian Blur
        self.ensure_effect_resources()?;
        let dirty = self.shape_dirty_rect(shape, sigma * 3.0 + 6.0);
        let left = dirty.left.floor().max(0.0) as u32;
        let top = dirty.top.floor().max(0.0) as u32;
        let right = dirty.right.ceil().min(self._width as f32) as u32;
        let bottom = dirty.bottom.ceil().min(self._height as f32) as u32;
        if right <= left || bottom <= top {
            return Ok(());
        }

        let aligned_dirty = D2D_RECT_F {
            left: left as f32,
            top: top as f32,
            right: right as f32,
            bottom: bottom as f32,
        };

        let cmd_list = self.d2d_context.CreateCommandList()?;
        self.d2d_context.SetTarget(&cmd_list);
        let mask_res = self.draw_shape_internal(shape, opacity, true);
        self.d2d_context.SetTarget(main_target);
        mask_res?;
        cmd_list.Close()?;

        let mask_img: ID2D1Image = if sigma > 0.15 {
            let blur = self.blur_effect.as_ref().unwrap();
            let cmd_img: ID2D1Image = cmd_list.cast()?;
            blur.SetInput(0, &cmd_img, true);
            let _ = blur.SetValue(
                D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                D2D1_PROPERTY_TYPE_FLOAT,
                &sigma.to_le_bytes(),
            );
            blur.GetOutput()?
        } else {
            cmd_list.cast()?
        };

        let accum_bmp = self.accum_bitmap.as_ref().unwrap();
        let scratch_bmp = self.effect_scratch_bitmap.as_ref().unwrap();
        let dest_pt = D2D_POINT_2U { x: left, y: top };
        let src_rect_u = D2D_RECT_U { left, top, right, bottom };
        scratch_bmp.CopyFromBitmap(Some(&dest_pt), accum_bmp, Some(&src_rect_u))?;

        let eff = match effect {
            ScreenEffectMode::Invert => self.frame_invert_effect.as_ref().unwrap(),
            ScreenEffectMode::Burn => self.frame_burn_effect.as_ref().unwrap(),
            ScreenEffectMode::Dodge => self.frame_dodge_effect.as_ref().unwrap(),
            ScreenEffectMode::Normal => unreachable!(),
        };
        let eff_out = eff.GetOutput()?;

        let comp = self.mask_composite_effect.as_ref().unwrap();
        comp.SetInput(0, &eff_out, true);
        comp.SetInput(1, &mask_img, true);
        let comp_out = comp.GetOutput()?;

        let offset = D2D_POINT_2F { x: aligned_dirty.left, y: aligned_dirty.top };
        self.d2d_context.DrawImage(
            &comp_out,
            Some(&offset),
            Some(&aligned_dirty),
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );

        if update_accum {
            self.d2d_context.SetTarget(accum_bmp);
            self.d2d_context.DrawImage(
                &comp_out,
                Some(&offset),
                Some(&aligned_dirty),
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            self.d2d_context.SetTarget(main_target);
        }

        Ok(())
    }

    /// Draw a completed, in-progress, or mask shape onto the active target
    unsafe fn draw_shape_internal(&self, shape: &Shape, opacity: f32, force_white_mask: bool) -> Result<()> {
        let mask_color = |c: [f32; 4]| -> [f32; 4] {
            if force_white_mask { [1.0, 1.0, 1.0, c[3]] } else { c }
        };
        let mask_effect = |e: ScreenEffectMode| -> ScreenEffectMode {
            if force_white_mask { ScreenEffectMode::Normal } else { e }
        };

        match shape {
            Shape::Freehand { points, color, width, softness: _, effect, snapshot_id } => {
                if points.len() < 2 {
                    return Ok(());
                }
                let brush = self.make_shape_brush(
                    mask_color(*color),
                    opacity,
                    mask_effect(*effect),
                    *snapshot_id,
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
            Shape::Line { start, end, color, width, dotted, wavy, effect, snapshot_id } => {
                let brush = self.make_shape_brush(mask_color(*color), opacity, mask_effect(*effect), *snapshot_id, None)?;

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
            Shape::Arrow { start, end, color, width, wavy, effect, snapshot_id } => {
                let brush = self.make_shape_brush(mask_color(*color), opacity, mask_effect(*effect), *snapshot_id, None)?;

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
            Shape::Rect { p1, p2, color, width, fill, rotation_rad, effect, snapshot_id } => {
                let brush = self.make_shape_brush(mask_color(*color), opacity, mask_effect(*effect), *snapshot_id, None)?;

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
            Shape::Ellipse { center, rx, ry, color, width, fill, effect, snapshot_id } => {
                let brush = self.make_shape_brush(mask_color(*color), opacity, mask_effect(*effect), *snapshot_id, None)?;
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
            Shape::Emoji { pos, emoji, size, opacity: shape_op, color, flat, filled, rotation_rad, effect, snapshot_id } => {
                let eff_opacity = (*shape_op * opacity).clamp(0.01, 1.0);
                let wide_text: Vec<u16> = emoji.encode_utf16().collect();
                let s = (*size / 32.0).clamp(0.2, 5.0);
                let cos_a = rotation_rad.cos();
                let sin_a = rotation_rad.sin();
                let transform = windows::Foundation::Numerics::Matrix3x2 {
                    M11: s * cos_a,
                    M12: s * sin_a,
                    M21: -s * sin_a,
                    M22: s * cos_a,
                    M31: pos.x * (1.0 - s * cos_a) + pos.y * (s * sin_a),
                    M32: pos.y * (1.0 - s * cos_a) - pos.x * (s * sin_a),
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

                let layer_params = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: rect,
                    geometricMask: ManuallyDrop::new(None),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: identity,
                    opacity: eff_opacity,
                    opacityBrush: ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                };
                self.d2d_context.PushLayer(&layer_params, None::<&ID2D1Layer>);

                let use_effect_or_flat = force_white_mask || *flat || *effect != ScreenEffectMode::Normal;
                if use_effect_or_flat {
                    let brush_color = [1.0, 1.0, 1.0, 1.0];
                    let brush_col = if force_white_mask { brush_color } else { [color[0], color[1], color[2], 1.0] };
                    let brush = self.make_shape_brush(brush_col, 1.0, mask_effect(*effect), *snapshot_id, Some(&transform))?;
                    if *filled || !*flat {
                        self.d2d_context.DrawText(
                            &wide_text,
                            &self.text_format_emoji,
                            &rect,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    } else {
                        let offsets = [
                            (-1.5f32, 0.0f32), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5),
                            (-1.1, -1.1), (1.1, 1.1), (-1.1, 1.1), (1.1, -1.1),
                        ];
                        for (dx, dy) in offsets {
                            let off_rect = D2D_RECT_F {
                                left: rect.left + dx,
                                top: rect.top + dy,
                                right: rect.right + dx,
                                bottom: rect.bottom + dy,
                            };
                            self.d2d_context.DrawText(
                                &wide_text,
                                &self.text_format_emoji,
                                &off_rect,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        }

                        self.d2d_context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                        let clear_brush = self.render_target.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
                            None,
                        )?;
                        self.d2d_context.DrawText(
                            &wide_text,
                            &self.text_format_emoji,
                            &rect,
                            &clear_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.d2d_context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                    }
                } else {
                    let brush = self.render_target.CreateSolidColorBrush(
                        &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                        None,
                    )?;
                    self.d2d_context.DrawText(
                        &wide_text,
                        &self.text_format_emoji,
                        &rect,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                self.d2d_context.PopLayer();
                self.d2d_context.SetTransform(&identity);
            }
            Shape::Text { pos, text, font_family, font_size, color, rotation_rad, effect, snapshot_id } => {
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

                    let is_rotated = rotation_rad.abs() > 0.001;
                    let mut rot_transform = None;
                    if is_rotated {
                        let text_w = (text.len() as f32 * font_size * 0.6).max(24.0);
                        let text_h = (*font_size * 1.3).max(24.0);
                        let cx = pos.x + text_w * 0.5;
                        let cy = pos.y + text_h * 0.5;
                        let cos_a = rotation_rad.cos();
                        let sin_a = rotation_rad.sin();
                        let mat = windows::Foundation::Numerics::Matrix3x2 {
                            M11: cos_a,
                            M12: sin_a,
                            M21: -sin_a,
                            M22: cos_a,
                            M31: cx * (1.0 - cos_a) + cy * sin_a,
                            M32: cy * (1.0 - cos_a) - cx * sin_a,
                        };
                        self.d2d_context.SetTransform(&mat);
                        rot_transform = Some(mat);
                    }

                    let brush = self.make_shape_brush(mask_color(*color), opacity, mask_effect(*effect), *snapshot_id, rot_transform.as_ref())?;
                    let rect = D2D_RECT_F {
                        left: pos.x,
                        top: pos.y,
                        right: pos.x + approx_w,
                        bottom: pos.y + approx_h,
                    };
                    let text_opts = if force_white_mask || *effect != ScreenEffectMode::Normal {
                        D2D1_DRAW_TEXT_OPTIONS_NONE
                    } else {
                        D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT
                    };
                    self.d2d_context.DrawText(
                        &wide_text,
                        &format,
                        &rect,
                        &brush,
                        text_opts,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if is_rotated {
                        let identity = windows::Foundation::Numerics::Matrix3x2 {
                            M11: 1.0,
                            M12: 0.0,
                            M21: 0.0,
                            M22: 1.0,
                            M31: 0.0,
                            M32: 0.0,
                        };
                        self.d2d_context.SetTransform(&identity);
                    }
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
        let clamped_font_size = config.font_size.clamp(12.0, 64.0);
        if (clamped_font_size - self.osd_font_size).abs() > 0.25 {
            if let Ok(tf) = self.dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                clamped_font_size,
                w!("en-US"),
            ) {
                let _ = tf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = tf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                self.text_format_osd = tf;
                self.osd_font_size = clamped_font_size;
            }
        }

        let work_rect = if config.monitor == "primary" {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                Some(mi.rcWork)
            } else {
                None
            }
        } else if let Some(idx_str) = config.monitor.strip_prefix("monitor_") {
            let monitors = enumerate_monitors();
            idx_str
                .parse::<usize>()
                .ok()
                .and_then(|idx| monitors.get(idx).map(|m| m.work_rect))
                .or_else(|| monitors.iter().find(|m| m.is_primary).or_else(|| monitors.first()).map(|m| m.work_rect))
        } else {
            let pt = POINT {
                x: cursor_screen.x as i32,
                y: cursor_screen.y as i32,
            };
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                Some(mi.rcWork)
            } else {
                None
            }
        };

        let (mon_left, mon_top, mon_w, mon_h) = if let Some(rc) = work_rect {
            (
                rc.left as f32,
                rc.top as f32,
                (rc.right - rc.left) as f32,
                (rc.bottom - rc.top) as f32,
            )
        } else {
            let w = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as f32;
            let h = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as f32;
            (0.0, 0.0, w, h)
        };

        let padding_h = (clamped_font_size * 0.78).max(14.0);
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
                120.0,
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
        total_width = (total_width - badge_spacing).max(0.0);

        let badge_height = (clamped_font_size * 1.85 + 6.0).max(38.0);
        let (screen_start_x, screen_top_y) = compute_osd_origin(
            mon_left,
            mon_top,
            mon_w,
            mon_h,
            total_width,
            badge_height,
            &config.position,
            config.edge_offset,
        );
        let client_origin = self.screen_to_client(Point2D::new(screen_start_x, screen_top_y));
        let mut start_x = client_origin.x;
        let top_y = client_origin.y;

        let bg_lum = 0.299 * config.bg_color[0] + 0.587 * config.bg_color[1] + 0.114 * config.bg_color[2];
        let is_light_bg = bg_lum > 0.62 && self.osd_bg_bitmap.is_none();
        let text_rgb = if is_light_bg && config.fg_color[0] > 0.85 && config.fg_color[1] > 0.85 && config.fg_color[2] > 0.85 {
            [0.08, 0.09, 0.12]
        } else {
            [config.fg_color[0], config.fg_color[1], config.fg_color[2]]
        };
        let border_col = if is_light_bg {
            [0.0, 0.0, 0.0, 0.22]
        } else {
            [1.0, 1.0, 1.0, 0.18]
        };

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
                    r: border_col[0],
                    g: border_col[1],
                    b: border_col[2],
                    a: border_col[3] * alpha,
                },
                None,
            )?;
            self.d2d_context.DrawRoundedRectangle(&rounded, &border_brush, 1.5, None);

            // Text
            let text_brush = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: text_rgb[0],
                    g: text_rgb[1],
                    b: text_rgb[2],
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

fn invert_matrix3x2(
    m: &windows::Foundation::Numerics::Matrix3x2,
) -> Option<windows::Foundation::Numerics::Matrix3x2> {
    let det = m.M11 * m.M22 - m.M12 * m.M21;
    if det.abs() < 1e-6 {
        return None;
    }
    let inv_det = 1.0 / det;
    let m11 = m.M22 * inv_det;
    let m12 = -m.M12 * inv_det;
    let m21 = -m.M21 * inv_det;
    let m22 = m.M11 * inv_det;
    let m31 = -(m.M31 * m11 + m.M32 * m21);
    let m32 = -(m.M31 * m12 + m.M32 * m22);
    Some(windows::Foundation::Numerics::Matrix3x2 {
        M11: m11,
        M12: m12,
        M21: m21,
        M22: m22,
        M31: m31,
        M32: m32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invert_matrix3x2_roundtrip() {
        let s = 2.0f32;
        let a = std::f32::consts::FRAC_PI_3;
        let (sin_a, cos_a) = a.sin_cos();
        let px = 120.0f32;
        let py = 80.0f32;
        let m = windows::Foundation::Numerics::Matrix3x2 {
            M11: s * cos_a,
            M12: s * sin_a,
            M21: -s * sin_a,
            M22: s * cos_a,
            M31: px * (1.0 - s * cos_a) + py * (s * sin_a),
            M32: py * (1.0 - s * cos_a) - px * (s * sin_a),
        };
        let inv = invert_matrix3x2(&m).expect("matrix should be invertible");
        // Verify point (150, 110) * m * inv == (150, 110)
        let x = 150.0f32;
        let y = 110.0f32;
        let tx = x * m.M11 + y * m.M21 + m.M31;
        let ty = x * m.M12 + y * m.M22 + m.M32;
        let rx = tx * inv.M11 + ty * inv.M21 + inv.M31;
        let ry = tx * inv.M12 + ty * inv.M22 + inv.M32;
        assert!((rx - x).abs() < 1e-3);
        assert!((ry - y).abs() < 1e-3);
    }

    #[test]
    fn test_compute_osd_origin_positions_and_offset() {
        let (x, y) = compute_osd_origin(0.0, 0.0, 1920.0, 1080.0, 200.0, 50.0, "top_left", 24.0);
        assert_eq!((x, y), (24.0, 24.0));

        let (x, y) = compute_osd_origin(0.0, 0.0, 1920.0, 1080.0, 200.0, 50.0, "top_center", 30.0);
        assert_eq!((x, y), (860.0, 30.0));

        let (x, y) = compute_osd_origin(0.0, 0.0, 1920.0, 1080.0, 200.0, 50.0, "bottom_right", 40.0);
        assert_eq!((x, y), (1680.0, 990.0));

        let (x, y) = compute_osd_origin(1920.0, 0.0, 1280.0, 720.0, 200.0, 60.0, "center", 50.0);
        assert_eq!((x, y), (2460.0, 330.0));
    }
}
