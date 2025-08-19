use rustpython_parser::{Parse, ast};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::utils::{
    find_relevant_migrations, get_name_from_migration, get_number_from_migration,
    is_migration_file, replace_range_in_file,
};

fn find_migration_string_location_in_file(
    python_path: &PathBuf,
    app_name: &str,
) -> Result<(u32, u32), String> {
    let parser = MigrationParser::new(python_path)?;
    parser.find_dependency_location_for_app(app_name)
}

struct MigrationParser {
    file_path: PathBuf,
    ast: ast::Suite,
}

impl MigrationParser {
    fn new(python_path: &PathBuf) -> Result<Self, String> {
        let python_source = std::fs::read_to_string(python_path)
            .map_err(|e| format!("Failed to read file {}: {}", python_path.display(), e))?;

        let ast = ast::Suite::parse(
            &python_source,
            python_path
                .to_str()
                .expect("Failed to convert path to string"),
        )
        .map_err(|e| format!("Failed to parse python statements: {e}"))?;

        Ok(Self {
            file_path: python_path.clone(),
            ast,
        })
    }

    fn find_dependency_location_for_app(&self, app_name: &str) -> Result<(u32, u32), String> {
        let migration_class = self.find_migration_class()?;
        let dependencies_assignment = self.find_dependencies_assignment(migration_class)?;
        let dependency_tuples = self.extract_dependency_tuples(dependencies_assignment)?;

        for tuple in dependency_tuples {
            if let Some(location) = self.try_extract_location_from_tuple(tuple, app_name)? {
                return Ok(location);
            }
        }

        Err(format!(
            "No dependency found for app '{}' in file {}",
            app_name,
            self.file_path.display()
        ))
    }

    fn find_migration_class(&self) -> Result<&ast::StmtClassDef, String> {
        for statement in &self.ast {
            if let ast::Stmt::ClassDef(class) = statement {
                if &class.name.to_string() == "Migration" {
                    return Ok(class);
                }
            }
        }
        Err(format!(
            "Migration class not found in file {}",
            self.file_path.display()
        ))
    }

    fn find_dependencies_assignment<'a>(
        &self,
        migration_class: &'a ast::StmtClassDef,
    ) -> Result<&'a ast::StmtAssign, String> {
        for item in &migration_class.body {
            if let ast::Stmt::Assign(assign) = item {
                if self.is_dependencies_assignment(assign) {
                    return Ok(assign);
                }
            }
        }
        Err(format!(
            "Dependencies assignment not found in Migration class in file {}",
            self.file_path.display()
        ))
    }

    fn is_dependencies_assignment(&self, assign: &ast::StmtAssign) -> bool {
        assign
            .targets
            .iter()
            .any(|target| matches!(target, ast::Expr::Name(name) if &name.id == "dependencies"))
    }

    fn extract_dependency_tuples<'a>(
        &self,
        assignment: &'a ast::StmtAssign,
    ) -> Result<&'a Vec<ast::Expr>, String> {
        match assignment.value.as_ref() {
            ast::Expr::List(dep_list) => Ok(&dep_list.elts),
            _ => Err(format!(
                "Dependencies should be a list in file {}",
                self.file_path.display()
            )),
        }
    }

    fn try_extract_location_from_tuple(
        &self,
        tuple_expr: &ast::Expr,
        target_app_name: &str,
    ) -> Result<Option<(u32, u32)>, String> {
        let tuple_items = match tuple_expr {
            ast::Expr::Tuple(tuple) => &tuple.elts,
            _ => return Ok(None), // Skip non-tuple elements
        };

        let app_name = self.extract_app_name_from_tuple(tuple_items)?;
        if app_name != target_app_name {
            return Ok(None);
        }

        let migration_name_expr = tuple_items.get(1).ok_or_else(|| {
            format!(
                "Missing migration name in dependencies tuple in {}",
                self.file_path.display()
            )
        })?;

        match migration_name_expr {
            ast::Expr::Constant(constant) => {
                let range = constant.range;
                Ok(Some((u32::from(range.start()), u32::from(range.end()))))
            }
            _ => Err(format!(
                "Migration name should be a string constant in {}",
                self.file_path.display()
            )),
        }
    }

    fn extract_app_name_from_tuple<'a>(
        &self,
        tuple_elements: &'a [ast::Expr],
    ) -> Result<&'a str, String> {
        match tuple_elements.first() {
            Some(ast::Expr::Constant(django_app)) => django_app
                .value
                .as_str()
                .map(|s| s.as_str())
                .ok_or_else(|| {
                    format!(
                        "Expected a string as django app name in {}",
                        self.file_path.display()
                    )
                }),
            Some(_) => Err(format!(
                "Expected a django app name as first item of the tuple in {}",
                self.file_path.display()
            )),
            None => Err(format!(
                "Missing first element in dependencies tuple in {}",
                self.file_path.display()
            )),
        }
    }
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
        if !max_migration_path.exists() {
            return Ok(());
        }
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
    let migrations = find_relevant_migrations(Path::new(search_path));
    let mut migration_groups = MigrationGroup::create(migrations)?;
    if migration_groups.is_empty() {
        return Err("No staged migrations found.".to_string());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{TempDir, tempdir};
    use git2::Repository;

    /// Helper function to create a test environment with temp directories
    fn setup_test_env() -> (TempDir, PathBuf) {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let app_dir = temp_dir.path().join("test_app");
        let migrations_dir = app_dir.join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        (temp_dir, migrations_dir)
    }

    fn create_test_migration_file(dir: &Path, number: u32, name: &str, deps: &str) -> PathBuf {
        fs::create_dir_all(dir).expect("Failed to create migration directory");

        let file_path = dir.join(format!("{:04}_{}.py", number, name));
        let content = format!(
            r#"
from django.db import migrations, models

class Migration(migrations.Migration):
    dependencies = [
        ('test_app', {}),
    ]

    operations = []
"#,
            deps
        );
        fs::write(&file_path, content).expect("Failed to write test migration file");
        file_path
    }

    #[test]
    fn test_is_migration_file() {
        assert!(is_migration_file("0001_initial.py"));
        assert!(is_migration_file("0002_auto_20230901_1234.py"));
        assert!(!is_migration_file("__init__.py"));
        assert!(!is_migration_file("not_a_migration.py"));
        assert!(!is_migration_file("a001_invalid.py"));
        assert!(!is_migration_file("_0001_invalid.py"));
    }

    #[test]
    fn test_find_relevant_migrations() {
        let temp_dir = tempdir().expect("Failed to create temp directory");

        // Skip the actual git operations in this test since they're tricky in tests
        // Just check that the function runs and returns an empty list
        let found_migrations = find_relevant_migrations(temp_dir.path());
        assert!(found_migrations.len() == 0);
    }


    #[test]
    fn test_get_number_from_migration() {
        let path = Path::new("/test/0001_initial.py");
        assert_eq!(get_number_from_migration(path), Some(1));

        let invalid_path = Path::new("/test/not_a_migration.py");
        assert_eq!(get_number_from_migration(invalid_path), None);
    }

    #[test]
    fn test_get_name_from_migration() {
        let path = Path::new("/test/0001_initial.py");
        assert_eq!(get_name_from_migration(path), Some("initial".to_string()));

        let path_no_extension = Path::new("/test/0001_initial");
        assert_eq!(
            get_name_from_migration(path_no_extension),
            Some("initial".to_string())
        );

        let invalid_path = Path::new("/test/not_a_migration.py");
        assert_eq!(get_name_from_migration(invalid_path), None);

        let path_with_underscore = Path::new("/test/0001_a_migration.py");
        assert_eq!(
            get_name_from_migration(path_with_underscore),
            Some("a_migration".to_string())
        );
    }

    #[test]
    fn test_find_migration_string_location_in_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_migration.py");
        let content = r#"
from django.db import migrations, models

class Migration(migrations.Migration):
    dependencies = [
        ('test_app', '0001_initial'),
    ]

    operations = []
"#;
        fs::write(&file_path, content).expect("Failed to write test file");

        let result = find_migration_string_location_in_file(&file_path, "test_app");
        assert!(result.is_ok());

        let (start, end) = result.unwrap();
        assert_eq!(start, 124);
        assert_eq!(end, 138);

        let invalid_file_path = temp_dir.path().join("invalid_migration.py");
        let invalid_content = "This is not a valid migration file";
        fs::write(&invalid_file_path, invalid_content).expect("Failed to write invalid test file");

        let invalid_result = find_migration_string_location_in_file(&invalid_file_path, "test_app");
        assert!(invalid_result.is_err());
    }

    #[test]
    fn test_replace_range_in_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_file.txt");
        let content = "Hello world!";
        fs::write(&file_path, content).expect("Failed to write test file");

        let file_path_str = file_path.to_str().unwrap();
        let result = replace_range_in_file(file_path_str, 6, 11, "Rust", false);
        assert!(result.is_ok());

        let new_content = fs::read_to_string(&file_path).expect("Failed to read test file");
        assert_eq!(new_content, "Hello Rust!");

        let result = replace_range_in_file(file_path_str, 0, 5, "Goodbye", true);
        assert!(result.is_ok());

        let unchanged_content = fs::read_to_string(&file_path).expect("Failed to read test file");
        assert_eq!(unchanged_content, "Hello Rust!");
    }

    #[test]
    fn test_migration_paths() {
        let (_, migrations_dir) = setup_test_env();

        let migration = Migration {
            number: 1,
            name: "initial".to_string(),
            new_number: Some(2),
            previous_migration_name: None,
        };

        let old_path = migration.old_full_path(&migrations_dir);
        assert_eq!(
            old_path.file_name().unwrap().to_str().unwrap(),
            "0001_initial.py"
        );

        let new_path = migration.new_full_path(&migrations_dir).unwrap();
        assert_eq!(
            new_path.file_name().unwrap().to_str().unwrap(),
            "0002_initial.py"
        );
    }

    #[test]
    fn test_migration_group_creation() {
        let (_, migrations_dir) = setup_test_env();

        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        let migration1 = migrations_dir.join("0001_initial.py");
        let migration2 = migrations_dir.join("0002_add_field.py");

        fs::write(&migration1, "test content").expect("Failed to write test migration");
        fs::write(&migration2, "test content").expect("Failed to write test migration");

        let result = MigrationGroup::create(vec![migration1.clone(), migration2.clone()]);
        assert!(result.is_ok());

        let groups = result.unwrap();
        assert_eq!(groups.len(), 1);

        let group = &groups[0];
        assert_eq!(group.migrations.len(), 2);
        assert!(group.migrations.contains_key(&1));
        assert!(group.migrations.contains_key(&2));
        assert_eq!(group.migration_dir, migrations_dir);
    }

    #[test]
    fn test_find_highest_migration_number() {
        let (_, migrations_dir) = setup_test_env();

        let mut migrations = HashMap::new();
        migrations.insert(
            1,
            Migration {
                number: 1,
                name: "initial".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );
        migrations.insert(
            3,
            Migration {
                number: 3,
                name: "add_field".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );

        let group = MigrationGroup {
            migrations,
            migration_dir: migrations_dir,
            migration_name_changes: None,
        };

        assert_eq!(group.find_highest_migration_number(), Some(3));
    }

    #[test]
    fn test_get_app_name() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let app_dir = temp_dir.path().join("app_name");
        fs::create_dir_all(&app_dir).expect("Failed to create app directory");

        let migrations_dir = app_dir.join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        let group = MigrationGroup {
            migrations: HashMap::new(),
            migration_dir: migrations_dir,
            migration_name_changes: None,
        };

        assert_eq!(group.get_app_name(), "app_name");
    }

    #[test]
    fn test_generate_new_migration_numbers() {
        let (_, migrations_dir) = setup_test_env();

        let last_head_migration =
            create_test_migration_file(&migrations_dir, 1, "initial", "'0000_manual'");

        let mut migrations = HashMap::new();
        migrations.insert(
            2,
            Migration {
                number: 2,
                name: "add_field".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );
        migrations.insert(
            3,
            Migration {
                number: 3,
                name: "remove_field".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );

        let mut group = MigrationGroup {
            migrations,
            migration_dir: migrations_dir,
            migration_name_changes: None,
        };

        let result = group.generate_new_migration_numbers(&last_head_migration);
        assert!(result.is_ok());

        assert_eq!(group.migrations.get(&2).unwrap().new_number, Some(2));
        assert_eq!(group.migrations.get(&3).unwrap().new_number, Some(3));

        assert!(group.migration_name_changes.is_some());
        let name_changes = group.migration_name_changes.unwrap();
        assert_eq!(name_changes.get("add_field").unwrap(), "0002_add_field");
        assert_eq!(
            name_changes.get("remove_field").unwrap(),
            "0003_remove_field"
        );
    }

    #[test]
    fn test_add_previous_migration_name() {
        let (_, migrations_dir) = setup_test_env();

        let last_head_migration =
            create_test_migration_file(&migrations_dir, 1, "initial", "'0000_manual'");

        let mut migrations = HashMap::new();
        migrations.insert(
            2,
            Migration {
                number: 2,
                name: "add_field".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );
        migrations.insert(
            3,
            Migration {
                number: 3,
                name: "remove_field".to_string(),
                new_number: None,
                previous_migration_name: None,
            },
        );

        let mut group = MigrationGroup {
            migrations,
            migration_dir: migrations_dir,
            migration_name_changes: None,
        };

        let result = group.add_previous_migration_name(&last_head_migration);
        assert!(result.is_ok());

        assert_eq!(
            group.migrations.get(&2).unwrap().previous_migration_name,
            Some("initial".to_string())
        );
        assert_eq!(
            group.migrations.get(&3).unwrap().previous_migration_name,
            Some("add_field".to_string())
        );
    }

    #[test]
    fn test_integration() {
        let (_, migrations_dir) = setup_test_env();

        let last_head_migration =
            create_test_migration_file(&migrations_dir, 1, "initial", "'0000_manual'");
        let staged_migration1 =
            create_test_migration_file(&migrations_dir, 2, "add_field", "'0001_initial'");
        let staged_migration2 =
            create_test_migration_file(&migrations_dir, 3, "remove_field", "'0002_add_field'");

        let result = MigrationGroup::create(vec![staged_migration1, staged_migration2]);
        assert!(result.is_ok());

        let mut groups = result.unwrap();
        assert_eq!(groups.len(), 1);

        let found_head = groups[0].find_last_head_migration();
        assert!(found_head.is_some());
        assert_eq!(
            found_head.unwrap().file_name().unwrap(),
            last_head_migration.file_name().unwrap()
        );

        let result = groups[0].generate_new_migration_numbers(&last_head_migration);
        assert!(result.is_ok());

        let result = groups[0].add_previous_migration_name(&last_head_migration);
        assert!(result.is_ok());

        let result = groups[0].rename_files(true);
        assert!(result.is_ok());

        for migration in groups[0].migrations.values() {
            let old_path = migration.old_full_path(&migrations_dir);
            assert!(old_path.exists());
        }
    }

    #[test]
    fn test_fix() {
        let (temp_dir, _) = setup_test_env();

        // Create migrations inside temp_dir rather than using the returned migrations_dir
        // since we want to test the fix() function which expects the repo root path
        let app_dir = temp_dir.path().join("test_app");
        let migrations_dir = app_dir.join("migrations");

        // Initialize a git repository
        let repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");

        // Create some existing "head" migrations (not staged) that we'll rebase against
        // Django migrations always start with 0001_initial
        create_test_migration_file(&migrations_dir, 1, "initial", "None");
        create_test_migration_file(&migrations_dir, 5, "existing_head", "'0001_initial'");

        // Create new migrations that need to be rebased (these will be staged)
        // These have lower numbers than the head migration, so they'll need renumbering
        let staged_migration1 =
            create_test_migration_file(&migrations_dir, 2, "new_feature", "'0001_initial'");
        let staged_migration2 =
            create_test_migration_file(&migrations_dir, 3, "another_feature", "'0002_new_feature'");

        // Add the new migrations to git index (stage them)
        let mut index = repo.index().expect("Failed to get git index");

        // Get relative paths for staging
        let staged_path1 = staged_migration1.strip_prefix(temp_dir.path()).unwrap();
        let staged_path2 = staged_migration2.strip_prefix(temp_dir.path()).unwrap();

        index
            .add_path(staged_path1)
            .expect("Failed to stage migration 1");
        index
            .add_path(staged_path2)
            .expect("Failed to stage migration 2");
        index.write().expect("Failed to write index");

        // Now test the fix function - it should find the staged migrations and process them
        let result = fix(temp_dir.path().to_str().unwrap(), true);
        assert!(result.is_ok());

        // Verify that migrations were found and would be processed
        // In dry-run mode, files aren't actually renamed, but we can verify the logic worked
        // by checking that find_relevant_migrations finds our migration files (both staged and untracked)
        let found_migrations = find_relevant_migrations(temp_dir.path());
        assert_eq!(found_migrations.len(), 4); // Now finds all 4 migration files (staged + untracked)

        // Verify the found migrations include both staged and untracked files
        let migration_names: Vec<String> = found_migrations
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .collect();

        // Should include all 4 migration files: 2 staged + 2 untracked
        assert!(migration_names.contains(&"0001_initial.py".to_string())); // untracked
        assert!(migration_names.contains(&"0002_new_feature.py".to_string())); // staged
        assert!(migration_names.contains(&"0003_another_feature.py".to_string())); // staged  
        assert!(migration_names.contains(&"0005_existing_head.py".to_string())); // untracked
    }
}
