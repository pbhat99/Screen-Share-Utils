use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Helper to construct a 32-bit hardware cursor from raw BGRA pixel bytes
pub unsafe fn create_cursor_from_bgra(
    width: i32,
    height: i32,
    hotspot_x: u32,
    hotspot_y: u32,
    bgra: &[u8],
) -> Option<HCURSOR> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut p_bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hdc_screen = GetDC(HWND::default());
    let hbm_color = CreateDIBSection(
        hdc_screen,
        &bmi,
        DIB_RGB_COLORS,
        &mut p_bits,
        HANDLE::default(),
        0,
    ).ok()?;

    if !p_bits.is_null() && bgra.len() == (width * height * 4) as usize {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), p_bits as *mut u8, bgra.len());
    }

    // 1-bit monochrome mask bitmap: all 0s for 32-bit alpha blended cursor
    let hbm_mask = CreateBitmap(width, height, 1, 1, None);

    let icon_info = ICONINFO {
        fIcon: FALSE, // FALSE = cursor
        xHotspot: hotspot_x,
        yHotspot: hotspot_y,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hcursor_raw = CreateIconIndirect(&icon_info).ok()?;

    let _ = DeleteObject(hbm_color);
    let _ = DeleteObject(hbm_mask);
    let _ = ReleaseDC(HWND::default(), hdc_screen);

    Some(HCURSOR(hcursor_raw.0))
}

/// Create a sleek, sharp 32x32 angled Pencil / Pen Cursor with tip at (0, 0)
pub fn create_pen_cursor() -> Option<HCURSOR> {
    unsafe {
        let size = 32;
        let mut bgra = vec![0u8; size * size * 4];

        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) * 4;

                // Diagonal geometry: line along x = y
                let along = (x as f32 + y as f32) / 1.4142;
                let perp = ((x as f32 - y as f32) / 1.4142).abs();

                // 1. Sharp Tip Lead (Hotspot at 0, 0)
                if (x == 0 && y == 0) || (x == 1 && y == 0) || (x == 0 && y == 1) {
                    bgra[idx] = 20;      // B
                    bgra[idx + 1] = 20;  // G
                    bgra[idx + 2] = 20;  // R
                    bgra[idx + 3] = 255; // A
                    continue;
                }

                if along >= 1.0 && along < 6.5 {
                    let cone_half_w = 0.5 + along * 0.45;
                    if perp <= cone_half_w {
                        if perp >= cone_half_w - 0.75 {
                            // Dark border
                            bgra[idx] = 30; bgra[idx + 1] = 30; bgra[idx + 2] = 30; bgra[idx + 3] = 255;
                        } else if along < 3.2 {
                            // Graphite lead
                            bgra[idx] = 35; bgra[idx + 1] = 35; bgra[idx + 2] = 40; bgra[idx + 3] = 255;
                        } else {
                            // Sharpened natural wood collar
                            if x < y {
                                bgra[idx] = 135; bgra[idx + 1] = 185; bgra[idx + 2] = 225; bgra[idx + 3] = 255;
                            } else {
                                bgra[idx] = 115; bgra[idx + 1] = 160; bgra[idx + 2] = 200; bgra[idx + 3] = 255;
                            }
                        }
                    }
                } else if along >= 6.5 && along < 23.5 {
                    // Hexagonal Pencil Barrel
                    let body_w = 3.3;
                    if perp <= body_w {
                        if perp >= body_w - 0.75 {
                            // Crisp dark outline
                            bgra[idx] = 25; bgra[idx + 1] = 25; bgra[idx + 2] = 30; bgra[idx + 3] = 255;
                        } else {
                            // Facets
                            let diff = x as f32 - y as f32;
                            if diff < -0.8 {
                                // Upper facet: bright golden highlight
                                bgra[idx] = 80; bgra[idx + 1] = 220; bgra[idx + 2] = 255; bgra[idx + 3] = 255;
                            } else if diff > 0.8 {
                                // Lower facet: shaded amber
                                bgra[idx] = 15; bgra[idx + 1] = 135; bgra[idx + 2] = 210; bgra[idx + 3] = 255;
                            } else {
                                // Middle facet: rich classic yellow
                                bgra[idx] = 25; bgra[idx + 1] = 180; bgra[idx + 2] = 245; bgra[idx + 3] = 255;
                            }
                        }
                    }
                } else if along >= 23.5 && along < 26.5 {
                    // Metallic Ferrule Band
                    let ferr_w = 3.4;
                    if perp <= ferr_w {
                        if perp >= ferr_w - 0.75 {
                            bgra[idx] = 35; bgra[idx + 1] = 35; bgra[idx + 2] = 40; bgra[idx + 3] = 255;
                        } else if along >= 24.8 && along <= 25.3 {
                            // Dark metallic rib groove
                            bgra[idx] = 140; bgra[idx + 1] = 145; bgra[idx + 2] = 150; bgra[idx + 3] = 255;
                        } else {
                            // Silver metal
                            bgra[idx] = 205; bgra[idx + 1] = 200; bgra[idx + 2] = 195; bgra[idx + 3] = 255;
                        }
                    }
                } else if along >= 26.5 && along <= 30.5 {
                    // Pink Rubber Eraser Tip
                    let r_max = 3.3 - (along - 26.5) * 0.35;
                    if perp <= r_max {
                        if perp >= r_max - 0.75 || along >= 30.0 {
                            bgra[idx] = 45; bgra[idx + 1] = 30; bgra[idx + 2] = 35; bgra[idx + 3] = 255;
                        } else if x < y {
                            // Highlighted pink
                            bgra[idx] = 170; bgra[idx + 1] = 150; bgra[idx + 2] = 255; bgra[idx + 3] = 255;
                        } else {
                            // Soft eraser pink
                            bgra[idx] = 140; bgra[idx + 1] = 115; bgra[idx + 2] = 240; bgra[idx + 3] = 255;
                        }
                    }
                }
            }
        }

        create_cursor_from_bgra(size as i32, size as i32, 0, 0, &bgra)
    }
}

/// Create a sleek 32x32 Eraser Block Cursor with contact point at bottom-left corner (4, 27)
pub fn create_eraser_cursor() -> Option<HCURSOR> {
    unsafe {
        let size = 32;
        let mut bgra = vec![0u8; size * size * 4];

        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) * 4;

                // Angled rectangular eraser oriented from bottom-left (4, 27) toward top-right (26, 5)
                // Center line: (x + y) around 31
                // Axis along eraser: t = (x - y)
                // Axis perpendicular: p = (x + y - 31.0)
                let t = x as f32 - y as f32; // ranges from ~ -23 to +21
                let p = ((x as f32 + y as f32) - 31.0) / 1.4142;

                let half_h = 5.2;

                if t >= -22.0 && t <= 20.0 && p.abs() <= half_h {
                    if p.abs() >= half_h - 0.85 || t <= -21.2 || t >= 19.2 {
                        // Crisp dark outer border
                        bgra[idx] = 30; bgra[idx + 1] = 30; bgra[idx + 2] = 35; bgra[idx + 3] = 255;
                    } else if t < -1.0 {
                        // Exposed Eraser Head (Pink)
                        if p < -1.0 {
                            // Top beveled face: lighter pink highlight
                            bgra[idx] = 175; bgra[idx + 1] = 155; bgra[idx + 2] = 255; bgra[idx + 3] = 255;
                        } else if p > 1.0 {
                            // Bottom shadow face: darker pink
                            bgra[idx] = 115; bgra[idx + 1] = 90; bgra[idx + 2] = 210; bgra[idx + 3] = 255;
                        } else {
                            // Main face: vibrant pink
                            bgra[idx] = 145; bgra[idx + 1] = 120; bgra[idx + 2] = 245; bgra[idx + 3] = 255;
                        }
                    } else if t >= -1.0 && t < 1.0 {
                        // Sleeve border line
                        bgra[idx] = 40; bgra[idx + 1] = 40; bgra[idx + 2] = 45; bgra[idx + 3] = 255;
                    } else {
                        // Paper protective sleeve (White with blue accent stripe)
                        if t >= 7.0 && t <= 11.0 {
                            // Blue brand accent stripe
                            bgra[idx] = 215; bgra[idx + 1] = 115; bgra[idx + 2] = 50; bgra[idx + 3] = 255;
                        } else if p < -1.0 {
                            // White top face
                            bgra[idx] = 255; bgra[idx + 1] = 255; bgra[idx + 2] = 255; bgra[idx + 3] = 255;
                        } else {
                            // Off-white / light silver shade
                            bgra[idx] = 230; bgra[idx + 1] = 230; bgra[idx + 2] = 235; bgra[idx + 3] = 255;
                        }
                    }
                }
            }
        }

        // Hotspot at bottom-left corner of eraser tip (4, 27)
        create_cursor_from_bgra(size as i32, size as i32, 4, 27, &bgra)
    }
}

/// Create a full-color 32x32 Emoji Stamp Cursor rendered via Direct2D with hotspot at center (16, 16)
pub fn create_emoji_cursor(
    d2d_factory: &ID2D1Factory,
    dwrite_factory: &IDWriteFactory,
    emoji: &str,
) -> Option<HCURSOR> {
    unsafe {
        let width = 32;
        let height = 32;

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut p_bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hdc_screen = GetDC(HWND::default());
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm_color = CreateDIBSection(
            hdc_mem,
            &bmi,
            DIB_RGB_COLORS,
            &mut p_bits,
            HANDLE::default(),
            0,
        ).ok()?;

        let old_bm = SelectObject(hdc_mem, hbm_color);

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };

        if let Ok(dc_target) = d2d_factory.CreateDCRenderTarget(&props) {
            let rect = RECT { left: 0, top: 0, right: width, bottom: height };
            if dc_target.BindDC(hdc_mem, &rect).is_ok() {
                let text_format = dwrite_factory.CreateTextFormat(
                    w!("Segoe UI Emoji"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    22.0,
                    w!("en-US"),
                );

                if let Ok(fmt) = text_format {
                    let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

                    dc_target.BeginDraw();
                    dc_target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

                    let wide: Vec<u16> = emoji.encode_utf16().collect();
                    let d2d_rect = D2D_RECT_F { left: 0.0, top: 0.0, right: width as f32, bottom: height as f32 };
                    if let Ok(brush) = dc_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None) {
                        dc_target.DrawText(
                            &wide,
                            &fmt,
                            &d2d_rect,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    let _ = dc_target.EndDraw(None, None);
                }
            }
        }

        let _ = SelectObject(hdc_mem, old_bm);
        let _ = DeleteDC(hdc_mem);

        // 1-bit mask bitmap
        let hbm_mask = CreateBitmap(width, height, 1, 1, None);

        let icon_info = ICONINFO {
            fIcon: FALSE,
            xHotspot: 16,
            yHotspot: 16,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };

        let hcursor_raw = CreateIconIndirect(&icon_info).ok();

        let _ = DeleteObject(hbm_color);
        let _ = DeleteObject(hbm_mask);
        let _ = ReleaseDC(HWND::default(), hdc_screen);

        hcursor_raw.map(|h| HCURSOR(h.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pen_and_eraser_cursors() {
        let pen = create_pen_cursor();
        assert!(pen.is_some(), "Pen cursor should be created successfully");
        if let Some(h) = pen {
            unsafe { let _ = DestroyCursor(h); }
        }

        let eraser = create_eraser_cursor();
        assert!(eraser.is_some(), "Eraser cursor should be created successfully");
        if let Some(h) = eraser {
            unsafe { let _ = DestroyCursor(h); }
        }
    }

    #[test]
    fn test_emoji_cursor() {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let emoji_cur = create_emoji_cursor(&d2d, &dwrite, "⭐");
            assert!(emoji_cur.is_some(), "Emoji cursor should be created successfully");
            if let Some(h) = emoji_cur {
                let _ = DestroyCursor(h);
            }
        }
    }
}
