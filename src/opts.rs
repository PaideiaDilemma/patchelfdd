use std::path::PathBuf;

use colored::Color;
use colored::Colorize;
use structopt::StructOpt;

#[derive(StructOpt, Clone)]
pub struct Opts {
    /// Binary to patch
    #[structopt(long)]
    pub bin: PathBuf,

    /// New runtime path
    #[structopt(short = "r", long)]
    pub set_runpath: Option<String>,

    /// New interpreter path
    #[structopt(short = "i", long)]
    pub set_interpreter: Option<String>,
}

impl Opts {
    pub fn print(&self) {
        println!(
            "{}: {}",
            "bin".color(Color::Cyan),
            self.bin.to_string_lossy().bold()
        );
        println!("{}: {:?}", "rpath".color(Color::Yellow), self.set_runpath);
    }
}
