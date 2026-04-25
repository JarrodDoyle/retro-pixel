use std::{ffi::OsStr, fs, path::PathBuf};

use clap::Parser;
use image::{ImageError, ImageFormat, ImageReader, RgbImage, imageops::FilterType};
use itertools::Itertools;
use palette::{
    DarkenAssign, IntoColor, IntoColorMut, LightenAssign, LinSrgb, Okhsl, Oklab, Oklch,
    SaturateAssign, ShiftHueAssign, Srgb, cast::FromComponents,
    color_difference::EuclideanDistance,
};
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    palette_path: PathBuf,
    #[arg(short, long)]
    input_path: PathBuf,
    #[arg(short, long)]
    output_path: Option<PathBuf>,
    #[arg(short = 's', long, default_value_t = 4)]
    pixel_scale: u32,
    #[arg(short, long)]
    dither: bool,
    #[arg(short = 'e', long, default_value_t = 2)]
    dither_exponent: u32,
    #[arg(short = 't', long, default_value_t = 0.05)]
    dither_threshold: f32,
    #[arg(short, long)]
    batch: bool,

    #[arg(long)]
    hue: Option<i32>,
    #[arg(long)]
    saturation: Option<i32>,
    #[arg(long)]
    brightness: Option<i32>,
    #[arg(long)]
    contrast: Option<i32>,
}

// Modified slightly from https://nelari.us/post/quick_and_dirty_dithering/#bayer-matrix
struct BayerMatrix {
    size: u32,
    matrix: Vec<u8>,
}

impl BayerMatrix {
    fn new(exponent: u32) -> Self {
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

    fn index(&self, x: u32, y: u32) -> u8 {
        let j = x % self.size;
        let i = y % self.size;
        let idx = (i * self.size + j) as usize;
        self.matrix[idx]
    }
}

fn palette_from_image(image: &RgbImage) -> Vec<LinSrgb> {
    let mut colours: Vec<LinSrgb> = vec![];
    for pixel in <&[Srgb<u8>]>::from_components(&**image) {
        colours.push(pixel.into_linear());
    }
    colours
}

fn apply_contrast(colour: &mut LinSrgb, contrast: i32) {
    let contrast = contrast.clamp(-100, 100) as f32 / 100.0;
    let percentage = (contrast + 1.0).powi(2);
    *colour = (*colour - 0.5) * percentage + 0.5;
}

fn apply_brightness(colour: &mut LinSrgb, brightness: i32) {
    let brightness = brightness.clamp(-100, 100) as f32 / 100.0;
    let colour_oklab: &mut Oklab = &mut colour.into_color_mut();
    if brightness.signum() > 0.0 {
        colour_oklab.lighten_assign(brightness);
    } else {
        colour_oklab.darken_assign(brightness);
    }
}

fn apply_hue(colour: &mut LinSrgb, hue: i32) {
    let colour_oklch: &mut Oklch = &mut colour.into_color_mut();
    colour_oklch.shift_hue_assign(hue as f32);
}

fn apply_saturation(colour: &mut LinSrgb, saturation: i32) {
    let saturation = saturation.clamp(-100, 100) as f32 / 100.0;
    let colour_okhsl: &mut Okhsl = &mut colour.into_color_mut();
    colour_okhsl.saturate_assign(saturation);
}

fn apply_palette(pixel_colour: &mut LinSrgb, palette: &Vec<LinSrgb>) {
    *pixel_colour = get_closest_palette_colour(palette, pixel_colour);
}

// Pattern dithering: https://bisqwit.iki.fi/story/howto/dither/jy/#PatternDitheringThePatentedAlgorithmUsedInAdobePhotoshop
fn apply_palette_dithered(
    x: u32,
    y: u32,
    pixel_colour: &mut LinSrgb,
    palette: &Vec<LinSrgb>,
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

fn get_closest_palette_colour(palette: &Vec<LinSrgb>, colour: &LinSrgb) -> LinSrgb {
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

fn main() -> Result<(), ImageError> {
    let args = Args::parse();

    let mut paths = vec![];
    if args.batch {
        // TODO: Handle input and output not being dirs!
        let output_dir = match args.output_path {
            Some(path) if path.is_dir() => path,
            _ => args.input_path.join("output"),
        };

        let input_paths = fs::read_dir(&args.input_path)?
            .filter_map(|e| e.ok())
            .filter_map(|e| match e.path().extension() {
                Some(ext) if ImageFormat::from_extension(ext).is_some() => Some(e.path()),
                _ => None,
            });
        for input_path in input_paths {
            let output_path = output_dir.join(input_path.file_name().unwrap_or_default());
            paths.push((input_path, output_path));
        }

        if !output_dir.exists() {
            fs::create_dir(output_dir)?;
        }
    } else {
        let output_path = match args.output_path {
            Some(path) => path,
            None => {
                let mut file_name = args.input_path.file_prefix().unwrap_or_default().to_owned();
                file_name.push(OsStr::new("_output"));
                let mut path = args.input_path.with_file_name(file_name);
                if let Some(ext) = args.input_path.extension() {
                    path.set_extension(ext);
                }
                path
            }
        };
        paths.push((args.input_path, output_path));
    }

    let palette_image = ImageReader::open(args.palette_path)?.decode()?.into_rgb8();
    let palette = palette_from_image(&palette_image);
    let bayer_matrix = BayerMatrix::new(args.dither_exponent);
    for (input_path, output_path) in paths {
        let image = ImageReader::open(input_path)?.decode()?;
        let image = image.resize(
            image.width() / args.pixel_scale,
            image.height() / args.pixel_scale,
            FilterType::Nearest,
        );

        let mut output_image = image.into_rgb8();
        output_image
            .par_enumerate_pixels_mut()
            .for_each(|(x, y, pixel)| {
                let mut colour_linear = Srgb::from(pixel.0).into_linear::<f32>();

                if let Some(contrast) = args.contrast {
                    apply_contrast(&mut colour_linear, contrast);
                }

                if let Some(brightness) = args.brightness {
                    apply_brightness(&mut colour_linear, brightness);
                }

                if let Some(hue) = args.hue {
                    apply_hue(&mut colour_linear, hue);
                }

                if let Some(saturation) = args.saturation {
                    apply_saturation(&mut colour_linear, saturation);
                }

                if args.dither {
                    apply_palette_dithered(
                        x,
                        y,
                        &mut colour_linear,
                        &palette,
                        &bayer_matrix,
                        args.dither_threshold,
                    );
                } else {
                    apply_palette(&mut colour_linear, &palette);
                }

                *pixel = image::Rgb(Srgb::from_linear(colour_linear).into());
            });

        output_image.save(output_path)?;
    }

    Ok(())
}
