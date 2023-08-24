use crate::opts::Opts;
use crate::patch::{self, Patcher};
use crate::sparse_elf;

use colored::Colorize;
use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to write binary: {}", source))]
    WriteElf { source: std::io::Error },

    #[snafu(display("Failed to patch elf: {}", source))]
    PatchElf { source: patch::Error },

    #[snafu(display("{}", source))]
    SparseElf { source: sparse_elf::Error },

    #[snafu(display("Failed to get .dynamic section data"))]
    NoDynamicSection,

    #[snafu(display("DT_RUNPATH is already set, overwriting it is not supported yet"))]
    RunpathAlreadySet,
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn run(opts: Opts) -> Result<()> {
    let mut patcher = Patcher::new(&opts.bin).context(PatchElfSnafu)?;

    if let Some(runpath) = opts.set_runpath {
        if patcher
            .elf
            .dynamic_contains(elf::abi::DT_RUNPATH)
            .context(SparseElfSnafu)?
        {
            return Err(Error::RunpathAlreadySet);
        }

        patcher.set_runpath(&runpath).context(PatchElfSnafu)?;
    }

    if let Some(interpreter_path) = opts.set_interpreter {
        patcher
            .set_interpreter_path(&interpreter_path)
            .context(PatchElfSnafu)?;
    }

    if patcher.is_empty() {
        println!("{}", "Nothing to do".yellow());
        return Ok(());
    }

    patcher.apply().context(PatchElfSnafu)?;

    Ok(())
}
