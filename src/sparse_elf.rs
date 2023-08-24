use elf::dynamic::DynamicTable;
use elf::endian::AnyEndian;
use elf::file::Class;
use elf::section::SectionHeader;
use elf::string_table::StringTable;
use elf::{ElfStream, ParseError};
use std::fs::OpenOptions;
use std::path::PathBuf;

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to open file {}: {}", file_path, source))]
    OpenElf {
        file_path: String,
        source: std::io::Error,
    },

    #[snafu(display("Failed to parse elf: {}", source))]
    ParseElf { source: ParseError },

    #[snafu(display("Elf is missing a .dynamic section"))]
    NoDynamicSection,

    #[snafu(display("Elf is missing .dynstr section"))]
    NoDynstrSection,

    #[snafu(display("Elf is missing .interp section"))]
    NoInterpSection,
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub struct SparseElf {
    elf_stream: ElfStream<AnyEndian, std::fs::File>,

    pub shdr_dynamic: SectionHeader,
    pub shdr_dynstr: SectionHeader,
    pub shdr_interp: SectionHeader,
}

impl SparseElf {
    pub fn new(file_path: &PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(file_path)
            .context(OpenElfSnafu {
                file_path: file_path.to_string_lossy(),
            })?;

        let mut elf_stream = ElfStream::open_stream(file).context(ParseElfSnafu)?;

        let shdr_dynamic = *elf_stream
            .section_header_by_name(".dynamic")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynamicSection)?;

        let shdr_dynstr = *elf_stream
            .section_header_by_name(".dynstr")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynstrSection)?;

        let shdr_interp = *elf_stream
            .section_header_by_name(".interp")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoInterpSection)?;

        Ok(Self {
            elf_stream,
            shdr_dynamic,
            shdr_dynstr,
            shdr_interp,
        })
    }

    pub fn dynamic(&mut self) -> Result<DynamicTable<AnyEndian>> {
        self.elf_stream
            .dynamic()
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynamicSection)
    }

    pub fn dynstr(&mut self) -> Result<StringTable> {
        self.elf_stream
            .section_data_as_strtab(&self.shdr_dynstr)
            .context(ParseElfSnafu)
    }

    pub fn class(&self) -> Class {
        self.elf_stream.ehdr.class
    }

    pub fn endianess(&self) -> AnyEndian {
        self.elf_stream.ehdr.endianness
    }

    pub fn dynstr_contains(&mut self, needle: &str) -> Result<bool> {
        let mut dynstr_index = 1;
        while (dynstr_index as u64) < self.shdr_dynstr.sh_size {
            let entry = self.dynstr()?.get(dynstr_index).context(ParseElfSnafu)?;

            if entry.contains(needle) {
                return Ok(true);
            }
            dynstr_index += entry.len() + 1;
        }

        Ok(false)
    }

    pub fn dynamic_contains(&mut self, d_tag: i64) -> Result<bool> {
        let section_dynamic = self.dynamic()?;

        for i in 0..section_dynamic.len() {
            let dyn_entry = section_dynamic.get(i).context(ParseElfSnafu)?;
            if dyn_entry.d_tag == d_tag {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
