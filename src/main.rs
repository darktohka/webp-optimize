use argh::FromArgs;
use humansize::FormatSizeOptions;
use humansize::SizeFormatter;
use image::DynamicImage;
use image::ImageReader;

use std::{
    fs::{self, File},
    io::Read,
    io::Write,
    path::Path,
};

/// Optimize images to webp format with deduplication.
#[derive(FromArgs)]
struct Cli {
    /// input directory
    #[argh(option)]
    input: String,

    /// output directory
    #[argh(option)]
    output: String,

    /// webp quality (0-100)
    #[argh(option, default = "75")]
    quality: u8,
}

fn main() -> anyhow::Result<()> {
    let cli: Cli = argh::from_env();

    let input_dir = Path::new(&cli.input);
    let output_dir = Path::new(&cli.output);

    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
    }

    let mut total_original_bytes: u64 = 0;
    let mut total_webp_bytes: u64 = 0;

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Read file bytes
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        total_original_bytes += bytes.len() as u64;

        // Calculate blake2b hash
        let mut hasher = blake3::Hasher::new();
        hasher.update(&bytes);
        let hash = hasher.finalize().to_hex();
        let webp_path = output_dir.join(format!("{hash}.webp"));

        if webp_path.exists() {
            // If already exists, count its size
            let webp_size = fs::metadata(&webp_path)?.len();

            if webp_size == 0 {
                total_webp_bytes += bytes.len() as u64; // Count original size if not saving
            } else {
                total_webp_bytes += webp_size;
            }

            continue;
        }

        println!("Processing: {:?}", path);

        // Try to decode image
        let img = ImageReader::open(path)?
            .decode()
            .map_err(|e| anyhow::anyhow!("Failed to decode image {:?}: {:?}", path, e))?;

        // Convert grayscale images to RGB/RGBA
        let img = match img {
            DynamicImage::ImageLuma8(ref gray) => {
                DynamicImage::ImageRgb8(DynamicImage::ImageLuma8(gray.clone()).to_rgb8())
            }
            DynamicImage::ImageLumaA8(ref gray_alpha) => {
                DynamicImage::ImageRgba8(DynamicImage::ImageLumaA8(gray_alpha.clone()).to_rgba8())
            }
            _ => img,
        };

        // Encode as webp
        let mut webp_bytes = Vec::new();
        {
            let encoder = webp::Encoder::from_image(&img)
                .map_err(|e| anyhow::anyhow!("WebP encode error: {:?}", e))?;
            let encoded = encoder.encode(cli.quality as f32);
            webp_bytes.extend_from_slice(&encoded);
        }

        // Compare sizes
        if webp_bytes.len() < bytes.len() {
            let mut out = File::create(&webp_path)?;
            out.write_all(&webp_bytes)?;
            total_webp_bytes += webp_bytes.len() as u64;
        } else {
            // Write empty file
            File::create(&webp_path)?;
            total_webp_bytes += bytes.len() as u64; // Count original size if not saving
            // If not saving, count as 0 bytes saved for this file
        }
    }

    // Print statistics
    let saved_bytes = if total_original_bytes > total_webp_bytes {
        total_original_bytes - total_webp_bytes
    } else {
        0
    };
    let percent_saved = if total_original_bytes > 0 {
        100.0 * (saved_bytes as f64) / (total_original_bytes as f64)
    } else {
        0.0
    };

    println!("\n--- Statistics ---");
    println!(
        "Original total: {}",
        SizeFormatter::new(total_original_bytes, FormatSizeOptions::default())
    );
    println!(
        "WebP total:     {}",
        SizeFormatter::new(total_webp_bytes, FormatSizeOptions::default())
    );
    println!(
        "Bytes saved:    {} ({:.2}%)",
        SizeFormatter::new(saved_bytes, FormatSizeOptions::default()),
        percent_saved
    );

    Ok(())
}
