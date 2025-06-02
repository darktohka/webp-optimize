use argh::FromArgs;
use humansize::FormatSizeOptions;
use humansize::SizeFormatter;
use image::DynamicImage;
use image::ImageReader;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use std::sync::{Arc, Mutex};

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

fn main() {
    let cli: Cli = argh::from_env();

    let input_dir = Path::new(&cli.input);
    let output_dir = Path::new(&cli.output);

    if !output_dir.exists() {
        fs::create_dir_all(output_dir).expect("Failed to create output directory");
    }

    let entries: Vec<_> = walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .collect();

    // Use a Mutex to safely update totals from multiple threads
    let total_original_bytes = Arc::new(Mutex::new(0u64));
    let total_webp_bytes = Arc::new(Mutex::new(0u64));

    entries.par_iter().for_each(|entry| {
        let path = entry.path();

        // Read file bytes
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => {
                eprintln!("Failed to open file: {:?}", path);
                return;
            }
        };

        let mut bytes = Vec::new();

        if file.read_to_end(&mut bytes).is_err() {
            eprintln!("Failed to read file: {:?}", path);
            return;
        }

        {
            let mut orig = total_original_bytes.lock().unwrap();
            *orig += bytes.len() as u64;
        }

        // Calculate blake2b hash
        let mut hasher = blake3::Hasher::new();
        hasher.update(&bytes);
        let hash = hasher.finalize().to_hex();
        let webp_path = entry
            .path()
            .parent()
            .unwrap()
            .join("../")
            .join(format!("{hash}.webp"));

        if webp_path.exists() {
            let webp_size = match fs::metadata(&webp_path) {
                Ok(meta) => meta.len(),
                Err(_) => 0,
            };

            let mut webp = total_webp_bytes.lock().unwrap();
            if webp_size == 0 {
                *webp += bytes.len() as u64;
            } else {
                *webp += webp_size;
            }
            return;
        }

        println!("Processing: {:?}", path);

        // Try to decode image
        let img = ImageReader::open(path)
            .expect("Failed to open image")
            .decode()
            .expect("Failed to decode image");

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
        let encoder = match webp::Encoder::from_image(&img) {
            Ok(enc) => enc,
            Err(_) => return,
        };
        let encoded = encoder.encode(cli.quality as f32);
        webp_bytes.extend_from_slice(&encoded);

        if webp_bytes.len() < bytes.len() {
            if let Ok(mut out) = File::create(&webp_path) {
                let _ = out.write_all(&webp_bytes);
            }
            let mut webp = total_webp_bytes.lock().unwrap();
            *webp += webp_bytes.len() as u64;
        } else {
            let _ = File::create(&webp_path);
            let mut webp = total_webp_bytes.lock().unwrap();
            *webp += bytes.len() as u64;
        }
    });

    // Get totals from mutexes
    let total_original_bytes = *total_original_bytes.lock().unwrap();
    let total_webp_bytes = *total_webp_bytes.lock().unwrap();

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
}
