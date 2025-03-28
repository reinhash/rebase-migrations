use git2::Repository;

use rustpython_parser::{Parse, ast};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

fn find_staged_migrations(repo_path: &Path) -> Vec<PathBuf> {
    let mut staged_migrations = Vec::new();
    let Ok(repo) = Repository::open(repo_path) else {
        return staged_migrations;
    };

    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_ignored(false)
        .include_untracked(false)
        .include_unmodified(false);

    let Ok(statuses) = repo.statuses(Some(&mut status_opts)) else {
        return staged_migrations;
    };

    for status_entry in statuses.iter() {
        let status = status_entry.status();
        let Some(path) = status_entry.path() else {
            continue;
        };

        let is_staged =
            status.is_index_new() || status.is_index_modified() || status.is_index_renamed();

        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        if is_staged
            && path.contains("migrations")
            && path.ends_with(".py")
            && path != "__init__.py"
        {
            let file_name = Path::new(path).file_name().unwrap_or_default();
            let file_name_str = file_name.to_string_lossy();

            if let Some(pos) = file_name_str.find('_') {
                if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_ascii_digit()) {
                    staged_migrations.push(repo_path.join(path));
                }
            }
        }
    }

    staged_migrations
}

fn stringify_migration_path(migration: &Path) -> Option<String> {
    if let Some(file_name) = migration.file_name() {
        let file_name_str = file_name.to_string_lossy();
        return Some(file_name_str.to_string());
    }
    None
}

fn get_number_from_migration(migration: &Path) -> Option<u32> {
    let file_name_str = stringify_migration_path(migration)?;
    if let Some(pos) = file_name_str.find('_') {
        if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_ascii_digit()) {
            let number_str = &file_name_str[..pos];
            return number_str.parse::<u32>().ok();
        }
    }
    None
}

fn get_name_from_migration(migration: &Path) -> Option<String> {
    let file_name_str = stringify_migration_path(migration)?;
    if let Some(pos) = file_name_str.find('_') {
        if pos > 0 {
            let name = &file_name_str[pos + 1..];
            if let Some(name_stripped) = name.strip_suffix(".py") {
                return Some(name_stripped.to_string());
            }
            return Some(name.to_string());
        }
    }
    None
}

fn find_migration_string_location_in_file(
    python_path: &PathBuf,
    app_name: &str,
) -> Result<(u32, u32), String> {
    let python_source = std::fs::read_to_string(python_path)
        .map_err(|e| format!("Failed to read file {}: {}", python_path.display(), e))?;
    let python_statements =
        ast::Suite::parse(&python_source, python_path.to_str().unwrap()).unwrap(); // statements
    for statement in python_statements {
        if let ast::Stmt::ClassDef(class) = statement {
            if &class.name.to_string() == "Migration" {
                for item in class.body {
                    if let ast::Stmt::Assign(assign) = item {
                        let mut found = false;
                        for target in &assign.targets {
                            if let ast::Expr::Name(name) = &target {
                                if &name.id == "dependencies" {
                                    found = true;
                                    break;
                                }
                            }
                        }
                        if found {
                            if let ast::Expr::List(dep_list) = &assign.value.as_ref() {
                                for tuple in &dep_list.elts {
                                    if let ast::Expr::Tuple(tuple_items) = tuple {
                                        let django_app = {
                                            match tuple_items.elts.first().unwrap() {
                                                ast::Expr::Constant(django_app) => {
                                                    django_app.value.as_str().unwrap()
                                                }
                                                _ => panic!(
                                                    "Expected a django app name as first item of the tuple."
                                                ),
                                            }
                                        };
                                        // we only want to update the dependencies for the current app
                                        if django_app != app_name {
                                            continue;
                                        }

                                        let migration_name_expression =
                                            tuple_items.elts.get(1).unwrap();
                                        if let ast::Expr::Constant(mig_const) =
                                            migration_name_expression
                                        {
                                            let range = mig_const.range;
                                            let start = u32::from(range.start());
                                            let end = u32::from(range.end());
                                            return Ok((start, end));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(format!(
        "Failed to find migration string location in file {}",
        python_path.display()
    ))
}

fn replace_range_in_file(
    file_path: &str,
    start_offset: usize,
    end_offset: usize,
    replacement: &str,
    dry_run: bool,
) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    let content = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file {file_path}: {e}"))?;
    let new_content = format!(
        "{}{}{}",
        &content[..start_offset],
        replacement,
        &content[end_offset..]
    );
    std::fs::write(file_path, new_content)
        .map_err(|e| format!("Failed to write to file {file_path}: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Migration {
    number: u32,
    name: String,
    new_number: Option<u32>,
    previous_migration_name: Option<String>,
}

impl Migration {
    fn old_full_path(&self, migration_dir: &Path) -> PathBuf {
        migration_dir
            .join(format!("{:04}_{}", self.number, self.name))
            .with_extension("py")
    }
    fn new_full_path(&self, migration_dir: &Path) -> Option<PathBuf> {
        let number = self.new_number?;
        let new_name = format!("{:04}_{}", number, self.name);
        let new_path = migration_dir.join(new_name);
        Some(new_path.with_extension("py"))
    }
}

#[derive(Debug)]
struct MigrationGroup {
    migrations: HashMap<u32, Migration>,
    migration_dir: PathBuf,
    migration_name_changes: Option<HashMap<String, String>>,
}

impl MigrationGroup {
    fn find_highest_migration_number(&self) -> Option<u32> {
        self.migrations.keys().max().copied()
    }

    /// This function should find the last head migration in the group
    /// Our new migrations lowest number could be smaller than the last head migration.
    /// It could be the same.
    /// But it can never be larger.
    /// So we need to find the last head migration in the group.
    fn find_last_head_migration(&self) -> Option<PathBuf> {
        let files = self.sorted_files_in_dir();
        let migration_paths = self.migration_paths();
        let mut highest_number = 0;
        let mut last_head_migration = None;
        for file in files {
            if let Some(file_name) = file.file_name() {
                let file_path = file.clone();
                if migration_paths.contains(&file_path) {
                    continue;
                }
                let file_name_str = file_name.to_string_lossy();
                if let Some(pos) = file_name_str.find('_') {
                    if pos > 0 && file_name_str[..pos].chars().all(|c| c.is_ascii_digit()) {
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
    fn sorted_files_in_dir(&self) -> Vec<PathBuf> {
        let dir = self.migration_dir.clone();
        let mut files = Vec::new();
        if let Ok(entries) = dir.read_dir() {
            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if std::path::Path::new(file_name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
                        && file_name != "__init__.py"
                    {
                        files.push(entry.path());
                    }
                }
            }
        }
        files.sort();
        files
    }

    /// Returns the Django app name from the migration directory.
    /// The app name is the folder name on level above the migration directory.
    fn get_app_name(&self) -> &str {
        let levels: Vec<_> = self.migration_dir.components().collect();
        levels[levels.len() - 2].as_os_str().to_str().unwrap()
    }

    fn migration_paths(&self) -> Vec<PathBuf> {
        self.migrations
            .values()
            .map(|migration| migration.old_full_path(&self.migration_dir))
            .collect()
    }

    fn add_previous_migration_name(&mut self, last_head_migration: &Path) {
        let lowest_number = self
            .migrations
            .values()
            .map(|migration| migration.number)
            .min()
            .unwrap_or(0);
        let migrations_lookup = self.migrations.clone();

        for (number, migration) in &mut self.migrations {
            if number == &lowest_number {
                migration.previous_migration_name =
                    Some(get_name_from_migration(last_head_migration).unwrap());
            } else {
                migration.previous_migration_name = Some(
                    migrations_lookup
                        .get(&(migration.number - 1))
                        .unwrap()
                        .name
                        .clone(),
                );
            }
        }
    }

    fn generate_new_migration_numbers(&mut self, last_head_migration: &Path) {
        let last_head_migration_number = get_number_from_migration(last_head_migration).unwrap();
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
    fn rename_files(&self, dry_run: bool) -> Result<(), String> {
        for migration in self.migrations.values() {
            let new_path = migration
                .new_full_path(&self.migration_dir)
                .ok_or_else(|| {
                    format!(
                        "Failed to generate new path for migration {}",
                        migration.name
                    )
                })?;
            if dry_run {
                println!(
                    "DRY RUN: Renaming {} to {}",
                    migration.old_full_path(&self.migration_dir).display(),
                    new_path.display()
                );
            } else if let Err(e) =
                std::fs::rename(migration.old_full_path(&self.migration_dir), &new_path)
            {
                return Err(format!(
                    "Failed to rename migration {}: {}",
                    migration.name, e
                ));
            }
        }
        Ok(())
    }

    /// Uses Python AST to parse the migration files and find the dependencies array.
    /// Then it updates the entry in the dependencies array with the new migration file name.
    fn update_migration_file_dependencies(&self, dry_run: bool) -> Result<(), String> {
        let app_name = self.get_app_name();
        for migration in self.migrations.values() {
            let python_path = if dry_run {
                migration.old_full_path(&self.migration_dir)
            } else {
                migration.new_full_path(&self.migration_dir).unwrap()
            };
            let (start, end) = find_migration_string_location_in_file(&python_path, app_name)?;
            let replacement = {
                let number = migration.new_number.unwrap() - 1;
                let name = migration.previous_migration_name.as_ref().unwrap();
                format!("'{number:04}_{name}'")
            };
            replace_range_in_file(
                python_path.to_str().unwrap(),
                start as usize,
                end as usize,
                &replacement,
                dry_run,
            )?;
        }
        Ok(())
    }

    /// Write the migration with the highest number in the file
    fn update_max_migration_file(&self, dry_run: bool) {
        if dry_run {
            println!("DRY RUN: Updating max migration file");
        } else {
            let max_migration_number = self.find_highest_migration_number().unwrap();
            let max_migration = self.migrations.get(&max_migration_number).unwrap();
            let max_migration_path = self
                .migration_dir
                .join("max_migration")
                .with_extension("txt");
            let content = format!(
                "{:04}_{}\n",
                max_migration.new_number.unwrap(),
                max_migration.name
            );
            std::fs::write(max_migration_path, content).unwrap();
        }
    }
}

impl MigrationGroup {
    fn create(migrations: Vec<PathBuf>) -> Vec<Self> {
        let mut grouped_migrations: Vec<Self> = Vec::new();

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
                            previous_migration_name: None,
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
                        previous_migration_name: None,
                    },
                );

                grouped_migrations.push(Self {
                    migrations: migrations_map,
                    migration_dir: parent_dir,
                    migration_name_changes: None,
                });
            }
        }
        grouped_migrations
    }
}

pub fn fix(search_path: &str, dry_run: bool) -> Result<(), String> {
    let migrations = find_staged_migrations(Path::new(search_path));
    let mut migration_groups = MigrationGroup::create(migrations);
    for group in &mut migration_groups {
        if let Some(last_head_migration) = group.find_last_head_migration() {
            group.generate_new_migration_numbers(&last_head_migration);
            group.add_previous_migration_name(&last_head_migration);
            group.rename_files(dry_run)?;
            group.update_migration_file_dependencies(dry_run)?;
            group.update_max_migration_file(dry_run);
        }
    }
    Ok(())
}
