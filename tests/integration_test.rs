// These integration tests do as little as checking:
// - whether the external crates (zlib/png) are fitted for the purpose of this program.
// - if I'm not too bad with image formats...

use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt};
use mtklogo::LogoImage;
use mtklogo::utils::{image, load_raw, z_lib};
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Result, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

fn test_folder() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("resources/tests");
    d
}

/// Compares rasters pixel for pixel, omitting a few bits specified by the mask.
fn compare_rasters<A: Read, B: Read>(mut a: A, mut b: B, pixels: usize, mask: u32) {
    for _pixels in 0..pixels {
        let before = a.read_u32::<BigEndian>().unwrap();
        let after = b.read_u32::<BigEndian>().unwrap();
        assert_eq!(before & mask, after & mask);
    }
}
/// Utility function to get a grasp at the raw memory...
pub fn rgba_to_ppm<W: Write>(mut writer: W, data: &[u8], w: u32, h: u32) -> Result<()> {
    // see http://rosettacode.org/wiki/Bitmap/Write_a_PPM_file#Rust
    let header = format!("P6 {} {} 255\n", w, h);
    writer.write_all(header.as_bytes())?;
    let pixels = w * h;
    let mut offset = 0;
    for _ in 0..pixels {
        let rgb = [data[offset], data[offset + 1], data[offset + 2]];
        writer.write_all(&rgb).unwrap();
        offset += 4;
    }
    Ok(())
}

fn png_to_raster_z(png: &[u8]) -> Vec<u8> {
    // Encodes it as RGBA.
    let (rgba, _, _) = image::png_to_rgba(Cursor::new(png)).unwrap();
    // zips it
    z_lib::deflate(&rgba).unwrap()
}

// just to avoid loading the samples too many times...
static IMAGE1_PNG: OnceLock<Vec<u8>> = OnceLock::new();
static IMAGE2_PNG: OnceLock<Vec<u8>> = OnceLock::new();
static IMAGE1_Z: OnceLock<Vec<u8>> = OnceLock::new();
static IMAGE2_Z: OnceLock<Vec<u8>> = OnceLock::new();
static SAMPLE: OnceLock<LogoImage> = OnceLock::new();

fn image1_png() -> &'static Vec<u8> {
    IMAGE1_PNG.get_or_init(|| {
        let file = File::open(test_folder().join("white-lotus-flower-bud.png")).unwrap();
        load_raw(file).unwrap()
    })
}

fn image2_png() -> &'static Vec<u8> {
    IMAGE2_PNG.get_or_init(|| {
        let file = File::open(test_folder().join("boat-at-sunrise-1488476212bRg.png")).unwrap();
        load_raw(file).unwrap()
    })
}

fn image1_z() -> &'static Vec<u8> {
    IMAGE1_Z.get_or_init(|| png_to_raster_z(image1_png()))
}

fn image2_z() -> &'static Vec<u8> {
    IMAGE2_Z.get_or_init(|| png_to_raster_z(image2_png()))
}

fn sample() -> &'static LogoImage {
    SAMPLE.get_or_init(|| {
        LogoImage::new_blobs(vec![image1_z().clone(), image2_z().clone()])
            .expect("sample logo image must build")
    })
}

#[test]
fn dither_rgb565() {
    fn test_it<O: ByteOrder>() {
        // Gets an RGBA image
        let (rgba, w, h) = image::png_to_rgba(Cursor::new(image1_png().as_slice())).unwrap();
        // encodes it as rgb565
        let rgba565 = image::rgba_to_rgb565::<O, _>(&rgba[..], w, h).unwrap();
        // It must be halved in size.
        assert_eq!(rgba.len() / 2, rgba565.len());
        // encodes it again as rgb
        let rgb_again = image::rgb565_to_rgba::<O>(&rgba565, w, h).unwrap();
        // Manual test:
        // saves it as a .ppm, you wil manually tell if it is a "good" dithering :)
        // let out = File::create("/tmp/mtklogo_rs_dither_rgb565.ppm").unwrap();
        // rgba_to_ppm(out, &rgb_again, w, h).unwrap();
        // ok, no, you won't do that each test... and don't want to have disk space filled
        // by my library, so we can do a few checks in memory:
        assert_eq!(rgb_again.len(), rgba.len());
        // let's check that we have the same images but with a little degradation...
        compare_rasters(&rgba[..], &rgb_again[..], (w * h) as usize, 0xF8FCF800);
    }
    // It should dither the same for big or little endian...
    test_it::<BigEndian>();
    test_it::<LittleEndian>();
}

/// We just test that we can read and write (serialize) a well crafted logo image file.
#[test]
fn can_explode() {
    const EXPECTED_IMAGES: usize = 2;
    let s = sample();
    assert_eq!(s.blobs.len(), EXPECTED_IMAGES);
    assert_eq!(s.table.logo_count, EXPECTED_IMAGES as u32);
    assert_eq!(s.table.offsets.len(), EXPECTED_IMAGES);
    // I can re-assemble without crashing
    let mut writer = BufWriter::new(Vec::<u8>::with_capacity(2_000_000));
    s.write(&mut writer).unwrap();
}

/// We check that compressing/decompressing the same data leads to 'equivalent' payloads.
#[test]
fn zlib_is_quite_symetric() {
    // Let's take some already compressed data.
    let blob1 = image1_z();
    // decompresses it
    let decompressed = z_lib::inflate(&blob1[..]).unwrap();
    let recompressed = z_lib::deflate(&decompressed).unwrap();
    let grow_ratio = (100 * (recompressed.len() - blob1.len())) / blob1.len();
    println!("{} - {} - {}%", blob1.len(), recompressed.len(), grow_ratio);

    #[cfg(feature = "flate2")]
    {
        // With flate2, since it wraps the system zlib, it must be the exact same bytes!
        assert_eq!(blob1, &recompressed);
    }

    #[cfg(feature = "libflate")]
    {
        // If we don't grow bigger than 15% the original size, it's OK...
        // In fact, it's 14%. That's a difference indeed.
        assert!(grow_ratio < 15);
    }
}

/// We check that converting from raster to PNG back and forth does not change a single bit!
#[test]
fn png_is_not_lossy() {
    // takes our raw sample PNG.
    let (raster, w, h) = image::png_to_rgba(Cursor::new(image1_png().as_slice())).unwrap();
    assert_eq!(w, 720);
    assert_eq!(h, 1080);
    // manual test: if you want to check the decoded file.
    // let out = File::create("/tmp/png_exploded.ppm").unwrap();
    // rgba_to_ppm(out, &raster, w, h).unwrap();

    // converts it to PNG (assume it will occupy less than a quarter of memory...)
    let mut png_data = Vec::<u8>::with_capacity(raster.len() >> 2);
    // saves it again as a PNG
    println!("SAVE...");
    image::rgba_to_png(&mut png_data, &raster, w, h).unwrap();

    // converts it one more time to raster!
    let (raster_again, ww, hh) = image::png_to_rgba(Cursor::new(&png_data)).unwrap();
    assert_eq!(w, ww);
    assert_eq!(h, hh);

    // Hopefully: decode(encode(x)) = x...
    assert_eq!(raster, raster_again);
}
