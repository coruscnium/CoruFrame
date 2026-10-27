use std::path::Path;

use coruframe_core::private_fs;
use image::{GrayImage, RgbImage};

// The reward row relative to the frame, calibrated on a live 3440x1440 reward
// screen. The card width scales with the game's HUD, which follows the height.
const TOP: f64 = 0.370;
const BOTTOM: f64 = 0.470;
const CARD_WIDTH: f64 = 0.239;
pub const LAYOUTS: [u32; 4] = [4, 3, 2, 1];

// The squad player-name row sits at the very bottom of the same band. Keeping
// the top 90% of a card is evidence of the reward name and drops that row; the
// full color card is still kept for the corpus and for OCR.
const NAME_STRIP: f64 = 0.90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometrySource {
    Fixed,
    Detected,
}

impl GeometrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Detected => "detected",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrText {
    pub variant: String,
    pub psm: u32,
    pub text: String,
    pub confidence: f32,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Crop {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub texts: Vec<OcrText>,
    /// Original color card, before thresholding: card evidence and the input to
    /// a later re-OCR pass.
    pub color: RgbImage,
}

impl Crop {
    pub fn name_strip(&self) -> RgbImage {
        let height = ((self.height as f64 * NAME_STRIP) as u32).clamp(1, self.height);
        image::imageops::crop_imm(&self.color, 0, 0, self.width, height).to_image()
    }
}

const WINDOW_RADIUS: i64 = 12;
const BRIGHTNESS_OFFSET: f32 = 25.5;

// Local adaptive threshold against a 25x25 mean, 10% above it. Tesseract fails
// on the pale blue names with a global threshold but reads both colors here.
fn adaptive_threshold_gray(source: &GrayImage) -> GrayImage {
    let (width, height) = source.dimensions();
    let stride = width as usize + 1;
    let mut sums = vec![0u64; stride * (height as usize + 1)];
    for y in 0..height as usize {
        let mut row = 0u64;
        for x in 0..width as usize {
            row += source.get_pixel(x as u32, y as u32)[0] as u64;
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }

    let mut out = GrayImage::new(width, height);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let x0 = (x as i64 - WINDOW_RADIUS).max(0) as usize;
            let y0 = (y as i64 - WINDOW_RADIUS).max(0) as usize;
            let x1 = (x as i64 + WINDOW_RADIUS).min(width as i64 - 1) as usize;
            let y1 = (y as i64 + WINDOW_RADIUS).min(height as i64 - 1) as usize;
            let area = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32;
            let sum = sums[(y1 + 1) * stride + x1 + 1] + sums[y0 * stride + x0]
                - sums[y0 * stride + x1 + 1]
                - sums[(y1 + 1) * stride + x0];
            let value = if source.get_pixel(x as u32, y as u32)[0] as f32
                > sum as f32 / area + BRIGHTNESS_OFFSET
            {
                255
            } else {
                0
            };
            out.put_pixel(x as u32, y as u32, image::Luma([value]));
        }
    }
    out
}

fn luma_image(source: &RgbImage) -> GrayImage {
    GrayImage::from_fn(source.width(), source.height(), |x, y| {
        let [r, g, b] = source.get_pixel(x, y).0;
        image::Luma([luma(r, g, b) as u8])
    })
}

// Gold names are red-heavy and pale blue names are blue-heavy; the per-pixel
// maximum keeps both above a dark background with one threshold.
fn max_channel_image(source: &RgbImage) -> GrayImage {
    GrayImage::from_fn(source.width(), source.height(), |x, y| {
        let [r, g, b] = source.get_pixel(x, y).0;
        image::Luma([r.max(g).max(b)])
    })
}

fn red_channel_image(source: &RgbImage) -> GrayImage {
    GrayImage::from_fn(source.width(), source.height(), |x, y| {
        image::Luma([source.get_pixel(x, y).0[0]])
    })
}

fn upscaled(source: &RgbImage, factor: u32) -> RgbImage {
    image::imageops::resize(
        source,
        source.width() * factor,
        source.height() * factor,
        image::imageops::FilterType::Lanczos3,
    )
}

fn adaptive_threshold(source: &RgbImage) -> GrayImage {
    adaptive_threshold_gray(&luma_image(source))
}

// Deterministic first-pass variants. A card whose first pass is not high
// confidence gets the two extras as well; no card gets an unbounded set.
// The common reward card reads on the first luma pass. Gold and pale blue names
// that do not still get the three extra variants below, so the expensive work
// only happens for the cards that need it.
fn first_pass(card: &RgbImage) -> Vec<(String, u32, GrayImage)> {
    vec![("luma".to_string(), 6, adaptive_threshold(card))]
}

fn extra_pass(card: &RgbImage) -> Vec<(String, u32, GrayImage)> {
    vec![
        (
            "max".to_string(),
            6,
            adaptive_threshold_gray(&max_channel_image(card)),
        ),
        (
            "luma2x".to_string(),
            6,
            adaptive_threshold(&upscaled(card, 2)),
        ),
        (
            "red11".to_string(),
            11,
            adaptive_threshold_gray(&red_channel_image(card)),
        ),
    ]
}

fn luma(r: u8, g: u8, b: u8) -> u64 {
    (r as u64 * 77 + g as u64 * 150 + b as u64 * 29) >> 8
}

pub fn fixed_top(height: u32) -> u32 {
    (height as f64 * TOP) as u32
}

pub fn geometry(width: u32, height: u32, cards: u32) -> Option<(u32, u32, u32, u32)> {
    let top = fixed_top(height);
    let bottom = (height as f64 * BOTTOM) as u32;
    let card_height = bottom.saturating_sub(top);
    let card_width = (height as f64 * CARD_WIDTH) as u32;
    if cards == 0 || card_width == 0 || card_height == 0 || card_width * cards > width {
        return None;
    }
    Some((
        (width - card_width * cards) / 2,
        top,
        card_width,
        card_height,
    ))
}

// Frame-derived refinment of the row. Card tops are strong horizontal edges that
// span the centered card row; the scan only accepts a result close to the
// calibrated top, so a busy background cannot move OCR onto another band.
pub fn detect_row(image: &RgbImage) -> Option<(u32, f32)> {
    let (width, height) = image.dimensions();
    if height < 100 || width < 100 {
        return None;
    }
    let fixed = (height as f64 * TOP) as u32;
    let low = (fixed as f64 * 0.94) as u32;
    let high = ((fixed as f64 * 1.06) as u32).min(height - 1);
    let step = (width / 120).max(1);
    let mut scores = Vec::new();
    for y in low.max(2)..=high {
        let mut score = 0f32;
        let mut samples = 0f32;
        let mut x = 0;
        while x < width {
            let a = luma_image_pixel(image, x, y);
            let b = luma_image_pixel(image, x, y - 2);
            score += a.abs_diff(b) as f32 / 255.0;
            samples += 1.0;
            x += step;
        }
        if samples > 0.0 {
            scores.push((y, score / samples));
        }
    }
    let peak = scores
        .iter()
        .copied()
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let mean = scores.iter().map(|(_, score)| score).sum::<f32>() / scores.len() as f32;
    if peak.1 < 0.05 || mean <= 0.0 || peak.1 / mean < 1.6 {
        return None;
    }
    Some((peak.0, peak.1 / mean))
}

fn luma_image_pixel(image: &RgbImage, x: u32, y: u32) -> u8 {
    let [r, g, b] = image.get_pixel(x, y).0;
    luma(r, g, b) as u8
}

fn ocr_card(
    index: u32,
    cards: u32,
    crop_dir: &Path,
    variants: &[(String, u32, GrayImage)],
) -> Vec<OcrText> {
    let pid = std::process::id();
    variants
        .iter()
        .map(|(variant, psm, image)| {
            let path = crop_dir.join(format!("card-{pid}-{cards}-{index}-{variant}.png"));
            let mut file = match private_fs::create_private(&path) {
                Ok(file) => file,
                Err(error) => {
                    return OcrText {
                        variant: variant.clone(),
                        psm: *psm,
                        text: String::new(),
                        confidence: 0.0,
                        error: Some(format!("create {}: {error}", path.display())),
                    }
                }
            };
            if let Err(error) = image.write_to(&mut file, image::ImageFormat::Png) {
                let _ = std::fs::remove_file(&path);
                return OcrText {
                    variant: variant.clone(),
                    psm: *psm,
                    text: String::new(),
                    confidence: 0.0,
                    error: Some(format!("save {}: {error}", path.display())),
                };
            }
            let ocr = crate::ocr::ocr_tsv(&path, *psm);
            let _ = std::fs::remove_file(&path);
            match ocr {
                Ok(ocr) => {
                    let confidence = ocr.mean_confidence();
                    OcrText {
                        variant: variant.clone(),
                        psm: *psm,
                        text: ocr.text,
                        confidence,
                        error: None,
                    }
                }
                Err(error) => OcrText {
                    variant: variant.clone(),
                    psm: *psm,
                    text: String::new(),
                    confidence: 0.0,
                    error: Some(error),
                },
            }
        })
        .collect()
}

// OCR one card count. A four-card reveal is the common case; the caller tries
// it first and only asks for smaller layouts when the names do not match.
// The detected row may only move the band a little; a detector that jumps to
// another UI element must not silently price text from the wrong place.
pub fn layout_image_with_top(
    image: &RgbImage,
    cards: u32,
    crop_dir: &Path,
    detected_top: Option<u32>,
) -> Result<Vec<Crop>, String> {
    let (width, height) = image.dimensions();
    let Some((left, fixed_top, card_width, card_height)) = geometry(width, height, cards) else {
        return Err(format!(
            "layout {cards} does not fit a {width}x{height} frame"
        ));
    };
    let top = detected_top
        .filter(|top| top.abs_diff(fixed_top) <= height / 100)
        .unwrap_or(fixed_top);
    let out: Vec<Crop> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..cards)
            .map(|index| {
                let x = left + index * card_width;
                let crop_dir = crop_dir.to_path_buf();
                scope.spawn(move || {
                    let crop = image::imageops::crop_imm(image, x, top, card_width, card_height)
                        .to_image();
                    // Card names are gold or pale blue depending on the reward;
                    // a global threshold loses one of the two, a local mean does not.
                    let variants = first_pass(&crop);
                    let texts = ocr_card(index, cards, &crop_dir, &variants);
                    Crop {
                        left: x,
                        top,
                        width: card_width,
                        height: card_height,
                        texts,
                        color: crop,
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "ocr thread panicked".to_string()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(out)
}

// Extra variants for one card that the first pass did not resolve. The crop is
// the full color card: a sub-band crop once cut the first name line and returned
// garbage.
pub fn reocr_image(crop: &Crop, crop_dir: &Path) -> Vec<OcrText> {
    let variants = extra_pass(&crop.color);
    ocr_card(0, 1, crop_dir, &variants)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_bright_text_on_a_dark_background() {
        let mut source = RgbImage::new(60, 60);
        for pixel in source.pixels_mut() {
            *pixel = image::Rgb([10, 10, 10]);
        }
        for y in 25..35 {
            for x in 25..35 {
                source.put_pixel(x, y, image::Rgb([240, 240, 240]));
            }
        }
        let out = adaptive_threshold(&source);
        assert_eq!(out.get_pixel(30, 30)[0], 255);
        assert_eq!(out.get_pixel(5, 5)[0], 0);
    }

    #[test]
    fn a_four_card_layout_fits_a_wide_frame() {
        assert_eq!(geometry(3440, 1440, 4), Some((1032, 532, 344, 144)));
    }

    #[test]
    fn a_layout_that_does_not_fit_is_rejected() {
        assert_eq!(geometry(800, 1440, 4), None);
    }

    #[test]
    fn geometry_scales_with_height_and_centers_on_width() {
        for (width, height) in [
            (1920, 1080),
            (2560, 1440),
            (3440, 1440),
            (2560, 1600),
            (3440, 1440),
        ] {
            for cards in LAYOUTS {
                let (left, top, card_width, card_height) =
                    geometry(width, height, cards).expect("layout fits");
                assert_eq!(card_width, (height as f64 * CARD_WIDTH) as u32);
                assert_eq!(card_height, (height as f64 * 0.1) as u32);
                assert_eq!(top, (height as f64 * TOP) as u32);
                assert_eq!(left, (width - card_width * cards) / 2);
            }
        }
        // Different width, same height: card size and row are unchanged, only
        // the centered left edge moves.
        let (left_wide, top_wide, width_wide, height_wide) = geometry(2560, 1080, 4).unwrap();
        let (left_narrow, top_narrow, width_narrow, height_narrow) =
            geometry(1920, 1080, 4).unwrap();
        assert_eq!(
            (top_wide, width_wide, height_wide),
            (top_narrow, width_narrow, height_narrow)
        );
        assert!(left_wide > left_narrow);
    }

    #[test]
    fn a_coherent_edge_bar_is_detected_near_the_fixed_row() {
        let mut image = RgbImage::from_pixel(1200, 900, image::Rgb([12, 12, 14]));
        let top = (900f64 * TOP) as u32;
        for y in top..(top + 90) {
            for x in 300..900 {
                image.put_pixel(x, y, image::Rgb([70, 66, 58]));
            }
        }
        let (detected, confidence) = detect_row(&image).unwrap();
        assert!(detected.abs_diff(top) <= 2, "{detected} vs {top}");
        assert!(confidence >= 1.6);
    }

    #[test]
    fn a_flat_frame_has_no_detected_row() {
        let image = RgbImage::from_pixel(1200, 900, image::Rgb([12, 12, 14]));
        assert!(detect_row(&image).is_none());
    }

    #[test]
    fn the_variant_budget_is_bounded_and_deterministic() {
        let card = RgbImage::from_pixel(32, 16, image::Rgb([20, 20, 20]));
        let first: Vec<String> = first_pass(&card)
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        let extras: Vec<String> = extra_pass(&card)
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(first, ["luma"]);
        assert_eq!(extras, ["max", "luma2x", "red11"]);
    }

    #[test]
    fn the_name_strip_drops_the_player_row() {
        let mut card = RgbImage::from_pixel(100, 100, image::Rgb([0, 0, 0]));
        for y in 0..90 {
            card.put_pixel(10, y, image::Rgb([255, 255, 255]));
        }
        let crop = Crop {
            left: 0,
            top: 0,
            width: 100,
            height: 100,
            texts: Vec::new(),
            color: card,
        };
        assert_eq!(crop.name_strip().height(), 90);
    }
}
