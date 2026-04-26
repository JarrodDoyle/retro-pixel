use std::{ffi::OsStr, fs, path::PathBuf};

use anyhow::Result;
use clap::{Parser, value_parser};
use image::{ImageFormat, ImageReader};

use crate::image::{ImageSettings, apply_effects, load_image, palette_from_image};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    palette_path: PathBuf,
    #[arg(short, long)]
    input_path: PathBuf,
    #[arg(short, long)]
    output_path: Option<PathBuf>,
    #[arg(short = 's', long, default_value_t = 4, value_parser = value_parser!(u32).range(1..=16))]
    pixel_scale: u32,
    #[arg(short, long)]
    dither: bool,
    #[arg(short = 'e', long, default_value_t = 2)]
    dither_exponent: u32,
    #[arg(short = 't', long, default_value_t = 0.05)]
    dither_threshold: f32,
    #[arg(short, long)]
    batch: bool,

    /// Shift hue [range: 0..=360]
    #[arg(long, value_parser = value_parser!(i32).range(0..=360))]
    hue: Option<i32>,
    /// Adjust saturation [range: -100..=100]
    #[arg(long, allow_hyphen_values = true, value_parser = value_parser!(i32).range(-100..=100))]
    saturation: Option<i32>,
    /// Adjust brightness [range: -100..=100]
    #[arg(long, allow_hyphen_values = true, value_parser = value_parser!(i32).range(-100..=100))]
    brightness: Option<i32>,
    /// Adjust contrast [range: -100..=100]
    #[arg(long, allow_hyphen_values = true, value_parser = value_parser!(i32).range(-100..=100))]
    contrast: Option<i32>,
}

pub fn run() -> Result<()> {
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
    let settings = ImageSettings {
        scale: args.pixel_scale,
        hue: args.hue.unwrap_or_default(),
        saturation: args.saturation.unwrap_or_default(),
        brightness: args.brightness.unwrap_or_default(),
        contrast: args.contrast.unwrap_or_default(),
        dither: args.dither,
        dither_exponent: args.dither_exponent,
        dither_threshold: args.dither_threshold,
    };

    for (input_path, output_path) in paths {
        let image = load_image(&input_path)?;
        let output_image = apply_effects(&image, &palette, &settings);
        output_image.save(output_path)?;
    }

    Ok(())
}
