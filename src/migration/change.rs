use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::migration::file::{MigrationDependency, MigrationFileName};
use crate::migration::parser::MigrationParser;
use crate::utils::replace_range_in_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFileNameChange {
    pub old_name: MigrationFileName,
    pub new_name: MigrationFileName,
}

impl MigrationFileNameChange {
    pub fn new(old_name: MigrationFileName, new_name: MigrationFileName) -> Self {
        Self { old_name, new_name }
    }

    pub fn apply_change(&self, migrations_dir: &Path) -> Result<(), String> {
        let old_path = migrations_dir.join(&self.old_name.0).with_extension("py");
        let new_path = migrations_dir.join(&self.new_name.0).with_extension("py");
        std::fs::rename(old_path, new_path).map_err(|e| format!("Failed to rename file: {}", e))?;
        Ok(())
    }
}

impl Display for MigrationFileNameChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.old_name.0, self.new_name.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationDependencyChange {
    pub old_dependencies: Vec<MigrationDependency>,
    pub new_dependencies: Vec<MigrationDependency>,
}

impl MigrationDependencyChange {
    pub fn new(
        old_dependencies: Vec<MigrationDependency>,
        new_dependencies: Vec<MigrationDependency>,
    ) -> Self {
        Self {
            old_dependencies,
            new_dependencies,
        }
    }

    pub fn apply_change(&self, migration_path: &PathBuf) -> Result<(), String> {
        let parser = MigrationParser::new(migration_path)?;
        let (start, end) = parser.find_dependency_location()?;
        let replacement = self.generate_replacement_string();
        replace_range_in_file(
            migration_path
                .to_str()
                .expect("Migration file path must be valid UTF-8"),
            start as usize,
            end as usize,
            &replacement,
        )?;
        Ok(())
    }

    fn generate_replacement_string(&self) -> String {
        let mut parts = Vec::new();
        for dep in &self.new_dependencies {
            parts.push(format!("('{}', '{}')", dep.app, dep.migration_file.0));
        }
        format!("dependencies = [{}]", parts.join(", "))
    }
}

impl Display for MigrationDependencyChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let old_deps: Vec<String> = self
            .old_dependencies
            .iter()
            .map(|d| d.to_string())
            .collect();
        let new_deps: Vec<String> = self
            .new_dependencies
            .iter()
            .map(|d| d.to_string())
            .collect();
        write!(f, "[{}] -> [{}]", old_deps.join(", "), new_deps.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::file::MIGRATIONS;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_migration_filename_change_apply_change() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a test migration file
        let old_filename = "0001_initial.py";
        let old_file_path = migrations_dir.join(old_filename);
        fs::write(&old_file_path, "# Test migration content").expect("Failed to create test file");

        // Verify the old file exists
        assert!(old_file_path.exists());

        // Create a MigrationFileNameChange
        let name_change = MigrationFileNameChange::new(
            MigrationFileName("0001_initial".to_string()),
            MigrationFileName("0003_initial".to_string()),
        );

        // Apply the change
        let result = name_change.apply_change(&migrations_dir);
        assert!(result.is_ok());

        // Verify the old file no longer exists and new file exists
        assert!(!old_file_path.exists());
        let new_file_path = migrations_dir.join("0003_initial.py");
        assert!(new_file_path.exists());

        // Verify the content was preserved
        let content = fs::read_to_string(&new_file_path).expect("Failed to read new file");
        assert_eq!(content, "# Test migration content");
    }

    #[test]
    fn test_migration_filename_change_apply_change_nonexistent_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a MigrationFileNameChange for a non-existent file
        let name_change = MigrationFileNameChange::new(
            MigrationFileName("0001_nonexistent".to_string()),
            MigrationFileName("0003_nonexistent".to_string()),
        );

        // Apply the change - should fail
        let result = name_change.apply_change(&migrations_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to rename file"));
    }

    #[test]
    fn test_migration_dependency_change_apply_change() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a test migration file with dependencies
        let migration_content = r#"
from django.db import migrations, models

class Migration(migrations.Migration):
    dependencies = [
        ('myapp', '0001_initial'),
        ('otherapp', '0002_add_field'),
    ]

    operations = [
        migrations.CreateModel(
            name='TestModel',
            fields=[
                ('id', models.AutoField(primary_key=True)),
            ],
        ),
    ]
"#;
        let migration_file = migrations_dir.join("0003_test_migration.py");
        fs::write(&migration_file, migration_content)
            .expect("Failed to create test migration file");

        // Create old and new dependencies
        let old_dependencies = vec![
            MigrationDependency {
                app: "myapp".to_string(),
                migration_file: MigrationFileName("0001_initial".to_string()),
            },
            MigrationDependency {
                app: "otherapp".to_string(),
                migration_file: MigrationFileName("0002_add_field".to_string()),
            },
        ];

        let new_dependencies = vec![
            MigrationDependency {
                app: "myapp".to_string(),
                migration_file: MigrationFileName("0005_initial".to_string()),
            },
            MigrationDependency {
                app: "otherapp".to_string(),
                migration_file: MigrationFileName("0007_add_field".to_string()),
            },
        ];

        // Create a MigrationDependencyChange
        let dependency_change = MigrationDependencyChange::new(old_dependencies, new_dependencies);

        // Apply the change
        let result = dependency_change.apply_change(&migration_file);
        assert!(result.is_ok());

        // Read the updated file and verify the dependencies were changed
        let updated_content =
            fs::read_to_string(&migration_file).expect("Failed to read updated file");
        assert!(updated_content.contains("('myapp', '0005_initial')"));
        assert!(updated_content.contains("('otherapp', '0007_add_field')"));
        assert!(!updated_content.contains("('myapp', '0001_initial')"));
        assert!(!updated_content.contains("('otherapp', '0002_add_field')"));
    }

    #[test]
    fn test_migration_dependency_change_apply_change_empty_dependencies() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a migration file with empty dependencies
        let migration_content = r#"
from django.db import migrations, models

class Migration(migrations.Migration):
    dependencies = []

    operations = []
"#;
        let migration_file = migrations_dir.join("0001_initial.py");
        fs::write(&migration_file, migration_content)
            .expect("Failed to create test migration file");

        // Create dependency change from empty to having dependencies
        let old_dependencies = vec![];
        let new_dependencies = vec![MigrationDependency {
            app: "myapp".to_string(),
            migration_file: MigrationFileName("0002_add_field".to_string()),
        }];

        let dependency_change = MigrationDependencyChange::new(old_dependencies, new_dependencies);

        // Apply the change
        let result = dependency_change.apply_change(&migration_file);
        assert!(result.is_ok());

        // Verify the dependencies were added
        let updated_content =
            fs::read_to_string(&migration_file).expect("Failed to read updated file");
        assert!(updated_content.contains("('myapp', '0002_add_field')"));
    }

    #[test]
    fn test_migration_dependency_change_apply_change_nonexistent_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        let migration_file = migrations_dir.join("nonexistent.py");

        let dependency_change = MigrationDependencyChange::new(vec![], vec![]);

        // Apply change to non-existent file - should fail
        let result = dependency_change.apply_change(&migration_file);
        assert!(result.is_err());
    }
}
