use super::StartExt;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::io::{Error as IOError, ErrorKind, Read, Result, Write};

/// An MTK image header (512 bytes), parsed and built structurally.
///
/// Layout:
/// ```text
/// 0x00 magic        u32 BE  (0x88168858)
/// 0x04 dsize        u32 LE  payload size (may include a cert region)
/// 0x08 name         [u8;32] image type label, NUL-padded
/// 0x28 addr         u32 LE
/// 0x2c mode         u32 LE
/// 0x30 ext_magic    u32 BE  (0x89168958 when the extended block is present)
/// 0x34 hdr_size     u32 LE  (0x200)
/// 0x38 hdr_ver      u32 LE
/// 0x3c img_type     u32 LE
/// 0x40 img_list_end u32 LE
/// 0x44 align_size   u32 LE
/// 0x48 dsize_ext    u32 LE
/// 0x4c addr_ext     u32 LE
/// 0x50 scrambled    u32 LE
/// 0x54 reserved     [u8;428]
/// ```
/// Without `ext_magic` the header is legacy and 0x28..0x200 is 0xFF fill.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MtkHeader {
    /// `dsize`: payload size (may include a cert region, so it can exceed
    /// the offset-table `block_size`).
    pub size: u32,
    pub mtk_type: MtkType,
    /// true = "LOGO", false = "logo"
    pub legacy_logo: bool,
    /// Whether the extended block (`ext_magic` at 0x30) is present.
    pub extended: bool,
    /// Raw 32-byte type-label at offset 0x08, preserved verbatim.
    pub name: [u8; 32],
    pub addr: u32,
    pub mode: u32,
    pub hdr_size: u32,
    pub hdr_ver: u32,
    pub img_type: u32,
    pub img_list_end: u32,
    pub align_size: u32,
    pub dsize_ext: u32,
    pub addr_ext: u32,
    pub scrambled: u32,
    /// Vendor-specific tail of the extended block (0x54..0x200), preserved
    /// verbatim.
    pub reserved: [u8; 428],
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MtkType {
    Recovery,
    Rootfs,
    Kernel,
    Logo,
}

impl MtkType {
    /// Tests whether the specified "magic bytes" correspond to some possible mtk image type.
    fn from_bytes(bytes: &[u8]) -> Option<MtkType> {
        const LABELS: [(MtkType, &[u8]); 4] = [
            (MtkType::Recovery, b"RECOVERY"),
            (MtkType::Rootfs, b"ROOTFS"),
            (MtkType::Kernel, b"KERNEL"),
            (MtkType::Logo, b"LOGO"),
        ];
        for (mtk_type, label) in LABELS {
            if bytes.starts_with_ascii_ignore_case(label) {
                return Some(mtk_type);
            }
        }
        None
    }

    /// The 32-byte type-label bytes written at offset 0x08.
    fn label_bytes(&self, legacy_logo: bool) -> [u8; 32] {
        let mut buf = [0u8; 32];
        let label: &[u8] = match (*self, legacy_logo) {
            (MtkType::Logo, true) => b"LOGO",
            (MtkType::Logo, false) => b"logo",
            (MtkType::Recovery, _) => b"RECOVERY",
            (MtkType::Kernel, _) => b"KERNEL",
            (MtkType::Rootfs, _) => b"ROOTFS",
        };
        buf[..label.len()].copy_from_slice(label);
        buf
    }
}

impl MtkHeader {
    pub const SIZE: usize = 512;
    pub const FILL: u8 = 0xFF;
    pub const MAGIC: u32 = 0x88168858;
    /// Extended-block magic, stored big-endian at offset 0x30.
    pub const MAGIC_EXT: u32 = 0x89168958;
    /// Default alignment (`align_size`) used when building a new header.
    pub const DEFAULT_ALIGN_SZ: u32 = 0x10;
    const NAME_OFF: usize = 0x08;
    const ADDR_OFF: usize = 0x28;
    const MODE_OFF: usize = 0x2c;
    const EXT_MAGIC_OFF: usize = 0x30;
    const HDR_SIZE_OFF: usize = 0x34;
    const HDR_VER_OFF: usize = 0x38;
    const IMG_TYPE_OFF: usize = 0x3c;
    const IMG_LIST_END_OFF: usize = 0x40;
    const ALIGN_OFF: usize = 0x44;
    const DSIZE_EXT_OFF: usize = 0x48;
    const ADDR_EXT_OFF: usize = 0x4c;
    const SCRAMBLED_OFF: usize = 0x50;
    const RESERVED_OFF: usize = 0x54;

    /// Builds a legacy (non-extended) header for a payload of `dsize` bytes.
    pub fn new_legacy(mtk_type: MtkType, legacy_logo: bool, dsize: u32) -> Self {
        MtkHeader {
            size: dsize,
            mtk_type,
            legacy_logo,
            extended: false,
            name: mtk_type.label_bytes(legacy_logo),
            addr: 0xFFFF_FFFF,
            mode: 0xFFFF_FFFF,
            hdr_size: 0,
            hdr_ver: 0,
            img_type: 0,
            img_list_end: 0,
            align_size: 0,
            dsize_ext: 0,
            addr_ext: 0,
            scrambled: 0,
            reserved: [Self::FILL; 428],
        }
    }

    /// Builds an extended header for a payload of `dsize` bytes, with the
    /// default extension field values used by modern MTK logo images.
    pub fn new_extended(mtk_type: MtkType, legacy_logo: bool, dsize: u32) -> Self {
        MtkHeader {
            size: dsize,
            mtk_type,
            legacy_logo,
            extended: true,
            name: mtk_type.label_bytes(legacy_logo),
            addr: 0xFFFF_FFFF,
            mode: 0xFFFF_FFFF,
            hdr_size: Self::SIZE as u32,
            hdr_ver: 1,
            img_type: 0,
            img_list_end: 0,
            align_size: Self::DEFAULT_ALIGN_SZ,
            dsize_ext: 0,
            addr_ext: 0,
            scrambled: 0,
            reserved: [Self::FILL; 428],
        }
    }

    /// Reads a 512-byte header, parsing the extended block when present.
    pub fn read<R: Read>(reader: &mut R) -> Result<MtkHeader> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;
        let magic = BigEndian::read_u32(&buf[..4]);
        if magic != Self::MAGIC {
            return Err(IOError::new(ErrorKind::InvalidData, "missing magic number"));
        }
        let size = LittleEndian::read_u32(&buf[4..8]);
        let mut name = [0u8; 32];
        name.copy_from_slice(&buf[Self::NAME_OFF..Self::ADDR_OFF]);
        let legacy_logo = name.starts_with(b"LOGO");
        let mtk_type = MtkType::from_bytes(&name)
            .ok_or_else(|| IOError::new(ErrorKind::InvalidData, "Missing MTK Header type."))?;
        let addr = LittleEndian::read_u32(&buf[Self::ADDR_OFF..Self::MODE_OFF]);
        let mode = LittleEndian::read_u32(&buf[Self::MODE_OFF..Self::EXT_MAGIC_OFF]);
        let ext_magic = BigEndian::read_u32(&buf[Self::EXT_MAGIC_OFF..Self::HDR_SIZE_OFF]);
        if ext_magic == Self::MAGIC_EXT {
            let mut reserved = [0u8; 428];
            reserved.copy_from_slice(&buf[Self::RESERVED_OFF..Self::SIZE]);
            Ok(MtkHeader {
                size,
                mtk_type,
                legacy_logo,
                extended: true,
                name,
                addr,
                mode,
                hdr_size: LittleEndian::read_u32(&buf[Self::HDR_SIZE_OFF..Self::HDR_VER_OFF]),
                hdr_ver: LittleEndian::read_u32(&buf[Self::HDR_VER_OFF..Self::IMG_TYPE_OFF]),
                img_type: LittleEndian::read_u32(&buf[Self::IMG_TYPE_OFF..Self::IMG_LIST_END_OFF]),
                img_list_end: LittleEndian::read_u32(&buf[Self::IMG_LIST_END_OFF..Self::ALIGN_OFF]),
                align_size: LittleEndian::read_u32(&buf[Self::ALIGN_OFF..Self::DSIZE_EXT_OFF]),
                dsize_ext: LittleEndian::read_u32(&buf[Self::DSIZE_EXT_OFF..Self::ADDR_EXT_OFF]),
                addr_ext: LittleEndian::read_u32(&buf[Self::ADDR_EXT_OFF..Self::SCRAMBLED_OFF]),
                scrambled: LittleEndian::read_u32(&buf[Self::SCRAMBLED_OFF..Self::RESERVED_OFF]),
                reserved,
            })
        } else {
            Ok(MtkHeader {
                size,
                mtk_type,
                legacy_logo,
                extended: false,
                name,
                addr,
                mode,
                hdr_size: 0,
                hdr_ver: 0,
                img_type: 0,
                img_list_end: 0,
                align_size: 0,
                dsize_ext: 0,
                addr_ext: 0,
                scrambled: 0,
                reserved: [Self::FILL; 428],
            })
        }
    }

    /// Serializes this header to a full 512-byte block, building the
    /// extended block from the parsed fields when `extended` is set.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut buf = [Self::FILL; Self::SIZE];
        BigEndian::write_u32(&mut buf[..4], Self::MAGIC);
        LittleEndian::write_u32(&mut buf[4..8], self.size);
        buf[Self::NAME_OFF..Self::ADDR_OFF].copy_from_slice(&self.name);
        LittleEndian::write_u32(&mut buf[Self::ADDR_OFF..Self::MODE_OFF], self.addr);
        LittleEndian::write_u32(&mut buf[Self::MODE_OFF..Self::EXT_MAGIC_OFF], self.mode);
        if self.extended {
            BigEndian::write_u32(
                &mut buf[Self::EXT_MAGIC_OFF..Self::HDR_SIZE_OFF],
                Self::MAGIC_EXT,
            );
            LittleEndian::write_u32(
                &mut buf[Self::HDR_SIZE_OFF..Self::HDR_VER_OFF],
                self.hdr_size,
            );
            LittleEndian::write_u32(
                &mut buf[Self::HDR_VER_OFF..Self::IMG_TYPE_OFF],
                self.hdr_ver,
            );
            LittleEndian::write_u32(
                &mut buf[Self::IMG_TYPE_OFF..Self::IMG_LIST_END_OFF],
                self.img_type,
            );
            LittleEndian::write_u32(
                &mut buf[Self::IMG_LIST_END_OFF..Self::ALIGN_OFF],
                self.img_list_end,
            );
            LittleEndian::write_u32(
                &mut buf[Self::ALIGN_OFF..Self::DSIZE_EXT_OFF],
                self.align_size,
            );
            LittleEndian::write_u32(
                &mut buf[Self::DSIZE_EXT_OFF..Self::ADDR_EXT_OFF],
                self.dsize_ext,
            );
            LittleEndian::write_u32(
                &mut buf[Self::ADDR_EXT_OFF..Self::SCRAMBLED_OFF],
                self.addr_ext,
            );
            LittleEndian::write_u32(
                &mut buf[Self::SCRAMBLED_OFF..Self::RESERVED_OFF],
                self.scrambled,
            );
            buf[Self::RESERVED_OFF..Self::SIZE].copy_from_slice(&self.reserved);
        }
        writer.write_all(&buf)?;
        Ok(())
    }
}
