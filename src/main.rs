use clap::{Arg, Command};
use std::env;
use std::fs;
use std::io::{self, Read};
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

struct ConflictInfo {
    head_migration: String,
    branch_migration: String,
}

fn detect_merge_conflict(content: &str) -> Option<ConflictInfo> {
    // Check if the content contains conflict markers
    if !content.contains("<<<<<<<") || !content.contains("=======") || !content.contains(">>>>>>>")
    {
        return None;
    }

    // Extract the conflicting parts
    let head_start = content.find("<<<<<<<").unwrap();
    let separator = content.find("=======").unwrap();
    let branch_end = content.find(">>>>>>>").unwrap();

    // Extract the migration names
    let head_part = &content[head_start + 7..separator].trim();
    let branch_part = &content[separator + 7..branch_end].trim();

    // Return the conflict info
    Some(ConflictInfo {
        head_migration: head_part.to_string(),
        branch_migration: branch_part.to_string(),
    })
}

fn find_migration_file(dir: &Path, migration_prefix: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("py") {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with(migration_prefix) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

fn extract_migration_number(migration_name: &str) -> Option<String> {
    // Migration names usually start with a number followed by underscore
    // e.g. "0040_delete_cms_pages_add_about_us_page"
    if let Some(underscore_pos) = migration_name.find('_') {
        return Some(migration_name[..underscore_pos].to_string());
    }
    None
}

fn fix_command(search_path: &str, dry_run: bool) {
    let migration_dirs = find_migration_dirs(search_path);

    if migration_dirs.is_empty() {
        println!("No migration directories found in {}", search_path);
        return;
    }

    println!("Found {} migration directories", migration_dirs.len());
    println!("Searching for merge conflicts in max_migration.txt files...");

    let mut conflicts_found = false;

    for migration_dir in &migration_dirs {
        if check_for_conflicts(migration_dir) {
            conflicts_found = true;
        }
    }

    if !conflicts_found {
        println!("\nNo merge conflicts found in any max_migration.txt files.");
    }
}

fn check_for_conflicts(dir: &Path) -> bool {
    let max_migration_path = dir.join("max_migration.txt");

    if !max_migration_path.exists() {
        return false;
    }

    // Read the file content
    let mut content = String::new();
    match fs::File::open(&max_migration_path).and_then(|mut file| file.read_to_string(&mut content))
    {
        Ok(_) => {}
        Err(_) => return false,
    }

    // Check for merge conflicts
    if let Some(conflict) = detect_merge_conflict(&content) {
        // Print conflict information
        println!("\n=== MERGE CONFLICT FOUND ===");
        println!("Directory: {}", dir.display());
        println!("File: {}", max_migration_path.display());
        println!("Content:");
        println!("```");
        println!("{}", content);
        println!("```");

        // Extract migration numbers
        let head_num = extract_migration_number(&conflict.head_migration);
        let branch_num = extract_migration_number(&conflict.branch_migration);

        // Focus on the branch migration (the incoming change)
        println!("Current (HEAD) migration: {}", conflict.head_migration);
        println!("Incoming branch migration: {}", conflict.branch_migration);

        // Highlight the problematic file (from the branch being rebased)
        if let Some(branch_num) = branch_num {
            if let Some(branch_file) = find_migration_file(dir, &branch_num) {
                println!("\n!!! PROBLEMATIC MIGRATION FILE !!!");
                println!("File: {}", branch_file.display());
                println!("This is the migration from the branch you're rebasing.");
            } else {
                println!(
                    "\nBranch migration file not found for prefix {}_",
                    branch_num
                );
                println!("This is unusual and may indicate a more complex conflict.");
            }
        }

        // Show head file for reference but with less emphasis
        if let Some(head_num) = head_num {
            if let Some(head_file) = find_migration_file(dir, &head_num) {
                println!(
                    "\nCurrent repository migration file: {}",
                    head_file.display()
                );
            } else {
                println!(
                    "\nCurrent repository migration file not found for prefix {}_",
                    head_num
                );
            }
        }

        // List all migrations in the directory for reference
        println!("\nAll migration files in directory:");
        if let Ok(entries) = fs::read_dir(dir) {
            let mut migration_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|entry| {
                    let path = entry.path();
                    path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("py")
                })
                .map(|entry| entry.path())
                .collect();

            migration_files.sort_by(|a, b| {
                a.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .cmp(b.file_name().unwrap().to_str().unwrap())
            });

            for file in migration_files {
                println!("  - {}", file.file_name().unwrap().to_string_lossy());
            }
        }

        println!("\n------------------------------------");
        return true;
    }

    false
}
