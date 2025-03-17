use clap::{Arg, Command};
use std::env;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() {
    let matches = Command::new("rebase-migrations")
        .version("0.1.0")
        .author("Reinhard Scheuerle")
        .about("A tool to help with migration rebasing for django.")
        .subcommand(
            Command::new("fix")
                .about("Fixes migration issues")
                .arg(
                    Arg::new("path")
                        .short('p')
                        .long("path")
                        .help("Path to search for migrations directories (defaults to current directory)")
                        .value_name("PATH")
                        .default_value("."),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Show what would be done without making changes")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("fix") {
        let search_path = matches.get_one::<String>("path").expect("Path is required");
        let dry_run = matches.get_flag("dry-run");

        fix_command(search_path, dry_run);
    } else {
        println!("No subcommand provided. Try 'rebase-migrations fix --help'");
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        let home = env::var("HOME").expect("Failed to get HOME directory");
        return PathBuf::from(path.replacen("~", &home, 1));
    }
    PathBuf::from(path)
}

fn find_migration_dirs(search_path: &str) -> Vec<PathBuf> {
    // Convert path with possible tilde to absolute path
    let expanded_path = expand_tilde(search_path);
    let mut migration_dirs = Vec::new();

    println!("Searching for migrations in: {}", expanded_path.display());

    for entry in WalkDir::new(expanded_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_dir() && path.file_name().and_then(|n| n.to_str()) == Some("migrations") {
            migration_dirs.push(path.to_path_buf());
        }
    }

    migration_dirs
}

fn fix_command(search_path: &str, dry_run: bool) {
    let migration_dirs = find_migration_dirs(search_path);

    if migration_dirs.is_empty() {
        println!("No migration directories found in {}", search_path);
        return;
    }

    println!("Found {} migration directories:", migration_dirs.len());

    for migration_dir in &migration_dirs {
        if dry_run {
            println!(
                "Dry run: Would fix migrations in {}",
                migration_dir.display()
            );
        } else {
            println!("Fixing migrations in {}", migration_dir.display());
            // Process each migration directory
            process_migration_dir(migration_dir);
        }
    }
}

fn process_migration_dir(dir: &Path) {
    // Here you would implement the actual migration fixing logic
    // For now we just print information about the directory
    println!("  Processing directory: {}", dir.display());

    // Example: List all Python files in the migration directory
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("py") {
                println!(
                    "    Found migration file: {}",
                    path.file_name().unwrap().to_string_lossy()
                );
            }
        }
    }
}
