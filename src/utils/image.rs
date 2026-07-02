use crate::{ColorMode, Endian};
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{BufRead, Cursor, Error as IOError, ErrorKind, Read, Result, Seek, Write};

pub trait ImageIO {
    /// Converts some image in RGBA, BigEndian format to device specific data.
    fn rgba_to_device(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>>;
    /// Converts some device specific image data to RGBA, BigEndian format.
    fn device_to_rgba(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>>;

    /// Reads a PNG source and returns a byte buffer in the specified ̀color_mode`.
    fn read_png<R: Read + BufRead + Seek>(&self, reader: R) -> Result<(Vec<u8>, u32, u32)> {
        let (rgba, w, h) = png_to_rgba(reader)?;
        self.rgba_to_device(&rgba, w, h).map(|data| (data, w, h))
    }

    /// Reads a byte buffer of the specified color mode and returns PNG data.
    /// The PNG is always encoded in RGBA. If the source does not specify an
    /// alpha, then it is replaced by full opacity.
    fn write_png<W: Write>(&self, writer: W, data: &[u8], w: u32, h: u32) -> Result<()> {
        let rgba = self.device_to_rgba(data, w, h)?;
        rgba_to_png(writer, &rgba, w, h)
    }
}

impl ImageIO for ColorMode {
    /// Converts some image in RGBA, BigEndian format to device specific data.
    fn rgba_to_device(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
        match self {
            ColorMode::Rgba(Endian::Big) => Ok(rgba.to_vec()),
            ColorMode::Rgba(Endian::Little) => u32be_to_u32le(rgba, (w * h) as usize),
            ColorMode::Bgra(Endian::Big) => rgba_to_bgra::<BigEndian, _>(rgba, w, h),
            ColorMode::Bgra(Endian::Little) => rgba_to_bgra::<LittleEndian, _>(rgba, w, h),
            ColorMode::Rgb565(Endian::Big) => rgba_to_rgb565::<BigEndian, _>(rgba, w, h),
            ColorMode::Rgb565(Endian::Little) => rgba_to_rgb565::<LittleEndian, _>(rgba, w, h),
        }
    }

    /// Converts some device specific image data to RGBA, BigEndian format.
    fn device_to_rgba(&self, device: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
        match self {
            ColorMode::Rgba(Endian::Big) => Ok(device.to_vec()),
            ColorMode::Rgba(Endian::Little) => u32be_to_u32le(device, device.len()),
            ColorMode::Bgra(Endian::Big) => rgba_to_bgra::<BigEndian, _>(device, w, h),
            ColorMode::Bgra(Endian::Little) => rgba_to_bgra::<LittleEndian, _>(device, w, h),
            ColorMode::Rgb565(Endian::Big) => rgb565_to_rgba::<BigEndian>(device, w, h),
            ColorMode::Rgb565(Endian::Little) => rgb565_to_rgba::<LittleEndian>(device, w, h),
        }
    }
}

/// Reads a PNG source as bytes buffer the Rgba color mode.
pub fn png_to_rgba<R: Read + BufRead + Seek>(reader: R) -> Result<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(reader);
    let mut png_reader = decoder.read_info()?;
    let buffer_size = png_reader
        .output_buffer_size()
        .ok_or_else(|| IOError::new(ErrorKind::InvalidData, "png has no output buffer size"))?;
    // Allocate the output buffer.
    let mut buf = vec![0; buffer_size];
    // png is supposed to contain a single frame.
    let info = png_reader.next_frame(&mut buf)?;
    Ok((buf, info.width, info.height))
}

/// Clears the alpha channel (set to 0xFF).
/// In general, you want to change the "big logo" and alpha is of no use.
/// Forcing it to a constant value may save precious bytes when deflating.
pub fn strip_alpha(data: &mut [u8]) {
    let words = data.len() / 4;
    // alpha is 4th bytes in rgba.
    let mut offset = 3;
    for _ in 0..words {
        data[offset] = 0xFF;
        offset += 4;
    }
}

/// Writes an Rgba color mode byte buffer as PNG.
pub fn rgba_to_png<W: Write>(writer: W, data: &[u8], w: u32, h: u32) -> Result<()> {
    let mut encoder = png::Encoder::new(writer, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header()?;
    png_writer
        .write_image_data(data)
        .map_err(|e| IOError::new(ErrorKind::InvalidData, e.to_string()))
}

/// Converts RGBA byte buffer to BGRA with the specified endianness.
pub fn rgba_to_bgra<O: ByteOrder, R: Read>(mut reader: R, w: u32, h: u32) -> Result<Vec<u8>> {
    let pixels = (w * h) as usize;
    let mut bgra: Vec<u8> = Vec::with_capacity(pixels * 4);
    for _ in 0..pixels {
        // 'pivot' rgba is always BigEndian.
        let color32 = reader.read_u32::<BigEndian>()?;
        bgra.write_u32::<O>(rgba2bgra(color32))?;
    }
    Ok(bgra)
}

/// Converts RGBA Big Endian to RGBA LittleEndian. It works also the other way round...
pub fn u32be_to_u32le<R: Read>(mut reader: R, words: usize) -> Result<Vec<u8>> {
    let mut rgbale: Vec<u8> = Vec::with_capacity(words * 4);
    for _ in 0..words {
        // 'pivot' rgba is always BigEndian.
        let color32 = reader.read_u32::<BigEndian>()?;
        // Endianness conversion must preserve channel order.
        rgbale.write_u32::<LittleEndian>(color32)?;
    }
    Ok(rgbale)
}

/// Converts RGBA byte buffer to Rgb565 with the specified endianness.
pub fn rgba_to_rgb565<O: ByteOrder, R: Read>(mut reader: R, w: u32, h: u32) -> Result<Vec<u8>> {
    let pixels = (w * h) as usize;
    let mut rgb565: Vec<u8> = Vec::with_capacity(pixels * 2);
    for _ in 0..pixels {
        // 'pivot' rgba is always BigEndian.
        let color32 = reader.read_u32::<BigEndian>()?;
        rgb565.write_u16::<O>(rgba2rgb565(color32))?;
    }
    Ok(rgb565)
}

/// Converts Rgba565 with specified endianness byte buffer as RGBA.
pub fn rgb565_to_rgba<B: ByteOrder>(data: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    // we'll expand the rgb565 into rgba; it'll take twice the space.
    let pixels = (w * h) as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let mut data_reader = Cursor::new(data);
    for _ in 0..pixels {
        let color16 = data_reader.read_u16::<B>()?;
        let color32 = rgb5652rgba(color16);
        // 'pivot' rgba is always BigEndian.
        rgba.write_u32::<BigEndian>(color32)?;
    }
    Ok(rgba)
}

#[inline(always)]
fn rgba2bgra(color32: u32) -> u32 {
    let r = (color32 & 0xFF000000) >> 16;
    let b = (color32 & 0x0000FF00) << 16;
    let ga = color32 & 0x00FF00FF;
    r | b | ga
}

#[inline(always)]
fn rgba2rgb565(color32: u32) -> u16 {
    let r = ((color32 & 0xF8000000) >> 16) as u16;
    let g = ((color32 & 0x00FC0000) >> 13) as u16;
    let b = ((color32 & 0x0000F800) >> 11) as u16;
    r | g | b
}

#[inline(always)]
fn rgb5652rgba(color16: u16) -> u32 {
    let r = ((color16 & 0xF800) as u32) << 16;
    let g = ((color16 & 0x07E0) as u32) << 13;
    let b = ((color16 & 0x001F) as u32) << 11;
    r | g | b | 0xFF
}

#[test]
fn test_i_do_not_mess_up_bitwise_ops() {
    fn test_pair(color32: u32, color16: u16) {
        assert_eq!(rgba2rgb565(color32), color16);
        // of course, we'll loose a few bits, thus the mask... + alpha.
        assert_eq!(rgb5652rgba(color16), color32 & 0xF8FCF800 | 0xFF);
    }
    // White...
    test_pair(0xFFFFFF00, 0xFFFF);
    // Red...
    test_pair(0xFF000000, 0xF800);
    // Green...
    test_pair(0x00FF0000, 0x07E0);
    // Blue...
    test_pair(0x0000FF00, 0x001F);
}

#[test]
fn test_i_do_not_mess_up_bigendian() {
    // red in rgb565 model.
    let color16 = [0xF8_u8, 0x00];
    assert_eq!(0xF8_u8, color16[0]);
    assert_eq!(0x00_u8, color16[1]);
    assert_eq!(0xF800_u16, (&color16[..]).read_u16::<BigEndian>().unwrap());
    // convert it using writers
    let converted = rgb565_to_rgba::<BigEndian>(&color16, 1, 1).unwrap();
    // red in rgba model full opacity.
    assert_eq!(
        0xF80000FF_u32,
        (&converted[..]).read_u32::<BigEndian>().unwrap()
    );
}

#[test]
fn test_i_do_not_mess_up_littleendian() {
    // red in rgb565 model, little endian.
    let color16 = [0x00, 0xF8_u8];
    assert_eq!(0x00_u8, color16[0]);
    assert_eq!(0xF8_u8, color16[1]);
    assert_eq!(
        0xF800_u16,
        (&color16[..]).read_u16::<LittleEndian>().unwrap()
    );
    // convert it using writers
    let converted = rgb565_to_rgba::<LittleEndian>(&color16, 1, 1).unwrap();
    // red in rgba model full opacity (I want rgba to be a 'pivot' format, always in BigEndian).
    assert_eq!(
        0xF80000FF_u32,
        (&converted[..]).read_u32::<BigEndian>().unwrap()
    );
}

#[test]
fn test_u32_endianness_conversion_preserves_rgba_channels() {
    let rgba_be = [0x12_u8, 0x34, 0x56, 0x78];
    let rgba_le = u32be_to_u32le(&rgba_be[..], 1).unwrap();
    let roundtrip = u32be_to_u32le(&rgba_le[..], 1).unwrap();
    assert_eq!(rgba_be.to_vec(), roundtrip);
}
