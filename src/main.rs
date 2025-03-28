mod cli;
mod rebase;

use cli::build as cli_builder;
use cli::{DRY_RUN, FIX, PATH};

fn main() {
    let command = cli_builder();
    let matches = command.get_matches();

    if let Some(matches) = matches.subcommand_matches(FIX) {
        let search_path = matches.get_one::<String>(PATH).expect("Path is required");
        let dry_run = matches.get_flag(DRY_RUN);

        rebase::fix(search_path, dry_run).unwrap();
    } else {
        println!("No subcommand provided. Try 'rebase-migrations fix --help'");
    }
}
