use crate::{
    serialize::{self, ArchSerializer},
    sparse_elf::{self, SparseElf},
};

use colored::Colorize;
use std::{fs::OpenOptions, io::Seek, io::SeekFrom, io::Write, mem::size_of, path::PathBuf};

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to open file {} for writing: {}", file_path, source))]
    OpenElfWritable {
        file_path: String,
        source: std::io::Error,
    },

    #[snafu(display("Failed to seek to offset {}: {}", offset, source))]
    SeekElf {
        offset: usize,
        source: std::io::Error,
    },

    #[snafu(display("Failed to parse elf: {}", source))]
    ParseElf { source: elf::ParseError },

    #[snafu(display("Failed to write elf: {}", source))]
    WriteElf { source: std::io::Error },

    #[snafu(display("{}", source))]
    SparseElf { source: sparse_elf::Error },

    #[snafu(display("Failed to cast integer: {}", source))]
    IntConversion { source: std::num::TryFromIntError },

    #[snafu(display("Failed to serialize: {}", source))]
    Serializing { source: serialize::Error },

    #[snafu(display("Integer overflow"))]
    IntegerOverflow,

    #[snafu(display("Did not find an appropriate entry in .dynstr to replace with DT_RUNPATH"))]
    NoDynstrReplacementCandidate,

    #[snafu(display(
        "Did not find a place to add a .dynamic entry without extending. Was looking for:\n\
        - At least two consecutive DT_NULL entries\n\
        - An entry containing the .dynstr offset of the symbol that we want to overwrite"
    ))]
    NoApplicableDynamicEntry,

    #[snafu(display(".dynamic is not delimited by a DT_NULL entry"))]
    DynamicSectionNotDelimited,

    #[snafu(display(
        "Elf .interp section is not large enough to hold the new interpreter path\n\
        .interp size: {}\n\
        requested size: {}",
        section_size,
        requested_size
    ))]
    CannotFitInterpreterPath {
        section_size: usize,
        requested_size: usize,
    },
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Copy, Clone)]
enum DynstrPatchCandidates {
    GmonStart,
    ITMDeregisterTMCloneTable,
}

impl DynstrPatchCandidates {
    fn as_string(&self) -> &'static str {
        match self {
            Self::GmonStart => "__gmon_start__",
            Self::ITMDeregisterTMCloneTable => "_ITM_deregisterTMCloneTable",
        }
    }

    fn get_valid_candiates(elf: &mut SparseElf) -> Result<Vec<Self>> {
        let mut res: Vec<Self> = Vec::new();

        if !(elf.dynstr_contains("mcount").context(SparseElfSnafu)?) {
            res.push(Self::GmonStart);
        }

        if !(elf.dynstr_contains("libitm.so").context(SparseElfSnafu)?) {
            res.push(Self::ITMDeregisterTMCloneTable);
        }

        Ok(res)
    }
}

#[derive(Default)]
struct Patch {
    offset: usize,
    data: Vec<u8>,
}

pub struct Patcher {
    pub elf: SparseElf,
    patches: Vec<Patch>,
    serializer: ArchSerializer,
    file_path: PathBuf,
}

impl Patcher {
    pub fn new(file_path: &PathBuf) -> Result<Self> {
        let elf = SparseElf::new(file_path).context(SparseElfSnafu)?;
        let serializer = ArchSerializer::new(elf.class(), elf.endianess());
        Ok(Self {
            elf,
            patches: Vec::new(),
            serializer,
            file_path: file_path.clone(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn apply(&mut self) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.file_path)
            .context(OpenElfWritableSnafu {
                file_path: self.file_path.to_string_lossy(),
            })?;

        self.patches.sort_by_key(|p| p.offset);

        for patch in self.patches.iter() {
            file.seek(SeekFrom::Start(patch.offset as u64))
                .context(SeekElfSnafu {
                    offset: patch.offset,
                })?;

            file.write_all(&patch.data).context(WriteElfSnafu)?;
        }

        Ok(())
    }

    fn add_patch(&mut self, offset: usize, size: usize) -> &mut Patch {
        self.patches.push(Patch {
            offset,
            data: vec![0; size],
        });

        self.patches.last_mut().unwrap()
    }

    pub fn set_interpreter_path(&mut self, new_interpreter_path: &str) -> Result<()> {
        let interp_sh_size =
            usize::try_from(self.elf.shdr_interp.sh_size).context(IntConversionSnafu)?;

        if interp_sh_size < new_interpreter_path.len() {
            return Err(Error::CannotFitInterpreterPath {
                section_size: interp_sh_size,
                requested_size: new_interpreter_path.len(),
            });
        }

        let interp_sh_offset =
            usize::try_from(self.elf.shdr_interp.sh_offset).context(IntConversionSnafu)?;

        let patch = self.add_patch(interp_sh_offset, new_interpreter_path.len() + 1);
        patch.data[..new_interpreter_path.len()].copy_from_slice(new_interpreter_path.as_bytes());

        Ok(())
    }

    pub fn set_runpath(&mut self, new_runpath: &str) -> Result<()> {
        let dynstr_entry_offset = self.set_runpath_dynstr(new_runpath)?;
        self.set_runpath_dynamic(dynstr_entry_offset as u64)?;

        Ok(())
    }

    fn set_runpath_dynstr(&mut self, new_runpath: &str) -> Result<usize> {
        let valid_candidates = DynstrPatchCandidates::get_valid_candiates(&mut self.elf)?;

        let mut dynstr_index = 1;
        let mut dynstr_candidate: Option<DynstrPatchCandidates> = None;

        let dynstr_sh_size = self.elf.shdr_dynstr.sh_size;

        let dynstr_data = self.elf.dynstr().context(SparseElfSnafu)?;

        while (dynstr_index as u64) < dynstr_sh_size {
            let entry = dynstr_data.get(dynstr_index).context(ParseElfSnafu)?;

            if entry.len() >= new_runpath.len() {
                if let Some(candidate) = valid_candidates.iter().find(|c| c.as_string() == entry) {
                    dynstr_candidate = Some(*candidate);
                    break;
                }
            }

            dynstr_index += entry.len() + 1;
        }

        let dynstr_candidate = match dynstr_candidate {
            Some(candidate) => candidate,
            None => return Err(Error::NoDynstrReplacementCandidate),
        };

        println!(
            "{}",
            format!(
                "Warning: Overwriting dynstr entry: {}",
                dynstr_candidate.as_string()
            )
            .yellow()
            .bold()
        );

        let dynstr_target_offset = usize::try_from(self.elf.shdr_dynstr.sh_offset)
            .context(IntConversionSnafu)?
            + dynstr_index;

        let patch = self.add_patch(dynstr_target_offset, new_runpath.len() + 1);
        patch.data[..new_runpath.len()].copy_from_slice(new_runpath.as_bytes());

        Ok(dynstr_index)
    }

    fn set_runpath_dynamic(&mut self, dynstr_entry_offset: u64) -> Result<()> {
        let dynamic_sh_offset =
            usize::try_from(self.elf.shdr_dynamic.sh_offset).context(IntConversionSnafu)?;

        let dynamic_data = self.elf.dynamic().context(SparseElfSnafu)?;

        let mut dyn_entry_position = dynamic_data
            .iter()
            .position(|d| d.d_tag == elf::abi::DT_NULL)
            .ok_or(Error::NoApplicableDynamicEntry)?;

        match dynamic_data.get(dyn_entry_position + 1) {
            Ok(_) => {}
            Err(e) => match e {
                // If there are not two DT_NULL entries following each other,
                // we try to find the Dyn entry, that referenced the .dynstr entry, that we
                // corrupted and overwrite that.
                elf::ParseError::BadOffset(_) => {
                    dyn_entry_position = dynamic_data
                        .iter()
                        .position(|d| d.d_val() == dynstr_entry_offset)
                        .ok_or(Error::NoApplicableDynamicEntry)?;
                }
                _ => return Err(Error::ParseElf { source: e }),
            },
        }

        let dyn_table_offset = dyn_entry_position
            .checked_mul(match self.elf.class() {
                elf::file::Class::ELF32 => size_of::<elf::dynamic::Elf32_Dyn>(),
                elf::file::Class::ELF64 => size_of::<elf::dynamic::Elf64_Dyn>(),
            })
            .ok_or(Error::IntegerOverflow)?;

        let dyn_entry_offset = dynamic_sh_offset
            .checked_add(dyn_table_offset)
            .ok_or(Error::IntegerOverflow)?;

        let dyn_d_tag_data = self
            .serializer
            .bytes_from_signed_long(elf::abi::DT_RUNPATH)
            .context(SerializingSnafu)?;

        let dyn_d_un_data = self
            .serializer
            .bytes_from_unsigned_long(dynstr_entry_offset)
            .context(SerializingSnafu)?;

        let patch = self.add_patch(dyn_entry_offset, dyn_d_tag_data.len() + dyn_d_un_data.len());

        patch.data[..dyn_d_tag_data.len()].copy_from_slice(dyn_d_tag_data.bytes());
        patch.data[dyn_d_tag_data.len()..].copy_from_slice(dyn_d_un_data.bytes());

        Ok(())
    }
}
