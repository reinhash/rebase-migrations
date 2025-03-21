use git2::Repository;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

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

fn get_number_from_migration(migration: &PathBuf) -> Option<u32> {
    if let Some(file_name) = migration.file_name() {
        let file_name_str = file_name.to_string_lossy();
        if let Some(pos) = file_name_str.find('_') {
            if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_digit(10)) {
                let number_str = &file_name_str[..pos];
                return number_str.parse::<u32>().ok();
            }
        }
    }
    None
}

fn get_name_from_migration(migration: &PathBuf) -> Option<String> {
    if let Some(file_name) = migration.file_name() {
        let file_name_str = file_name.to_string_lossy();
        if let Some(pos) = file_name_str.find('_') {
            if pos > 0 {
                return Some(file_name_str[pos + 1..].to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
struct Migration {
    number: u32,
    name: String,
    new_number: Option<u32>,
}

impl Migration {
    fn old_full_path(&self, migration_dir: &PathBuf) -> PathBuf {
        migration_dir
            .join(format!("{:04}_{}", self.number, self.name))
            .with_extension("py")
    }
    fn new_full_path(&self, migration_dir: &PathBuf) -> Option<PathBuf> {
        let number = self.new_number?;
        let new_name = format!("{:04}_{}", number, self.name);
        let new_path = migration_dir.join(new_name);
        Some(new_path.with_extension("py").into())
    }
}

#[derive(Debug)]
struct MigrationGroup {
    migrations: HashMap<u32, Migration>,
    migration_dir: PathBuf,
    migration_name_changes: Option<HashMap<String, String>>,
}

impl MigrationGroup {
    /// This function should find the last head migration in the group
    /// Our new migrations lowest number could be smaller than the last head migration.
    /// It could be the same.
    /// But it can never be larger.
    /// So we need to find the last head migration in the group.
    fn find_last_head_migration(&self) -> Option<PathBuf> {
        let files = self.sorted_files_in_dir()?;
        let migration_paths = self.migration_paths();
        let mut highest_number = 0;
        let mut last_head_migration = None;
        for file in files {
            if let Some(file_name) = file.file_name() {
                let file_path = file.to_path_buf();
                if migration_paths.contains(&file_path) {
                    continue;
                }
                let file_name_str = file_name.to_string_lossy();
                if let Some(pos) = file_name_str.find('_') {
                    if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_digit(10)) {
                        let number_str = &file_name_str[..pos];
                        if let Ok(number) = number_str.parse::<u32>() {
                            if number >= highest_number {
                                highest_number = number;
                                last_head_migration = Some(file_path);
                            }
                        }
                    }
                }
            }
        }
        last_head_migration
    }

    /// Finds all current files in the migration directory.
    fn sorted_files_in_dir(&self) -> Option<Vec<PathBuf>> {
        let dir = self.migration_dir.clone();
        let mut files = Vec::new();
        if let Ok(entries) = dir.read_dir() {
            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if file_name.ends_with(".py") && file_name != "__init__.py" {
                        files.push(entry.path());
                    }
                }
            }
        }
        files.sort_by(|a, b| a.cmp(b));
        Some(files)
    }

    fn migration_paths(&self) -> Vec<PathBuf> {
        self.migrations
            .iter()
            .map(|(_, migration)| migration.old_full_path(&self.migration_dir))
            .collect()
    }

    fn generate_new_migration_numbers(&mut self, last_head_migration: PathBuf) {
        let last_head_migration_number = get_number_from_migration(&last_head_migration).unwrap();
        let mut new_migration_names = HashMap::new();

        // Get migrations sorted by their number
        let mut sorted_migrations: Vec<&mut Migration> = self.migrations.values_mut().collect();
        sorted_migrations.sort_by_key(|m| m.number);

        // Start numbering from last_head_migration_number + 1
        let mut new_migration_number = last_head_migration_number + 1;
        for migration in sorted_migrations {
            // Set the new_number field in the Migration struct
            migration.new_number = Some(new_migration_number);

            let new_migration_name = format!("{:04}_{}", new_migration_number, migration.name);
            new_migration_names.insert(migration.name.clone(), new_migration_name);
            new_migration_number += 1;
        }

        self.migration_name_changes = Some(new_migration_names);
    }
    fn rename_files(&self) {
        for migration in self.migrations.values() {
            if let Some(new_path) = migration.new_full_path(&self.migration_dir) {
                if let Err(e) =
                    std::fs::rename(migration.old_full_path(&self.migration_dir), new_path)
                {
                    eprintln!("Error renaming file: {}", e);
                }
            }
        }
    }

    fn update_migration_file_dependencies(&self) {}
}

impl MigrationGroup {
    fn create(migrations: Vec<PathBuf>) -> Vec<MigrationGroup> {
        let mut grouped_migrations: Vec<MigrationGroup> = Vec::new();

        for migration_path in migrations {
            let parent_dir = migration_path.parent().unwrap().to_path_buf();
            let mut found_group = false;
            let migration_number = get_number_from_migration(&migration_path).unwrap();
            let migration_name = get_name_from_migration(&migration_path).unwrap();

            for group in &mut grouped_migrations {
                if group.migration_dir == parent_dir {
                    group.migrations.insert(
                        migration_number,
                        Migration {
                            number: migration_number,
                            name: migration_name.clone(),
                            new_number: None,
                        },
                    );
                    found_group = true;
                    break;
                }
            }

            if !found_group {
                let mut migrations_map = HashMap::new();
                migrations_map.insert(
                    migration_number,
                    Migration {
                        number: migration_number,
                        name: migration_name,
                        new_number: None,
                    },
                );

                grouped_migrations.push(MigrationGroup {
                    migrations: migrations_map,
                    migration_dir: parent_dir,
                    migration_name_changes: None,
                });
            }
        }
        grouped_migrations
    }
}

pub fn fix(search_path: &str, dry_run: bool) {
    let migrations = find_staged_migrations(Path::new(search_path));
    let mut migration_groups = MigrationGroup::create(migrations);
    for group in migration_groups.iter_mut() {
        if let Some(last_head_migration) = group.find_last_head_migration() {
            println!("Last head migration: {}", last_head_migration.display());
            group.generate_new_migration_numbers(last_head_migration);
            println!("Group: {:?}", group);
            if !dry_run {
                group.rename_files();
                group.update_migration_file_dependencies();
            }
        }
    }
}
