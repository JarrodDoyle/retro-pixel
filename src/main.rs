use std::{ffi::OsStr, path::PathBuf};

use clap::Parser;
use image::{ImageError, ImageReader, RgbImage, imageops::FilterType};
use itertools::Itertools;
use palette::{IntoColor, Oklab, Srgb, cast::FromComponents, color_difference::EuclideanDistance};
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

fn palette_from_image(image: &RgbImage) -> Vec<Oklab> {
    let mut colours: Vec<Oklab> = vec![];
    for pixel in <&[Srgb<u8>]>::from_components(&**image) {
        colours.push(pixel.into_linear().into_color());
    }
    colours
}

fn apply_palette(image: &mut RgbImage, palette: &Vec<Oklab>) {
    image.par_pixels_mut().for_each(|pixel| {
        let pixel_colour: Oklab = Srgb::from(pixel.0).into_linear().into_color();
        let closest_colour = get_closest_palette_colour(palette, pixel_colour);
        let srgb_colour = Srgb::from_linear(closest_colour.into_color());
        *pixel = image::Rgb([srgb_colour.red, srgb_colour.green, srgb_colour.blue]);
    });
}

// Pattern dithering: https://bisqwit.iki.fi/story/howto/dither/jy/#PatternDitheringThePatentedAlgorithmUsedInAdobePhotoshop
fn apply_palette_dithered(
    image: &mut RgbImage,
    palette: &Vec<Oklab>,
    bayer_matrix: &BayerMatrix,
    threshold: f32,
) {
    image.par_enumerate_pixels_mut().for_each(|(x, y, pixel)| {
        let pixel_colour: Oklab = Srgb::from(pixel.0).into_linear().into_color();

        let mut candidates: Vec<Oklab> = vec![];
        let mut error = Oklab::new(0.0, 0.0, 0.0);
        let matrix_element_count = bayer_matrix.size.pow(2);
        for _ in 0..matrix_element_count {
            let sample = pixel_colour + error * threshold;
            let candidate = get_closest_palette_colour(palette, sample);
            candidates.push(candidate);
            error += pixel_colour - candidate;
        }

        candidates.sort_by(|Oklab { l: l1, .. }, Oklab { l: l2, .. }| l1.partial_cmp(l2).unwrap());

        let index = bayer_matrix.index(x, y) as usize;
        let srgb_colour = Srgb::from_linear(candidates[index].into_color());
        *pixel = image::Rgb([srgb_colour.red, srgb_colour.green, srgb_colour.blue]);
    });
}

fn get_closest_palette_colour(palette: &Vec<Oklab>, colour: Oklab) -> Oklab {
    let mut closest_colour = Oklab::new(0.0, 0.0, 0.0);
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

    let image = ImageReader::open(&args.input_path)?.decode()?;
    let image = image.resize(
        image.width() / args.pixel_scale,
        image.height() / args.pixel_scale,
        FilterType::Nearest,
    );

    let palette_image = ImageReader::open(args.palette_path)?.decode()?.into_rgb8();
    let palette_rgb = palette_from_image(&palette_image);
    let mut output_image = image.into_rgb8();

    if args.dither {
        let bayer_matrix = BayerMatrix::new(args.dither_exponent);
        apply_palette_dithered(
            &mut output_image,
            &palette_rgb,
            &bayer_matrix,
            args.dither_threshold,
        );
    } else {
        apply_palette(&mut output_image, &palette_rgb);
    }

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

    output_image.save(output_path)?;
    Ok(())
}
