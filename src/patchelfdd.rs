use crate::opts::Opts;
use crate::patch::{self, Patcher};

use colored::Colorize;
use elf::ParseError;
use snafu::prelude::*;
use std::{fs, io};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to open binary: {}", source))]
    ReadElf { source: io::Error },

    #[snafu(display("Failed to write binary: {}", source))]
    WriteElf { source: io::Error },

    #[snafu(display("Failed to parse elf: {}", source))]
    ParseElf { source: ParseError },

    #[snafu(display("Failed to patch elf: {}", source))]
    PatchElf { source: patch::Error },

    #[snafu(display("Failed to cast integer: {}", source))]
    IntConversion { source: std::num::TryFromIntError },

    #[snafu(display("Failed to get .dynamic section data"))]
    NoDynamicSection,

    #[snafu(display("DT_RUNPATH is already set, overwriting it is not supported yet"))]
    RunpathAlreadySet,
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn has_dt_runpath(elf_object: &elf::ElfBytes<'_, elf::endian::AnyEndian>) -> Result<bool> {
    let section_dynamic = elf_object
        .dynamic()
        .context(ParseElfSnafu)?
        .ok_or(Error::NoDynamicSection)?;

    for i in 0..section_dynamic.len() {
        let dyn_entry = section_dynamic.get(i).context(ParseElfSnafu)?;
        if dyn_entry.d_tag == elf::abi::DT_RUNPATH {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn run(opts: Opts) -> Result<()> {
    let mut elf_raw = fs::read(&opts.bin).context(ReadElfSnafu)?;
    let elf_object =
        elf::ElfBytes::<elf::endian::AnyEndian>::minimal_parse(&elf_raw).context(ParseElfSnafu)?;

    let mut patcher = Patcher::new(&elf_object);

    if let Some(runpath) = opts.set_runpath {
        if has_dt_runpath(&elf_object)? {
            return Err(Error::RunpathAlreadySet);
        }

        patcher
            .set_runpath(&elf_object, &runpath)
            .context(PatchElfSnafu)?;
    }

    if let Some(interpreter_path) = opts.set_interpreter {
        patcher
            .set_interpreter_path(&elf_object, &interpreter_path)
            .context(PatchElfSnafu)?;
    }

    if patcher.is_empty() {
        println!("{}", "Nothing to do".yellow());
        return Ok(());
    }

    patcher.apply_patches_to(elf_raw.as_mut());
    fs::write(opts.bin, &elf_raw).context(ReadElfSnafu)?;

    Ok(())
}
