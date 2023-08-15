use patchelfdd::opts::Opts;
use patchelfdd::Error;

use colored::Colorize;
use structopt::StructOpt;

fn run() -> Result<(), Error> {
    let opts = Opts::from_args();
    patchelfdd::run(opts)?;
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{}", format!("Error - {}", err).red());
        std::process::exit(1);
    }
}
