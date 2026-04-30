use crate::common::get_exact_text_metrics;
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use lianli_shared::template::TextAlign;
use rusttype::{point, Font, Scale};

#[allow(clippy::too_many_arguments)]
pub fn draw_text_widget(
    sub: &mut RgbaImage,
    text: &str,
    font: &Font<'static>,
    size: f32,
    color: [u8; 4],
    align: TextAlign,
    ww: u32,
    wh: u32,
    letter_spacing: f32,
) {
    if text.is_empty() || color[3] == 0 {
        return;
    }
    let scale = Scale::uniform(size.max(1.0));

    if letter_spacing.abs() < f32::EPSILON {
        let (tw, th, ox, oy, _ascent) = get_exact_text_metrics(font, text, scale);
        if tw <= 0 || th <= 0 {
            return;
        }
        let x = match align {
            TextAlign::Left => 0,
            TextAlign::Center => ((ww as i32) - tw) / 2,
            TextAlign::Right => (ww as i32) - tw,
        } - ox;
        let y = ((wh as i32) - th) / 2 - oy;
        draw_text_mut(sub, Rgba(color), x, y, scale, font, text);
        return;
    }

    let v_metrics = font.v_metrics(scale);
    let ascent = v_metrics.ascent;
    let mut cursor_x = 0.0_f32;
    let mut positioned: Vec<(f32, rusttype::PositionedGlyph<'static>)> = Vec::new();
    for ch in text.chars() {
        let glyph = font.glyph(ch).scaled(scale);
        let advance = glyph.h_metrics().advance_width;
        let positioned_glyph = glyph.positioned(point(cursor_x, ascent));
        positioned.push((cursor_x, positioned_glyph));
        cursor_x += advance + letter_spacing;
    }
    let total_w = (cursor_x - letter_spacing).max(0.0);
    let th = (v_metrics.ascent - v_metrics.descent) as i32;

    let base_x = match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => (ww as f32 - total_w) / 2.0,
        TextAlign::Right => ww as f32 - total_w,
    };
    let base_y = ((wh as i32) - th) / 2;

    let rgba = Rgba(color);
    let (iw, ih) = (sub.width() as i32, sub.height() as i32);
    for (_start_x, glyph) in positioned {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, gv| {
                if gv <= 0.0 {
                    return;
                }
                let px = base_x.round() as i32 + bb.min.x + gx as i32;
                let py = base_y + bb.min.y + gy as i32;
                if px < 0 || py < 0 || px >= iw || py >= ih {
                    return;
                }
                let a = gv * (color[3] as f32 / 255.0);
                let pix = sub.get_pixel_mut(px as u32, py as u32);
                pix[0] = (pix[0] as f32 * (1.0 - a) + rgba[0] as f32 * a).round() as u8;
                pix[1] = (pix[1] as f32 * (1.0 - a) + rgba[1] as f32 * a).round() as u8;
                pix[2] = (pix[2] as f32 * (1.0 - a) + rgba[2] as f32 * a).round() as u8;
                let alpha_out = pix[3] as f32 / 255.0 + a * (1.0 - pix[3] as f32 / 255.0);
                pix[3] = (alpha_out * 255.0).round().min(255.0) as u8;
            });
        }
    }
}
