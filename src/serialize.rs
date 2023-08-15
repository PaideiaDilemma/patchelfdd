use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to cast integer: {}", source))]
    IntConversion { source: std::num::TryFromIntError },
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub enum ArchLong {
    Elf32([u8; 4]),
    Elf64([u8; 8]),
}

impl ArchLong {
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match *self {
            Self::Elf32(_) => 4,
            Self::Elf64(_) => 8,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Elf32(v) => v,
            Self::Elf64(v) => v,
        }
    }
}

pub trait SerializeIntoArchValue {
    fn to_le_arch(&self) -> ArchLong;
    fn to_be_arch(&self) -> ArchLong;
}

impl SerializeIntoArchValue for u32 {
    fn to_le_arch(&self) -> ArchLong {
        ArchLong::Elf32(self.to_le_bytes())
    }
    fn to_be_arch(&self) -> ArchLong {
        ArchLong::Elf32(self.to_be_bytes())
    }
}

impl SerializeIntoArchValue for u64 {
    fn to_le_arch(&self) -> ArchLong {
        ArchLong::Elf64(self.to_le_bytes())
    }
    fn to_be_arch(&self) -> ArchLong {
        ArchLong::Elf64(self.to_be_bytes())
    }
}

impl SerializeIntoArchValue for i32 {
    fn to_le_arch(&self) -> ArchLong {
        ArchLong::Elf32(self.to_le_bytes())
    }
    fn to_be_arch(&self) -> ArchLong {
        ArchLong::Elf32(self.to_be_bytes())
    }
}

impl SerializeIntoArchValue for i64 {
    fn to_le_arch(&self) -> ArchLong {
        ArchLong::Elf64(self.to_le_bytes())
    }
    fn to_be_arch(&self) -> ArchLong {
        ArchLong::Elf64(self.to_be_bytes())
    }
}

pub struct ArchSerializer {
    class: elf::file::Class,
    endianness: elf::endian::AnyEndian,
}

impl ArchSerializer {
    pub fn new(class: elf::file::Class, endianness: elf::endian::AnyEndian) -> Self {
        ArchSerializer { class, endianness }
    }

    pub fn bytes_from_signed_long(&self, val: i64) -> Result<ArchLong> {
        match self.class {
            elf::file::Class::ELF32 => {
                let val = i32::try_from(val).context(IntConversionSnafu)?;

                match self.endianness {
                    elf::endian::AnyEndian::Little => Ok(val.to_le_arch()),
                    elf::endian::AnyEndian::Big => Ok(val.to_be_arch()),
                }
            }
            elf::file::Class::ELF64 => match self.endianness {
                elf::endian::AnyEndian::Little => Ok(val.to_le_arch()),
                elf::endian::AnyEndian::Big => Ok(val.to_be_arch()),
            },
        }
    }

    pub fn bytes_from_unsigned_long(&self, val: u64) -> Result<ArchLong> {
        match self.class {
            elf::file::Class::ELF32 => {
                let val = u32::try_from(val).context(IntConversionSnafu)?;

                match self.endianness {
                    elf::endian::AnyEndian::Little => Ok(val.to_le_arch()),
                    elf::endian::AnyEndian::Big => Ok(val.to_be_arch()),
                }
            }
            elf::file::Class::ELF64 => match self.endianness {
                elf::endian::AnyEndian::Little => Ok(val.to_le_arch()),
                elf::endian::AnyEndian::Big => Ok(val.to_be_arch()),
            },
        }
    }
}

#[test]
fn test_le32() -> Result<()> {
    let serializer = ArchSerializer::new(elf::file::Class::ELF32, elf::endian::AnyEndian::Little);

    assert_eq!(
        serializer.bytes_from_signed_long(-1234)?.bytes(),
        [46, 251, 255, 255]
    );

    assert_eq!(
        serializer.bytes_from_unsigned_long(1234)?.bytes(),
        [210, 4, 0, 0]
    );

    Ok(())
}

#[test]
fn test_be32() -> Result<()> {
    let serializer = ArchSerializer::new(elf::file::Class::ELF32, elf::endian::AnyEndian::Big);

    assert_eq!(
        serializer.bytes_from_signed_long(-1234)?.bytes(),
        [255, 255, 251, 46]
    );

    assert_eq!(
        serializer.bytes_from_unsigned_long(1234)?.bytes(),
        [0, 0, 4, 210]
    );

    Ok(())
}

#[test]
fn test_le64() -> Result<()> {
    let serializer = ArchSerializer::new(elf::file::Class::ELF64, elf::endian::AnyEndian::Little);

    assert_eq!(
        serializer.bytes_from_signed_long(-0x133708)?.bytes(),
        [248, 200, 236, 255, 255, 255, 255, 255]
    );

    assert_eq!(
        serializer.bytes_from_unsigned_long(0x133708)?.bytes(),
        [8, 55, 19, 0, 0, 0, 0, 0]
    );

    Ok(())
}

#[test]
fn test_be64() -> Result<()> {
    let serializer = ArchSerializer::new(elf::file::Class::ELF64, elf::endian::AnyEndian::Big);

    assert_eq!(
        serializer.bytes_from_signed_long(-0x133708)?.bytes(),
        [255, 255, 255, 255, 255, 236, 200, 248]
    );

    assert_eq!(
        serializer.bytes_from_unsigned_long(0x133708)?.bytes(),
        [0, 0, 0, 0, 0, 19, 55, 8]
    );

    Ok(())
}
