use clap::{Arg, Command};

pub const DRY_RUN: &str = "dry-run";
pub const PATH: &str = "path";
pub const PATH_DEFAULT: &str = ".";
pub const PATH_SHORT: char = 'p';
pub const ALL_DIRS: &str = "all-dirs";
pub const APP_PATH: &str = "app-path";
pub const JSON: &str = "json";

pub fn build() -> Command {
    Command::new("rebase-migrations")
        .version("0.5.0")
        .author("Reinhard Scheuerle")
        .about("A tool to help with migration rebasing for django.")
        .arg(
            Arg::new(PATH)
                .short(PATH_SHORT)
                .long(PATH)
                .help("Path to search for migrations directories (defaults to current directory)")
                .value_name("PATH")
                .default_value(PATH_DEFAULT),
        )
        .arg(
            Arg::new(DRY_RUN)
                .long(DRY_RUN)
                .help("Show what would be done without making changes")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new(ALL_DIRS)
                .long(ALL_DIRS)
                .help("Scan all directories without skipping common build/cache directories (slower but comprehensive)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new(APP_PATH)
            .long(APP_PATH)
            .help("Only scan and rebase one django app with the path provided")
            .value_name("APP_PATH")
            .value_parser(clap::value_parser!(std::path::PathBuf)),
        )
        .arg(
            Arg::new(JSON)
            .long(JSON)
            .help("Output changes in JSON format (only works with --dry-run)")
            .action(clap::ArgAction::SetTrue)
            .requires(DRY_RUN),
        )
}
