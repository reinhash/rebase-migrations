use git2::Repository;

use rustpython_parser::{Parse, ast};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Check if the given string is a migration file name.
/// A migration file name should start with a number followed by an underscore.
/// For example: `"0001_initial.py"`, `"0002_auto_20230901_1234.py"`
fn is_migration_file(s: &str) -> bool {
    s.find('_')
        .is_some_and(|pos| pos > 0 && s[..pos].chars().all(|c| c.is_ascii_digit()))
}

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

            if is_migration_file(&file_name_str) {
                staged_migrations.push(repo_path.join(path));
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
    if is_migration_file(&file_name_str) {
        let number_str = &file_name_str[..4];
        return number_str.parse::<u32>().ok();
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
    let python_statements = ast::Suite::parse(
        &python_source,
        python_path
            .to_str()
            .expect("Failed to convert path to string"),
    )
    .map_err(|e| format!("Failed to parse python statements: {e}"))?;
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
                                        let django_app = match tuple_items.elts.first() {
                                            Some(expr) => match expr {
                                                ast::Expr::Constant(django_app) => {
                                                    django_app.value.as_str()
                                                        .expect("Since we are using a string, we should be able to convert it to a str")
                                                }
                                                _ => return Err(format!(
                                                    "Expected a django app name as first item of the tuple in {}",
                                                    python_path.display()
                                                )),
                                            },
                                            None => return Err(format!(
                                                "Missing first element in dependencies tuple in {}",
                                                python_path.display()
                                            )),
                                        };

                                        // we only want to update the dependencies for the current app
                                        if django_app != app_name {
                                            continue;
                                        }

                                        let migration_name_expression = tuple_items.elts.get(1)
                                            .ok_or_else(|| format!(
                                                "Missing migration name in dependencies tuple in {}",
                                                python_path.display()
                                            ))?;

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
    fn find_last_head_migration(&self) -> Option<PathBuf> {
        let files = self.sorted_files_in_dir();
        let migration_paths = self.migration_paths();
        let mut highest_number = 0;
        let mut last_head_migration = None;
        for file in files {
            if let Some(file_name) = file.file_name() {
                let file_path = file.clone();
                if migration_paths.contains(&file_path) {
                    // We could have migrations with much higher numbers
                    // that are coming from our branch. But we are only
                    // interested in the last migration of the branch
                    // that we rebase against.
                    continue;
                }
                let file_name_str = file_name.to_string_lossy();
                if is_migration_file(&file_name_str) {
                    let number = get_number_from_migration(&file)?;
                    if number >= highest_number {
                        highest_number = number;
                        last_head_migration = Some(file_path);
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
        levels[levels.len() - 2]
            .as_os_str()
            .to_str()
            .expect("We must be able to convert the app name to a string")
    }

    fn migration_paths(&self) -> Vec<PathBuf> {
        self.migrations
            .values()
            .map(|migration| migration.old_full_path(&self.migration_dir))
            .collect()
    }

    fn add_previous_migration_name(&mut self, last_head_migration: &Path) -> Result<(), String> {
        let lowest_number = self
            .migrations
            .values()
            .map(|migration| migration.number)
            .min()
            .ok_or_else(|| "No migrations found".to_string())?;
        let migrations_lookup = self.migrations.clone();

        for (number, migration) in &mut self.migrations {
            if number == &lowest_number {
                let previous_migration_name = get_name_from_migration(last_head_migration)
                    .ok_or_else(|| {
                        format!(
                            "Failed to get migration name from path: {}",
                            last_head_migration.display()
                        )
                    })?;
                migration.previous_migration_name = Some(previous_migration_name);
            } else {
                migration.previous_migration_name = Some(
                    migrations_lookup
                        .get(&(migration.number - 1))
                        .ok_or_else(|| {
                            format!("Failed to find previous migration for {}", migration.name)
                        })?
                        .name
                        .clone(),
                );
            }
        }
        Ok(())
    }

    fn generate_new_migration_numbers(&mut self, last_head_migration: &Path) -> Result<(), String> {
        let last_head_migration_number = get_number_from_migration(last_head_migration)
            .ok_or_else(|| {
                format!(
                    "Failed to get migration number from path: {}",
                    last_head_migration.display()
                )
            })?;
        let mut new_migration_names = HashMap::new();
        // Get migrations sorted by their number
        let mut sorted_migrations: Vec<&mut Migration> = self.migrations.values_mut().collect();
        sorted_migrations.sort_by_key(|m| m.number);

        let mut new_migration_number = last_head_migration_number + 1;
        for migration in sorted_migrations {
            migration.new_number = Some(new_migration_number);

            let new_migration_name = format!("{:04}_{}", new_migration_number, migration.name);
            new_migration_names.insert(migration.name.clone(), new_migration_name);
            new_migration_number += 1;
        }
        self.migration_name_changes = Some(new_migration_names);
        Ok(())
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
                    "Would rename {} to {}",
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
        if dry_run {
            return Ok(());
        }
        let app_name = self.get_app_name();
        for migration in self.migrations.values() {
            let python_path = migration
                .new_full_path(&self.migration_dir)
                .ok_or_else(|| {
                    format!(
                        "Failed to get new full path for migration {}",
                        migration.name
                    )
                })?;
            let (start, end) = find_migration_string_location_in_file(&python_path, app_name)?;
            let replacement = {
                let number = migration
                    .new_number
                    .expect("New Migration numbers must be set at this point.")
                    - 1;
                let name = migration
                    .previous_migration_name
                    .as_ref()
                    .expect("Previous migration name must be set at this point.");
                format!("'{number:04}_{name}'")
            };
            replace_range_in_file(
                python_path
                    .to_str()
                    .expect("Migration file path must be valid UTF-8"),
                start as usize,
                end as usize,
                &replacement,
                dry_run,
            )?;
        }
        Ok(())
    }

    /// Write the migration with the highest number in the file
    fn update_max_migration_file(&self, dry_run: bool) -> Result<(), String> {
        if dry_run {
            return Ok(());
        }
        let max_migration_number = self.find_highest_migration_number().unwrap_or(0);
        let max_migration = self.migrations.get(&max_migration_number).ok_or_else(|| {
            format!("No migration found for the highest number: {max_migration_number}")
        })?;
        let max_migration_path = self
            .migration_dir
            .join("max_migration")
            .with_extension("txt");
        let content = format!(
            "{:04}_{}\n",
            max_migration
                .new_number
                .expect("New migration number must be set."),
            max_migration.name
        );
        std::fs::write(max_migration_path, content)
            .map_err(|e| format!("Failed to write max migration file: {e}"))?;
        Ok(())
    }
}

impl MigrationGroup {
    fn create(migrations: Vec<PathBuf>) -> Result<Vec<Self>, String> {
        let mut grouped_migrations: Vec<Self> = Vec::new();

        for migration_path in migrations {
            let parent_dir = match migration_path.parent() {
                Some(dir) => dir.to_path_buf(),
                None => {
                    return Err(format!(
                        "Failed to get parent directory for migration path: {}",
                        migration_path.display()
                    ));
                }
            };
            let migration_number = get_number_from_migration(&migration_path).ok_or_else(|| {
                format!(
                    "Failed to get migration number from path: {}",
                    migration_path.display()
                )
            })?;
            let migration_name = get_name_from_migration(&migration_path).ok_or_else(|| {
                format!(
                    "Failed to get migration name from path: {}",
                    migration_path.display()
                )
            })?;

            let mut found_group = false;
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
        Ok(grouped_migrations)
    }
}

pub fn fix(search_path: &str, dry_run: bool) -> Result<(), String> {
    if dry_run {
        println!("Dry run detected. No changes will be made.");
    }
    let migrations = find_staged_migrations(Path::new(search_path));
    let mut migration_groups = MigrationGroup::create(migrations)?;
    for group in &mut migration_groups {
        if let Some(last_head_migration) = group.find_last_head_migration() {
            group.generate_new_migration_numbers(&last_head_migration)?;
            group.add_previous_migration_name(&last_head_migration)?;
            group.rename_files(dry_run)?;
            group.update_migration_file_dependencies(dry_run)?;
            group.update_max_migration_file(dry_run)?;
        }
    }
    Ok(())
}
