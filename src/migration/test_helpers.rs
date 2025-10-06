//! Test helper functions shared across migration tests

#[cfg(test)]
use crate::migration::change::{MigrationDependencyChange, MigrationFileNameChange};
#[cfg(test)]
use crate::migration::file::{Migration, MigrationFileName, MAX_MIGRATION_TXT, MIGRATIONS};
#[cfg(test)]
use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(test)]
use tempfile::{TempDir, tempdir};

/// Helper function to create a test environment with temp directories
#[cfg(test)]
pub fn setup_test_env() -> (TempDir, PathBuf) {
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let app_dir = temp_dir.path().join("test_app");
    let migrations_dir = app_dir.join(MIGRATIONS);
    fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
    (temp_dir, migrations_dir)
}

/// Helper function to create a max_migration.txt file
#[cfg(test)]
pub fn create_max_migration_file(migrations_dir: &Path, migration_name: &str) {
    let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
    fs::write(&max_migration_path, format!("{}\n", migration_name))
        .expect("Failed to write max migration file");
}

/// Helper function to create a test migration file
#[cfg(test)]
pub fn create_test_migration_file(
    dir: &Path,
    number: u32,
    name: &str,
    dependencies: Vec<(&str, &str)>,
) -> PathBuf {
    fs::create_dir_all(dir).expect("Failed to create migration directory");

    let file_path = dir.join(format!("{:04}_{}.py", number, name));

    // Generate dependency lines
    let dependency_lines = if dependencies.is_empty() {
        String::new()
    } else {
        dependencies
            .iter()
            .map(|(dep_app, dep_migration)| format!("        ('{}', {}),", dep_app, dep_migration))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let content = format!(
        r#"
from django.db import migrations, models

class Migration(migrations.Migration):
    dependencies = [
{}
    ]

    operations = []
"#,
        dependency_lines
    );
    fs::write(&file_path, content).expect("Failed to write test migration file");
    file_path
}

/// Helper function to create an in-memory Migration struct for unit tests.
/// This does NOT create actual files on disk - use create_test_migration_file for that.
#[cfg(test)]
pub fn create_in_memory_migration(
    file_name: &str,
    app_name: &str,
    name_change: Option<MigrationFileNameChange>,
    dependency_change: Option<MigrationDependencyChange>,
) -> Migration {
    Migration {
        file_path: PathBuf::from(format!(
            "/test/app/{}/migrations/{}.py",
            app_name, file_name
        )),
        file_name: MigrationFileName::try_from(file_name.to_string()).unwrap(),
        app_name: app_name.to_string(),
        dependencies: vec![],
        from_rebased_branch: false,
        name_change,
        dependency_change,
    }
}
