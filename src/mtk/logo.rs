use super::header::{MtkHeader, MtkType};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Error as IOError, ErrorKind, Read, Result, Seek, SeekFrom, Write};

/// The raw logo binary's table: the 512-byte MTK header (parsed
/// structurally, including the vendor "extended" block) followed by the
/// offset table.
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

/// The whole logo image (table + blobs + cert).
pub struct LogoImage {
    pub table: LogoTable,
    pub blobs: Vec<Vec<u8>>,
    /// Cert bytes (cert1/cert2 + padding) following the blobs.
    pub cert: Vec<u8>,
    /// Bytes of `cert` counted inside `dsize` (`dsize - block_size`).
    pub cert_inner_len: u32,
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

    /// Writes the logo table (header + offset table).
    pub fn write<W: Write>(&self, mut writer: &mut W) -> Result<()> {
        self.header.write(&mut writer)?;
        self.write_offsets(&mut writer)
    }

    /// Writes only the offset table (logo_count + block_size + offsets),
    /// without the 512-byte header.
    pub fn write_offsets<W: Write>(&self, writer: &mut W) -> Result<()> {
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
        let count = offsets.len();
        // Validate the index up front: indexing `offsets` directly would
        // panic on an out-of-range request instead of returning an error.
        if i >= count {
            return Err(IOError::new(
                ErrorKind::InvalidInput,
                format!("blob index {} is out of range (0..{})", i, count),
            ));
        }
        let offset = offsets[i];
        let next_offset = if i + 1 < count {
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
        // Capture the cert trailing the blobs for re-append on repack.
        let blob_end = MtkHeader::SIZE as u64 + table.block_size as u64;
        let end = reader.seek(SeekFrom::End(0))?;
        let mut cert = Vec::new();
        if end > blob_end {
            reader.seek(SeekFrom::Start(blob_end))?;
            reader.read_to_end(&mut cert)?;
        }
        let cert_inner_len = table.header.size.saturating_sub(table.block_size);
        Ok(LogoImage {
            table,
            blobs,
            cert,
            cert_inner_len,
        })
    }

    /// Given a list of blobs, creates a complete logo image with a default
    /// extended header (as used by modern MTK devices) and no cert.
    pub fn new_blobs(blobs: Vec<Vec<u8>>) -> Result<LogoImage> {
        let mut image = LogoImage {
            table: LogoTable {
                header: MtkHeader::new_extended(MtkType::Logo, false, 0),
                logo_count: 0,
                block_size: 0,
                offsets: Vec::new(),
            },
            blobs,
            cert: Vec::new(),
            cert_inner_len: 0,
        };
        image.table = image.rebuild_table()?;
        Ok(image)
    }

    /// Recomputes `logo_count`, `block_size`, `offsets` and `header.size`
    /// from the current `blobs` (and `cert_inner_len`). Call after mutating
    /// `blobs` through the public API.
    pub fn rebuild_table(&self) -> Result<LogoTable> {
        let mut offsets: Vec<u32> = Vec::with_capacity(self.blobs.len());
        let mut offset: u32 = (2 + self.blobs.len() as u32)
            .checked_mul(4)
            .ok_or_else(|| IOError::new(ErrorKind::InvalidInput, "offset table overflow"))?;
        for blob in self.blobs.iter() {
            offsets.push(offset);
            offset = offset
                .checked_add(blob.len() as u32)
                .ok_or_else(|| IOError::new(ErrorKind::InvalidInput, "blob offset overflow"))?;
        }
        let block_size = offset;
        let dsize = block_size
            .checked_add(self.cert_inner_len)
            .ok_or_else(|| IOError::new(ErrorKind::InvalidInput, "dsize overflow"))?;
        let mut header = self.table.header;
        header.size = dsize;
        Ok(LogoTable {
            header,
            logo_count: self.blobs.len() as u32,
            block_size,
            offsets,
        })
    }

    /// Refreshes `table` in place to match the current `blobs`.
    pub fn refresh(&mut self) -> Result<()> {
        self.table = self.rebuild_table()?;
        Ok(())
    }

    /// Writes the complete logo image. The offset table, `block_size` and
    /// `dsize` are recomputed from the blobs; the cert is re-appended at
    /// `align_up(512 + dsize, align_size)` so cert1/cert2 stay detectable.
    pub fn write<W: Write>(&self, mut writer: &mut W) -> Result<()> {
        let table = self.rebuild_table()?;
        table.header.write(&mut writer)?;
        table.write_offsets(&mut writer)?;
        for blob in self.blobs.iter() {
            writer.write_all(blob)?;
        }
        let inner_len = self.cert_inner_len as usize;
        writer.write_all(self.cert.get(..inner_len).unwrap_or(&[]))?;
        let outer = self.cert.get(inner_len..).unwrap_or(&[]);
        if !outer.is_empty() {
            let align = table.header.align_size as usize;
            if align > 0 {
                let data_end = MtkHeader::SIZE + table.header.size as usize;
                let rem = data_end % align;
                if rem != 0 {
                    writer.write_all(&vec![0u8; align - rem])?;
                }
            }
            writer.write_all(outer)?;
        }
        Ok(())
    }
}
