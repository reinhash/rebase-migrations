use clap::{Arg, Command};
use git2::{Repository, Status};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() {
    let command = Command::new("rebase-migrations")
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
        );

    let matches = command.get_matches();

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
        .follow_links(false)
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
    if !content.contains("<<<<<<<") || !content.contains("=======") || !content.contains(">>>>>>>")
    {
        return None;
    }

    let head_start = content.find("<<<<<<<").unwrap();
    let separator = content.find("=======").unwrap();
    let branch_end = content.find(">>>>>>>").unwrap();

    // Extract the migration names, being careful to remove the marker line
    let head_section = &content[head_start + 7..separator].trim();
    let branch_section = &content[separator + 7..branch_end].trim();

    // Handle the HEAD marker in the head section
    let head_part = if head_section.starts_with("HEAD") {
        // Find the actual migration after "HEAD" (usually on next line)
        head_section.trim_start_matches("HEAD").trim()
    } else {
        head_section
    };

    // For branch part, find the actual migration before any commit message
    let branch_part = if let Some(idx) = branch_section.find("(") {
        branch_section[..idx].trim()
    } else {
        branch_section
    };

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
    // Handle empty or invalid migration names
    if migration_name.is_empty() {
        return None;
    }

    // Migration names usually start with a number followed by underscore
    // e.g. "0040_delete_cms_pages_add_about_us_page"
    if let Some(underscore_pos) = migration_name.find('_') {
        return Some(migration_name[..underscore_pos].to_string());
    }
    None
}

fn find_staged_migrations(repo_path: &Path) -> Vec<PathBuf> {
    let mut staged_migrations = Vec::new();

    // Open the git repository
    let repo = match Repository::open(repo_path) {
        Ok(repo) => repo,
        Err(_) => return staged_migrations,
    };

    // Get the repository status with options to include staged files
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_ignored(false)
        .include_untracked(false)
        .include_unmodified(false);

    // Get the repository status
    let statuses = match repo.statuses(Some(&mut status_opts)) {
        Ok(statuses) => statuses,
        Err(_) => return staged_migrations,
    };

    // Filter for staged files that are migration files
    for status_entry in statuses.iter() {
        let status = status_entry.status();
        let path = match status_entry.path() {
            Some(p) => p,
            None => continue,
        };

        // Check if it's a staged file (added, modified, or renamed)
        let is_staged =
            status.is_index_new() || status.is_index_modified() || status.is_index_renamed();

        if is_staged
            && path.contains("migrations")
            && path.ends_with(".py")
            && path != "__init__.py"
        {
            // Check if it's a migration file (has a numeric prefix)
            let file_name = Path::new(path).file_name().unwrap_or_default();
            let file_name_str = file_name.to_string_lossy();

            if let Some(pos) = file_name_str.find('_') {
                if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_digit(10)) {
                    staged_migrations.push(repo_path.join(path));
                }
            }
        }
    }

    staged_migrations
}

fn find_conflicted_migrations(repo_path: &Path) -> Vec<PathBuf> {
    let mut conflicted_migrations = Vec::new();

    // Open the git repository
    let repo = match Repository::open(repo_path) {
        Ok(repo) => repo,
        Err(_) => return conflicted_migrations,
    };

    // Get the repository status
    let statuses = match repo.statuses(None) {
        Ok(statuses) => statuses,
        Err(_) => return conflicted_migrations,
    };

    // Filter for conflicted files that are migration files
    for status_entry in statuses.iter() {
        let status = status_entry.status();
        let path = status_entry.path().unwrap_or("");

        // Check if it's a conflicted file
        if status.is_conflicted() && path.contains("migrations") && path.ends_with(".py") {
            // Check if it's a migration file (has a numeric prefix)
            let file_name = Path::new(path).file_name().unwrap_or_default();
            let file_name_str = file_name.to_string_lossy();

            if let Some(pos) = file_name_str.find('_') {
                if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_digit(10)) {
                    conflicted_migrations.push(repo_path.join(path));
                }
            }
        }
    }

    conflicted_migrations
}

fn get_repo_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();

    loop {
        if current.join(".git").exists() {
            return Some(current);
        }

        if !current.pop() {
            return None;
        }
    }
}

fn fix_command(search_path: &str, dry_run: bool) {
    let expanded_path = expand_tilde(search_path);
    let repo_root = match get_repo_root(&expanded_path) {
        Some(root) => root,
        None => {
            println!(
                "Error: Could not find Git repository in or above '{}'",
                expanded_path.display()
            );
            return;
        }
    };

    println!("Git repository found at: {}", repo_root.display());

    // First find any migration directories with max_migration.txt conflicts
    let migration_dirs = find_migration_dirs(search_path);
    let mut found_conflicts = false;

    println!("Checking for conflicts in max_migration.txt files...");
    for migration_dir in &migration_dirs {
        if check_for_conflicts(migration_dir, &repo_root) {
            found_conflicts = true;
        }
    }

    if !found_conflicts {
        println!("\nNo merge conflicts found in any max_migration.txt files.");
    }

    // Check for directly conflicted migration files
    let conflicted_migrations = find_conflicted_migrations(&repo_root);
    if !conflicted_migrations.is_empty() {
        println!("\n=== DIRECTLY CONFLICTED MIGRATION FILES ===");
        println!(
            "Found {} migration files with Git conflicts:",
            conflicted_migrations.len()
        );

        for file in &conflicted_migrations {
            println!("  - {}", file.display());
        }

        println!("\nYou need to resolve these conflicts manually before renumbering.");
        return; // Don't attempt renumbering if conflicts exist
    }

    // Check for staged migration files and align their numbering
    let staged_migrations = find_staged_migrations(&repo_root);
    if !staged_migrations.is_empty() {
        println!("\n=== STAGED MIGRATION FILES ===");
        println!("Found {} staged migration files:", staged_migrations.len());

        // Group staged migrations by directory
        let mut migrations_by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        for path in &staged_migrations {
            if let Some(parent) = path.parent() {
                migrations_by_dir
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(path.clone());
            }
        }

        // Process each directory separately
        for (dir, staged_paths) in migrations_by_dir {
            println!("\nProcessing migrations in directory: {}", dir.display());

            // Get all existing migrations in this directory
            let mut all_migrations = get_all_migrations_in_dir(&dir);

            // Mark the staged ones
            for staged_path in &staged_paths {
                if let Some(idx) = all_migrations.iter().position(|m| m.path == *staged_path) {
                    all_migrations[idx].is_staged = true;
                } else if let Some((number, name)) = extract_migration_info(staged_path) {
                    all_migrations.push(MigrationFile {
                        path: staged_path.clone(),
                        number,
                        name,
                        is_staged: true,
                    });
                }
            }

            // Sort again to ensure correct order
            all_migrations.sort_by_key(|m| m.number);

            // Find the highest non-staged migration number
            let highest_existing = all_migrations
                .iter()
                .filter(|m| !m.is_staged)
                .map(|m| m.number)
                .max()
                .unwrap_or(0);

            println!("Highest existing migration number: {}", highest_existing);

            // Get only the staged migrations
            let staged_migrations: Vec<_> = all_migrations.iter().filter(|m| m.is_staged).collect();

            if staged_migrations.is_empty() {
                println!("No staged migrations found in this directory.");
                continue;
            }

            println!(
                "Found {} staged migrations to renumber:",
                staged_migrations.len()
            );
            for m in &staged_migrations {
                println!("  - {} (current number: {})", m.path.display(), m.number);
            }

            // Generate new numbers for staged migrations
            let mut new_number = highest_existing + 1;
            let mut rename_operations = Vec::new();

            for m in &staged_migrations {
                let old_path = &m.path;
                let new_file_name = format!("{:04}{}.py", new_number, m.name);
                let new_path = old_path.with_file_name(new_file_name);

                rename_operations.push((old_path.clone(), new_path.clone(), m.number, new_number));
                new_number += 1;
            }

            // Perform or simulate the renaming
            println!("\nRenumbering operations:");
            for (old_path, new_path, old_num, new_num) in rename_operations.iter() {
                if dry_run {
                    println!(
                        "  Would rename {} -> {} (number {} -> {})",
                        old_path.file_name().unwrap().to_str().unwrap(),
                        new_path.file_name().unwrap().to_str().unwrap(),
                        old_num,
                        new_num
                    );
                } else {
                    println!(
                        "  Renaming {} -> {} (number {} -> {})",
                        old_path.file_name().unwrap().to_str().unwrap(),
                        new_path.file_name().unwrap().to_str().unwrap(),
                        old_num,
                        new_num
                    );

                    if let Err(e) = fs::rename(&old_path, &new_path) {
                        println!("    Error: Could not rename file: {}", e);
                    }
                }
            }

            // Reminder to update dependencies if not in dry-run mode
            if !dry_run && !rename_operations.is_empty() {
                println!("\nRemember to update dependencies in your migration files!");
                println!("Look for 'dependencies = [...]' in each migration file");
            }
        }
    } else {
        println!("\nNo staged migration files found to renumber.");
    }
}

fn check_for_conflicts(dir: &Path, repo_root: &Path) -> bool {
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

        // Find the problematic migration files using Git status
        if let Some(branch_num) = &branch_num {
            // First try to find it directly
            if let Some(branch_file) = find_migration_file(dir, branch_num) {
                println!("\n!!! PROBLEMATIC MIGRATION FILE !!!");
                println!("File: {}", branch_file.display());
                println!("This is the migration from the branch you're rebasing.");

                // Check if this file is staged or conflicted in Git
                let relative_path = branch_file
                    .strip_prefix(repo_root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .unwrap_or_default();

                let repo = Repository::open(repo_root).ok();
                if let Some(repo) = repo {
                    if let Ok(statuses) = repo.statuses(None) {
                        for status_entry in statuses.iter() {
                            let status = status_entry.status();
                            let path = status_entry.path().unwrap_or("");

                            if path == relative_path {
                                println!("\nGit Status: {}", git_status_to_string(status));
                                break;
                            }
                        }
                    }
                }
            } else {
                println!(
                    "\nBranch migration file not found for prefix {}_",
                    branch_num
                );
                println!("This is unusual and may indicate a more complex conflict.");
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
        return true;
    }

    false
}

fn git_status_to_string(status: Status) -> String {
    let mut status_str = Vec::new();

    if status.is_index_new() {
        status_str.push("new in index");
    }
    if status.is_index_modified() {
        status_str.push("modified in index");
    }
    if status.is_index_deleted() {
        status_str.push("deleted in index");
    }
    if status.is_index_renamed() {
        status_str.push("renamed in index");
    }
    if status.is_index_typechange() {
        status_str.push("typechange in index");
    }
    if status.is_wt_new() {
        status_str.push("new in workdir");
    }
    if status.is_wt_modified() {
        status_str.push("modified in workdir");
    }
    if status.is_wt_deleted() {
        status_str.push("deleted in workdir");
    }
    if status.is_wt_renamed() {
        status_str.push("renamed in workdir");
    }
    if status.is_wt_typechange() {
        status_str.push("typechange in workdir");
    }
    if status.is_conflicted() {
        status_str.push("conflicted");
    }

    if status_str.is_empty() {
        "unknown".to_string()
    } else {
        status_str.join(", ")
    }
}

struct MigrationFile {
    path: PathBuf,
    number: u32,
    name: String,
    is_staged: bool,
}

fn extract_migration_info(file_path: &Path) -> Option<(u32, String)> {
    let file_name = file_path.file_name()?.to_str()?;

    // Skip __init__.py and other non-migration files
    if !file_name.ends_with(".py") || file_name == "__init__.py" {
        return None;
    }

    // Find the first underscore which separates number from name
    let underscore_pos = file_name.find('_')?;
    if underscore_pos == 0 {
        return None;
    }

    // Extract the number part and convert to integer
    let number_str = &file_name[..underscore_pos];
    let number = number_str.parse::<u32>().ok()?;

    // Extract the name part without the .py extension
    let name = if file_name.ends_with(".py") {
        let py_len = ".py".len();
        file_name[underscore_pos..(file_name.len() - py_len)].to_string()
    } else {
        file_name[underscore_pos..].to_string()
    };

    Some((number, name))
}

fn get_all_migrations_in_dir(dir: &Path) -> Vec<MigrationFile> {
    let mut migrations = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("py") {
                if let Some((number, name)) = extract_migration_info(&path) {
                    migrations.push(MigrationFile {
                        path,
                        number,
                        name,
                        is_staged: false,
                    });
                }
            }
        }
    }

    migrations.sort_by_key(|m| m.number);
    migrations
}
