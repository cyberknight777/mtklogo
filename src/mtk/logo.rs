use super::header::{MtkHeader, MtkType};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Error as IOError, ErrorKind, Read, Result, Seek, SeekFrom, Write};

/// The raw logo binary's header, we only keep "relevant" information.
/// Data like padding or non-meaningful bytes are not preserved.
#[derive(Debug)]
pub struct LogoTable {
    /// Mtk Header
    pub header: MtkHeader,
    /// Number of logos
    pub logo_count: u32,
    /// size of a block
    pub block_size: u32,
    /// offset of each blob
    pub offsets: Vec<u32>,
}

/// The whole logo image (table + blobs).
pub struct LogoImage {
    pub table: LogoTable,
    pub blobs: Vec<Vec<u8>>,
}

impl LogoTable {
    /// Reads a logo table.
    pub fn read<R: Read>(mut reader: R) -> Result<LogoTable> {
        // reads the header
        let header = MtkHeader::read(&mut reader)?;
        // It must be a logo!
        match header.mtk_type {
            MtkType::Logo => (),
            _ => {
                return Err(IOError::new(
                    ErrorKind::InvalidData,
                    "MTK image is not a logo",
                ));
            }
        };
        // now we have the number of image
        let logo_count: u32 = reader.read_u32::<LittleEndian>()?;
        // and the block size
        let block_size: u32 = reader.read_u32::<LittleEndian>()?;
        if block_size > header.size {
            return Err(IOError::new(
                ErrorKind::InvalidData,
                format!(
                    "MTK Header size '{:0x}' is smaller than block size '{:0x}'",
                    header.size, block_size
                ),
            ));
        }
        // The offsets table (8 B prefix + 4 B per entry) must fit in the
        // declared header size, otherwise a crafted `logo_count` could
        // trigger a multi-gigabyte `Vec::with_capacity` before reads fail.
        let table_bytes = 8u64 + (logo_count as u64) * 4;
        if table_bytes > header.size as u64 {
            return Err(IOError::new(
                ErrorKind::InvalidData,
                format!(
                    "logo table declares {} entries ({} B) which exceeds header size '{:0x}'",
                    logo_count, table_bytes, header.size
                ),
            ));
        }
        let mut offsets: Vec<u32> = Vec::with_capacity(logo_count as usize);
        for _ in 0..(logo_count as usize) {
            offsets.push(reader.read_u32::<LittleEndian>()?);
        }
        // Sanity-check the offset table before anyone tries to extract blobs.
        // Blobs live in the data section right after this table, so a valid
        // offset must be >= table size, <= block_size and the sequence must be
        // non-decreasing (otherwise `next_offset - offset` would underflow).
        let min_offset = (2u64 + logo_count as u64) * 4;
        let mut prev = 0u32;
        for (i, &offset) in offsets.iter().enumerate() {
            if (offset as u64) < min_offset {
                return Err(IOError::new(
                    ErrorKind::InvalidData,
                    format!(
                        "blob offset {:#x} (index {}) is below the table size {:#x}",
                        offset, i, min_offset
                    ),
                ));
            }
            if offset > block_size {
                return Err(IOError::new(
                    ErrorKind::InvalidData,
                    format!(
                        "blob offset {:#x} (index {}) exceeds block size {:#x}",
                        offset, i, block_size
                    ),
                ));
            }
            if offset < prev {
                return Err(IOError::new(
                    ErrorKind::InvalidData,
                    format!(
                        "blob offsets are not in non-decreasing order at index {} ({:#x} < {:#x})",
                        i, offset, prev
                    ),
                ));
            }
            prev = offset;
        }
        Ok(LogoTable {
            header,
            logo_count,
            block_size,
            offsets,
        })
    }

    /// Writes the logo table (the table only, not the logos).
    pub fn write<W: Write>(&self, mut writer: &mut W) -> Result<()> {
        self.header.write(&mut writer)?;
        writer.write_u32::<LittleEndian>(self.logo_count)?;
        writer.write_u32::<LittleEndian>(self.block_size)?;
        for offset in self.offsets.iter() {
            writer.write_u32::<LittleEndian>(*offset)?;
        }
        Ok(())
    }

    /// Given this logo table, extract the logos as blobs from the specified reader.
    pub fn read_blobs<R: Read + Seek>(&self, mut reader: &mut R) -> Result<Vec<Vec<u8>>> {
        // Computes image slots
        let logo_count = self.logo_count as usize;
        let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(logo_count);
        for i in 0..logo_count {
            blobs.push(self.read_blob(&mut reader, i)?);
        }
        Ok(blobs)
    }

    /// Given this logo table, extract the i-th logo as blobs from the specified reader.
    pub fn read_blob<R: Read + Seek>(&self, reader: &mut R, i: usize) -> Result<Vec<u8>> {
        let offsets = &self.offsets;
        let logo_count = self.logo_count as usize;
        let offset = offsets[i];
        let next_offset = if i < logo_count - 1 {
            offsets[i + 1]
        } else {
            self.block_size
        };
        let size = next_offset.checked_sub(offset).ok_or_else(|| {
            IOError::new(
                ErrorKind::InvalidData,
                format!(
                    "blob {} has a negative size (offset {:#x} > next {:#x})",
                    i, offset, next_offset
                ),
            )
        })?;
        // We must inflate the image to guess its dimensions.
        reader.seek(SeekFrom::Start(offset as u64 + MtkHeader::SIZE as u64))?;
        // reads the whole image block in memory.
        let mut data: Vec<u8> = vec![0; size as usize];
        reader.read_exact(&mut data)?;
        Ok(data)
    }
}

impl LogoImage {
    /// Reads a complete logo image from a binary stream.
    pub fn read<R: Read + Seek>(mut reader: &mut R) -> Result<LogoImage> {
        // reads raw data structure.
        let table = LogoTable::read(&mut reader)?;
        // extracts images
        let blobs = table.read_blobs(&mut reader)?;
        Ok(LogoImage { table, blobs })
    }

    /// Given a list of blobs, creates a complete logo image.
    pub fn new_blobs(blobs: Vec<Vec<u8>>) -> LogoImage {
        let mut offsets: Vec<u32> = Vec::with_capacity(blobs.len());
        // first block will be located just after offsets table.
        let mut offset: u32 = (2 + blobs.len() as u32) * 4;
        for blob in blobs.iter() {
            offsets.push(offset);
            offset += blob.len() as u32;
        }
        let block_size = offset;
        let header = MtkHeader {
            size: block_size,
            mtk_type: MtkType::Logo,
            legacy_logo: false, // new format is the default.
        };
        let table = LogoTable {
            header,
            logo_count: blobs.len() as u32,
            block_size,
            offsets,
        };
        LogoImage { table, blobs }
    }

    /// Writes this complete logo image to the specified writer.
    pub fn write<W: Write>(&self, mut writer: &mut W) -> Result<()> {
        self.table.write(&mut writer)?;
        for blob in self.blobs.iter() {
            writer.write_all(blob)?;
        }
        Ok(())
    }
}
