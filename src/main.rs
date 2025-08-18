mod cli;
mod rebase;
mod utils;

use cli::build as cli_builder;
use cli::{DRY_RUN, PATH};

fn main() {
    let command = cli_builder();
    let matches = command.get_matches();

    let search_path = matches.get_one::<String>(PATH).expect("Path is required");
    let dry_run = matches.get_flag(DRY_RUN);

    match rebase::fix(search_path, dry_run) {
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
