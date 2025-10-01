mod cli;
mod migration;
mod tables;
mod utils;

use cli::build as cli_builder;
use cli::{ALL_DIRS, DRY_RUN, PATH};

fn main() {
    let command = cli_builder();
    let matches = command.get_matches();

    let search_path = matches.get_one::<String>(PATH).expect("Path is required");
    let dry_run = matches.get_flag(DRY_RUN);
    let all_dirs = matches.get_flag(ALL_DIRS);

    match migration::project::fix(search_path, dry_run, all_dirs) {
        Ok(()) => {
            if dry_run {
                println!("Dry run completed successfully.");
            } else {
                println!("Rebase completed successfully.");
            }
        }
        Err(e) => {
            eprintln!("{e}");
        }
    }
}
