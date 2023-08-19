use crate::serialize::{self, ArchSerializer};

use colored::Colorize;
use std::mem::size_of;

use elf::{endian::AnyEndian, ElfBytes, ParseError};

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to parse elf: {}", source))]
    ParseElf { source: ParseError },

    #[snafu(display("Failed to cast integer: {}", source))]
    IntConversion { source: std::num::TryFromIntError },

    #[snafu(display("Failed to serialize: {}", source))]
    Serializing { source: serialize::Error },

    #[snafu(display("Integer overflow"))]
    IntegerOverflow,

    #[snafu(display("Elf is missing a .dynamic section"))]
    NoDynamicSection,

    #[snafu(display("Elf is missing .dynstr section"))]
    NoDynstrSection,

    #[snafu(display("Elf is missing .interp section"))]
    NoInterpSection,

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

#[derive(Debug)]
enum DynstrPatchCandidate {
    GmonStart,
    ITMDeregisterTMCloneTable,
}

impl DynstrPatchCandidate {
    fn from_string(data: &str) -> Option<Self> {
        match data {
            "__gmon_start__" => Some(DynstrPatchCandidate::GmonStart),
            "_ITM_deregisterTMCloneTable" => Some(DynstrPatchCandidate::ITMDeregisterTMCloneTable),
            _ => None,
        }
    }
    // low has higher priority
    fn rank(&self) -> Option<u16> {
        match self {
            Self::GmonStart => Some(10),
            Self::ITMDeregisterTMCloneTable => Some(1),
        }
    }

    fn check_valid(&self, elf_object: &ElfBytes<'_, AnyEndian>) -> Result<bool> {
        // TODO: add checks to make sure the symbols are not actively used
        match self {
            Self::GmonStart => Self::check_gprof(elf_object),
            Self::ITMDeregisterTMCloneTable => Ok(true),
        }
    }

    fn check_gprof(elf_object: &ElfBytes<'_, AnyEndian>) -> Result<bool> {
        let shdr_dynstr = elf_object
            .section_header_by_name(".dynstr")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynstrSection)?;

        let dynstr_data = elf_object
            .section_data_as_strtab(&shdr_dynstr)
            .context(ParseElfSnafu)?;

        let mut dynstr_index = 1;
        while (dynstr_index as u64) < shdr_dynstr.sh_size {
            let entry = dynstr_data.get(dynstr_index).context(ParseElfSnafu)?;

            if entry.contains("mcount") {
                return Ok(false);
            }
            dynstr_index += entry.len() + 1;
        }

        Ok(true)
    
}

#[derive(Default)]
struct Patch {
    offset: usize,
    data: Vec<u8>,
}

pub struct Patcher {
    patches: Vec<Patch>,
    serializer: ArchSerializer,
}

impl Patcher {
    pub fn new(elf_object: &ElfBytes<'_, AnyEndian>) -> Self {
        Self {
            patches: Vec::new(),
            serializer: ArchSerializer::new(elf_object.ehdr.class, elf_object.ehdr.endianness),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn apply_patches_to(&self, binary_data: &mut Vec<u8>) {
        // TODO: Add overlap check
        for patch in self.patches.iter() {
            let patch_end = patch.offset + patch.data.len();
            if patch_end > binary_data.len() {
                binary_data.resize(patch_end, 0);
            }
            binary_data[patch.offset..patch_end].copy_from_slice(&patch.data)
        }
    }

    fn add_patch(&mut self, offset: usize, size: usize) -> &mut Patch {
        self.patches.push(Patch {
            offset,
            data: vec![0; size],
        });

        self.patches.last_mut().unwrap()
    }

    pub fn set_interpreter_path(
        &mut self,
        elf_object: &ElfBytes<'_, AnyEndian>,
        new_interpreter_path: &str,
    ) -> Result<()> {
        let shdr_interp = elf_object
            .section_header_by_name(".interp")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoInterpSection)?;

        let interp_sh_offset =
            usize::try_from(shdr_interp.sh_offset).context(IntConversionSnafu)?;

        let interp_sh_size = usize::try_from(shdr_interp.sh_size).context(IntConversionSnafu)?;

        if interp_sh_size < new_interpreter_path.len() {
            return Err(Error::CannotFitInterpreterPath {
                section_size: interp_sh_size,
                requested_size: new_interpreter_path.len(),
            });
        }

        let patch = self.add_patch(interp_sh_offset, new_interpreter_path.len() + 1);
        patch.data[..new_interpreter_path.len()].copy_from_slice(new_interpreter_path.as_bytes());

        Ok(())
    }

    pub fn set_runpath(
        &mut self,
        elf_object: &ElfBytes<'_, AnyEndian>,
        new_runpath: &str,
    ) -> Result<()> {
        let dynstr_entry_offset = self.set_runpath_dynstr(elf_object, new_runpath)?;
        self.set_runpath_dynamic(elf_object, dynstr_entry_offset as u64)?;

        Ok(())
    }

    fn set_runpath_dynstr(
        &mut self,
        elf_object: &ElfBytes<'_, AnyEndian>,
        new_runpath: &str,
    ) -> Result<usize> {
        let shdr_dynstr = elf_object
            .section_header_by_name(".dynstr")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynstrSection)?;

        let dynstr_data = elf_object
            .section_data_as_strtab(&shdr_dynstr)
            .context(ParseElfSnafu)?;

        let mut dynstr_index = 1;
        let mut dynstr_candidates: Vec<(DynstrPatchCandidate, usize)> = Vec::new();

        while (dynstr_index as u64) < shdr_dynstr.sh_size {
            let entry = dynstr_data.get(dynstr_index).context(ParseElfSnafu)?;

            if let Some(candidate) = DynstrPatchCandidate::from_string(entry) {
                if entry.len() >= new_runpath.len() && candidate.check_valid(elf_object)? {
                    dynstr_candidates.push((candidate, dynstr_index));
                }
            };

            dynstr_index += entry.len() + 1;
        }

        let best_dynstr_overwrite = dynstr_candidates
            .iter()
            .max_by_key(|a| a.0.rank())
            .ok_or(Error::NoDynstrReplacementCandidate)?;

        println!(
            "{}",
            format!(
                "Warning: Overwriting dynstr entry: {}",
                dynstr_data
                    .get(best_dynstr_overwrite.1)
                    .context(ParseElfSnafu)?
            )
            .yellow()
            .bold()
        );

        let dynstr_target_offset = usize::try_from(shdr_dynstr.sh_offset)
            .context(IntConversionSnafu)?
            + best_dynstr_overwrite.1;

        let patch = self.add_patch(dynstr_target_offset, new_runpath.len() + 1);
        patch.data[..new_runpath.len()].copy_from_slice(new_runpath.as_bytes());

        Ok(best_dynstr_overwrite.1)
    }

    fn set_runpath_dynamic(
        &mut self,
        elf_object: &'_ ElfBytes<'_, AnyEndian>,
        dynstr_entry_offset: u64,
    ) -> Result<()> {
        let shdr_dynamic = elf_object
            .section_header_by_name(".dynamic")
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynamicSection)?;

        let dynamic_data = elf_object
            .dynamic()
            .context(ParseElfSnafu)?
            .ok_or(Error::NoDynamicSection)?;
        let dynamic_sh_offset =
            usize::try_from(shdr_dynamic.sh_offset).context(IntConversionSnafu)?;

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
                ParseError::BadOffset(_) => {
                    dyn_entry_position = dynamic_data
                        .iter()
                        .position(|d| d.d_val() == dynstr_entry_offset)
                        .ok_or(Error::NoApplicableDynamicEntry)?;
                }
                _ => return Err(Error::ParseElf { source: e }),
            },
        }

        let dyn_table_offset = dyn_entry_position
            .checked_mul(match elf_object.ehdr.class {
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
