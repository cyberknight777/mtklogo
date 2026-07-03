use mtklogo::{LogoImage, MtkHeader};
use std::io::{Cursor, Write};
use std::path::Path;

/// Magic prefix for the `.mtklogo-meta` sidecar (versioned layout).
pub(crate) const META_MAGIC: [u8; 4] = *b"MTKL";
const HEADER_LEN: usize = MtkHeader::SIZE;
const PREFIX_LEN: usize = META_MAGIC.len() + 4 + HEADER_LEN;

pub(crate) struct Meta {
    pub header: Vec<u8>,
    pub cert: Vec<u8>,
    pub cert_inner_len: u32,
}

/// Layout: `[META_MAGIC][cert_inner_len: u32 LE][512-B header][cert]`.
pub(crate) fn write(dir: &Path, image: &LogoImage) -> std::io::Result<()> {
    let mut f = std::fs::File::create(dir.join(".mtklogo-meta"))?;
    f.write_all(&META_MAGIC)?;
    f.write_all(&image.cert_inner_len.to_le_bytes())?;
    image.table.header.write(&mut f)?;
    f.write_all(&image.cert)?;
    Ok(())
}

/// Loads the sidecar. Returns `None` when absent, truncated, or unrecognized.
pub(crate) fn load(dir: &Path) -> Option<Meta> {
    let buf = std::fs::read(dir.join(".mtklogo-meta")).ok()?;
    if buf.len() < PREFIX_LEN {
        return None;
    }
    if buf[..META_MAGIC.len()] != META_MAGIC {
        return None;
    }
    let cert_inner_len = u32::from_le_bytes(
        buf[META_MAGIC.len()..META_MAGIC.len() + 4]
            .try_into()
            .ok()?,
    );
    let header = buf[META_MAGIC.len() + 4..PREFIX_LEN].to_vec();
    let cert = buf[PREFIX_LEN..].to_vec();
    // Reject a sidecar whose header does not parse: repack would otherwise
    // build from a default header.
    if MtkHeader::read(&mut Cursor::new(&header)).is_err() {
        return None;
    }
    Some(Meta {
        header,
        cert,
        cert_inner_len,
    })
}
