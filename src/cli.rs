use clap::{Arg, Command};

pub const DRY_RUN: &str = "dry-run";
pub const PATH: &str = "path";
pub const PATH_SHORT: char = 'p';

pub fn build() -> Command {
    Command::new("rebase-migrations")
        .version("0.1.0")
        .author("Reinhard Scheuerle")
        .about("A tool to help with migration rebasing for django.")
        .arg(
            Arg::new(PATH)
                .short(PATH_SHORT)
                .long(PATH)
                .help("Path to search for migrations directories (defaults to current directory)")
                .value_name("PATH")
                .default_value("."),
        )
        .arg(
            Arg::new(DRY_RUN)
                .long(DRY_RUN)
                .help("Show what would be done without making changes")
                .action(clap::ArgAction::SetTrue),
        )
}
