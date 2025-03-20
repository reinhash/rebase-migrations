use git2::Repository;

use std::path::{Path, PathBuf};

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

#[derive(Debug)]
struct MigrationGroup {
    migration_files: Vec<PathBuf>,
}

impl MigrationGroup {
    fn find_last_head_migration(&self) -> Option<PathBuf> {
        // This function should find the last head migration in the group
        // For now, it returns None as a placeholder
        let dir = self.directory()?;
        println!("Directory: {:?}", dir);
        let lowest_number = self.lowest_migration_number()?;
        println!("Lowest migration number: {:?}", lowest_number);
        None
    }

    fn directory(&self) -> Option<PathBuf> {
        self.migration_files
            .get(0)
            .and_then(|path| path.parent().map(|p| p.to_path_buf()))
    }

    /// Finds the lowest migration number in the group.
    fn lowest_migration_number(&self) -> Option<u32> {
        let mut lowest_number = None;
        for migration in &self.migration_files {
            if let Some(file_name) = migration.file_name() {
                let file_name_str = file_name.to_string_lossy();
                if let Some(pos) = file_name_str.find('_') {
                    if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_digit(10)) {
                        let number_str = &file_name_str[..pos];
                        if let Ok(number) = number_str.parse::<u32>() {
                            lowest_number =
                                Some(lowest_number.map_or(number, |n: u32| n.min(number)));
                        }
                    }
                }
            }
        }
        lowest_number
    }
}

impl MigrationGroup {
    fn group_by_dir(migrations: Vec<PathBuf>) -> Vec<MigrationGroup> {
        let mut grouped_migrations: Vec<MigrationGroup> = Vec::new();

        for migration in migrations {
            let parent_dir = migration.parent().unwrap();
            let mut found_group = false;

            for group in &mut grouped_migrations {
                if group.migration_files[0].parent() == Some(parent_dir) {
                    group.migration_files.push(migration.clone());
                    found_group = true;
                    break;
                }
            }

            if !found_group {
                grouped_migrations.push(MigrationGroup {
                    migration_files: vec![migration],
                });
            }
        }

        grouped_migrations
    }
}

pub fn fix(search_path: &str, dry_run: bool) {
    let migrations = find_staged_migrations(Path::new(search_path));
    let migration_groups = MigrationGroup::group_by_dir(migrations);
    for group in migration_groups {
        let last_head_migration = group.find_last_head_migration();
    }
}
