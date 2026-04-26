use image::RgbImage;
use itertools::Itertools;
use palette::{
    IntoColor, IntoColorMut, LightenAssign, LinSrgb, Okhsl, Oklab, Oklch, SaturateAssign,
    ShiftHueAssign, Srgb, cast::FromComponents, color_difference::EuclideanDistance,
};

// Modified slightly from https://nelari.us/post/quick_and_dirty_dithering/#bayer-matrix
pub struct BayerMatrix {
    size: u32,
    matrix: Vec<u8>,
}

impl BayerMatrix {
    pub fn new(exponent: u32) -> Self {
        let size = 2_u32.pow(exponent);
        let matrix = (0..size)
            .cartesian_product(0..size)
            .map(|(x, y)| {
                let xc = x ^ y;
                let yc = y;
                let mut v = 0;
                for p in (0..exponent).rev() {
                    let bit_idx = 2 * (exponent - p - 1);
                    v |= ((yc >> p) & 1) << bit_idx;
                    v |= ((xc >> p) & 1) << (bit_idx + 1);
                }
                v as f32 as u8
            })
            .collect();
        Self { size, matrix }
    }

    pub fn index(&self, x: u32, y: u32) -> u8 {
        let j = x % self.size;
        let i = y % self.size;
        let idx = (i * self.size + j) as usize;
        self.matrix[idx]
    }
}

pub fn palette_from_image(image: &RgbImage) -> Vec<LinSrgb> {
    let mut colours: Vec<LinSrgb> = vec![];
    for pixel in <&[Srgb<u8>]>::from_components(&**image) {
        colours.push(pixel.into_linear());
    }
    colours
}

pub fn apply_contrast(colour: &mut LinSrgb, contrast: i32) {
    let contrast = contrast.clamp(-100, 100) as f32 / 100.0;
    let percentage = (contrast + 1.0).powi(2);
    *colour = (*colour - 0.5) * percentage + 0.5;
}

pub fn apply_brightness(colour: &mut LinSrgb, brightness: i32) {
    let brightness = brightness.clamp(-100, 100) as f32 / 100.0;
    let colour_oklab: &mut Oklab = &mut colour.into_color_mut();
    colour_oklab.lighten_assign(brightness);
}

pub fn apply_hue(colour: &mut LinSrgb, hue: i32) {
    let hue = hue.clamp(0, 360);
    let colour_oklch: &mut Oklch = &mut colour.into_color_mut();
    colour_oklch.shift_hue_assign(hue as f32);
}

pub fn apply_saturation(colour: &mut LinSrgb, saturation: i32) {
    let saturation = saturation.clamp(-100, 100) as f32 / 100.0;
    let colour_okhsl: &mut Okhsl = &mut colour.into_color_mut();
    colour_okhsl.saturate_assign(saturation);
}

pub fn apply_palette(pixel_colour: &mut LinSrgb, palette: &[LinSrgb]) {
    *pixel_colour = get_closest_palette_colour(palette, pixel_colour);
}

// Pattern dithering: https://bisqwit.iki.fi/story/howto/dither/jy/#PatternDitheringThePatentedAlgorithmUsedInAdobePhotoshop
pub fn apply_palette_dithered(
    x: u32,
    y: u32,
    pixel_colour: &mut LinSrgb,
    palette: &[LinSrgb],
    bayer_matrix: &BayerMatrix,
    threshold: f32,
) {
    let mut candidates: Vec<Oklab> = vec![];
    let mut error = LinSrgb::new(0.0, 0.0, 0.0);
    let matrix_element_count = bayer_matrix.size.pow(2);
    for _ in 0..matrix_element_count {
        let sample = *pixel_colour + error * threshold;
        let candidate = get_closest_palette_colour(palette, &sample);
        candidates.push(candidate.into_color());
        error += *pixel_colour - candidate;
    }

    candidates.sort_by(|Oklab { l: l1, .. }, Oklab { l: l2, .. }| l1.partial_cmp(l2).unwrap());
    *pixel_colour = candidates[bayer_matrix.index(x, y) as usize].into_color();
}

pub fn get_closest_palette_colour(palette: &[LinSrgb], colour: &LinSrgb) -> LinSrgb {
    let mut closest_colour = LinSrgb::new(0.0, 0.0, 0.0);
    let mut closest_distance_squared = f32::MAX;
    for palette_colour in palette {
        let distance = colour.distance_squared(*palette_colour);
        if distance < closest_distance_squared {
            closest_distance_squared = distance;
            closest_colour = *palette_colour
        }
    }
    closest_colour
}
