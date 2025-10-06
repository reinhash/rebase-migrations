mod cli;
mod migration;
mod tables;
mod utils;

use cli::build as cli_builder;
use cli::{ALL_DIRS, DRY_RUN, JSON, PATH};

use crate::cli::{APP_PATH, PATH_DEFAULT};

fn main() {
    let command = cli_builder();
    let matches = command.get_matches();

    let search_path = matches.get_one::<String>(PATH).expect("Path is required");
    let app_path = matches.get_one::<std::path::PathBuf>(APP_PATH);
    let dry_run = matches.get_flag(DRY_RUN);
    let all_dirs = matches.get_flag(ALL_DIRS);
    let json = matches.get_flag(JSON);

    if dry_run && !json {
        println!("Dry run detected. No changes will be made.");
    }

    let result = match (app_path, search_path.as_str()) {
        (Some(app_path), PATH_DEFAULT) => migration::project::rebase_app(app_path, dry_run, json),
        (None, search_path) => {
            migration::project::rebase_apps(search_path, dry_run, all_dirs, json)
        }
        (Some(_), _) => {
            eprintln!(
                "Please only provide an app-path or a path for the whole Django project, but not both."
            );
            return;
        }
    };

    match result {
        Ok(json_output) => {
            if let Some(json_str) = json_output {
                println!("{}", json_str);
            } else if dry_run {
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
