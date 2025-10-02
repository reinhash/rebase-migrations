use rustpython_parser::ast;
use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::migration::change::{MigrationDependencyChange, MigrationFileNameChange};
use crate::migration::parser::MigrationParser;

pub const MIGRATIONS: &str = "migrations";
pub const MAX_MIGRATION_TXT: &str = "max_migration.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFileName(pub String);

impl TryFrom<&ast::Expr> for MigrationFileName {
    type Error = String;

    fn try_from(expr: &ast::Expr) -> Result<Self, Self::Error> {
        match expr {
            ast::Expr::Tuple(tuple) => {
                if tuple.elts.len() != 2 {
                    return Err("Tuple must have exactly 2 elements".to_string());
                }

                // Extract migration name (second element)
                match &tuple.elts[1] {
                    ast::Expr::Constant(constant) => match constant.value.as_str() {
                        Some(s) => MigrationFileName::try_from(s.to_string()),
                        None => Err("Second tuple element is not a string constant".to_string()),
                    },
                    _ => Err("Second tuple element is not a constant".to_string()),
                }
            }
            _ => Err("Expression is not a tuple".to_string()),
        }
    }
}

impl TryFrom<String> for MigrationFileName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let underscore_pos = name.find('_');
        if !underscore_pos.is_some_and(|pos| {
            pos == 4 && // Exactly 4 digits
            name[..pos].chars().all(|c| c.is_ascii_digit()) &&
            pos < name.len() - 1 // Must have content after underscore
        }) {
            return Err(format!("Invalid migration file name: {}", name));
        }
        Ok(Self(name.strip_suffix(".py").unwrap_or(&name).to_string()))
    }
}

impl Display for MigrationFileName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl MigrationFileName {
    pub fn from_number_and_name(number: u32, name: &str) -> Self {
        Self::try_from(format!("{:04}_{}", number, name)).unwrap()
    }

    pub fn name(&self) -> String {
        self.0.split_once('_').unwrap().1.to_string()
    }

    pub fn number(&self) -> u32 {
        self.0
            .split('_')
            .next()
            .expect("we validate on create, this cannot fail")
            .parse()
            .expect("we validate on create, this cannot fail")
    }
}

#[derive(Debug, Clone)]
pub struct MaxMigrationFile {
    pub current_content: MigrationFileName,
    pub new_content: Option<MigrationFileName>,
}

impl From<MigrationFileName> for MaxMigrationFile {
    fn from(value: MigrationFileName) -> Self {
        Self {
            current_content: value,
            new_content: None,
        }
    }
}

impl MaxMigrationFile {
    /// Applies the max migration file change to disk.
    ///
    /// Writes the new content to the `max_migration.txt` file in the specified
    /// migrations directory. Only performs the write operation if `new_content`
    /// is present, otherwise does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if the file write operation fails.
    pub fn apply_change(&self, migrations_dir: &Path) -> Result<(), String> {
        if let Some(new_content) = &self.new_content {
            let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
            let content = format!("{}\n", new_content.0);
            std::fs::write(&max_migration_path, content)
                .map_err(|e| format!("Failed to write max migration file: {e}"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MergeConflict {
    pub head: MigrationFileName,
    pub incoming_change: MigrationFileName,
}

impl TryFrom<String> for MergeConflict {
    type Error = String;

    /// Find one merge conflict in a file content.
    /// Currently, only one conflict is supported.
    fn try_from(content: String) -> Result<Self, Self::Error> {
        if content.contains("<<<<<<< HEAD")
            && content.contains("=======")
            && content.contains(">>>>>>> ")
        {
            let head = content.split("<<<<<<< HEAD").nth(1).unwrap_or("");
            let head = head
                .split("=======")
                .nth(0)
                .unwrap_or("")
                .trim()
                .to_string();
            let head = MigrationFileName::try_from(head)?;
            let incoming_change = content.split("=======").nth(1).unwrap_or("");
            let incoming_change = incoming_change
                .split(">>>>>>> ")
                .nth(0)
                .unwrap_or("")
                .trim()
                .to_string();
            let incoming_change = MigrationFileName::try_from(incoming_change)?;
            return Ok(Self {
                head,
                incoming_change,
            });
        }
        Err(format!("No merge conflict found in {}", content))
    }
}

#[derive(Debug)]
pub enum MaxMigrationResult {
    Ok(MaxMigrationFile),
    Conflict(MergeConflict),
    None,
}

#[derive(Debug, Clone)]
pub struct Migration {
    pub file_name: MigrationFileName,
    pub file_path: PathBuf,
    pub app_name: String,
    pub dependencies: Vec<MigrationDependency>,
    pub from_rebased_branch: bool,
    pub name_change: Option<MigrationFileNameChange>,
    pub dependency_change: Option<MigrationDependencyChange>,
}

impl TryFrom<PathBuf> for Migration {
    type Error = String;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let file_path = value.clone();
        let app_name = value
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .and_then(|os_str| os_str.to_str())
            .ok_or_else(|| "Failed to extract app name".to_string())?;
        let file_name = value
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .ok_or_else(|| "Failed to extract file name".to_string())?
            .to_string();
        let file_name = MigrationFileName::try_from(file_name)?;

        // parse dependencies recursively
        let parser = MigrationParser::new(&file_path).unwrap();
        let dependencies = parser.get_dependencies();

        Ok(Migration {
            file_name,
            file_path,
            app_name: app_name.to_string(),
            dependencies,
            from_rebased_branch: false,
            name_change: None,
            dependency_change: None,
        })
    }
}

impl Migration {
    pub fn iter(&self) -> MigrationIterator {
        MigrationIterator {
            dependency_iterator: MigrationDependencyIterator::new(self.clone()),
        }
    }

    pub fn new_full_path(&self, directory: &Path) -> Option<PathBuf> {
        let name_change = self.name_change.clone()?;
        let new_path = directory.join(name_change.new_name.0);
        Some(new_path.with_extension("py"))
    }

    /// Check that no merge migration exists in one of the rebased migrations.
    pub fn is_merge_migration(&self) -> Result<(), String> {
        let dependency_condition = self
            .dependencies
            .iter()
            .filter(|d| d.app == self.app_name)
            .count()
            > 1;
        if dependency_condition {
            return Err(format!(
                "Merge migration detected in rebased migration: {}. Currently, merge migrations cannot be resolved properly when they are not part of the HEAD branch. In fact, you can use this tool to avoid merge migrations by rebasing your feature branch on the latest HEAD migration.",
                self.file_name.0
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MigrationDependencyIterator {
    migration_stack: Vec<Migration>,
    visited: HashSet<String>,
}

impl Iterator for MigrationDependencyIterator {
    type Item = Migration;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(current_migration) = self.migration_stack.pop() {
            let migration_key = format!(
                "{}:{}",
                current_migration.app_name, current_migration.file_name.0
            );
            if self.visited.contains(&migration_key) {
                continue;
            }
            self.visited.insert(migration_key);

            // Load dependencies and add them to the stack (in reverse order for depth-first)
            for dependency in current_migration.dependencies.iter().rev() {
                // TODO: handle error and inform user?
                if let Ok(dep_migration) =
                    self.load_dependency_migration(dependency, &current_migration)
                {
                    let dep_key =
                        format!("{}:{}", dep_migration.app_name, dep_migration.file_name.0);
                    if !self.visited.contains(&dep_key) {
                        self.migration_stack.push(dep_migration);
                    }
                }
            }
            return Some(current_migration);
        }
        None
    }
}

impl MigrationDependencyIterator {
    fn new(initial_migration: Migration) -> Self {
        let migration_stack = vec![initial_migration];

        Self {
            migration_stack,
            visited: HashSet::new(),
        }
    }

    fn load_dependency_migration(
        &self,
        dependency: &MigrationDependency,
        current_migration: &Migration,
    ) -> Result<Migration, String> {
        if dependency.app == current_migration.app_name {
            let migration_path = current_migration
                .file_path
                .parent()
                .unwrap()
                .join(&dependency.migration_file.0)
                .with_extension("py");

            if migration_path.exists() {
                return Migration::try_from(migration_path);
            }
            Err(format!(
                "Could not find migration {} in app {}",
                dependency.migration_file.0, dependency.app
            ))
        } else {
            Err("Migration is of another app.".to_string())
        }
    }
}

#[derive(Debug)]
pub struct MigrationIterator {
    dependency_iterator: MigrationDependencyIterator,
}

impl Iterator for MigrationIterator {
    type Item = Migration;

    fn next(&mut self) -> Option<Self::Item> {
        self.dependency_iterator.next()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationDependency {
    pub app: String,
    pub migration_file: MigrationFileName,
}

impl Display for MigrationDependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "('{}', '{}')", self.app, self.migration_file.0)
    }
}

impl TryFrom<&ast::Expr> for MigrationDependency {
    type Error = String;

    fn try_from(expr: &ast::Expr) -> Result<Self, Self::Error> {
        match expr {
            ast::Expr::Tuple(tuple) => {
                if tuple.elts.len() != 2 {
                    return Err("Tuple must have exactly 2 elements".to_string());
                }

                // Extract app name (first element)
                let app = match &tuple.elts[0] {
                    ast::Expr::Constant(constant) => match constant.value.as_str() {
                        Some(s) => s.to_string(),
                        None => {
                            return Err("First tuple element is not a string constant".to_string());
                        }
                    },
                    _ => return Err("First tuple element is not a constant".to_string()),
                };

                // Use MigrationFileName::try_from for the migration filename
                let migration_file = MigrationFileName::try_from(expr)?;

                Ok(MigrationDependency {
                    app,
                    migration_file,
                })
            }
            _ => Err("Expression is not a tuple".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustpython_parser::Parse;

    #[test]
    fn test_migration_filename_try_from_string_valid() {
        // Test valid migration file names
        let result = MigrationFileName::try_from("0001_initial".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0001_initial");

        let result = MigrationFileName::try_from("0002_add_field".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0002_add_field");

        let result = MigrationFileName::try_from("0123_complex_migration_name".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0123_complex_migration_name");
    }

    #[test]
    fn test_migration_filename_try_from_string_with_py_extension() {
        // Test that .py extension is stripped
        let result = MigrationFileName::try_from("0001_initial.py".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0001_initial");

        let result = MigrationFileName::try_from("0042_remove_field.py".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0042_remove_field");
    }

    #[test]
    fn test_migration_filename_try_from_string_invalid() {
        // Test invalid migration file names

        // No underscore
        let result = MigrationFileName::try_from("0001initial".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        // No digits before underscore
        let result = MigrationFileName::try_from("_initial".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        // Non-digits before underscore
        let result = MigrationFileName::try_from("abc_initial".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        // Mixed alphanumeric before underscore
        let result = MigrationFileName::try_from("001a_initial".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        // Underscore at the beginning
        let result = MigrationFileName::try_from("_0001_initial".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));
    }

    #[test]
    fn test_migration_filename_try_from_string_edge_cases() {
        let result = MigrationFileName::try_from("1_test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        let result = MigrationFileName::try_from("12_test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        let result = MigrationFileName::try_from("123_test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        let result = MigrationFileName::try_from("12345_test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));

        let result = MigrationFileName::try_from("0001_".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));
    }

    #[test]
    fn test_migration_filename_try_from_string_valid_complex_names() {
        let result = MigrationFileName::try_from("0001_test_with_underscores".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "0001_test_with_underscores");

        // Long descriptive name
        let result =
            MigrationFileName::try_from("0042_add_user_profile_and_update_permissions".to_string());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().0,
            "0042_add_user_profile_and_update_permissions"
        );
    }

    #[test]
    fn test_migration_dependency_try_from_valid() {
        // Test valid migration dependency tuple
        let python_code = r#"('app_name', '0001_initial')"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_ok());

        let dependency = result.unwrap();
        assert_eq!(dependency.app, "app_name");
        assert_eq!(dependency.migration_file.0, "0001_initial");
    }

    #[test]
    fn test_migration_dependency_try_from_valid_complex() {
        // Test with longer app and migration names
        let python_code = r#"('my_complex_app', '0042_add_user_profile_and_permissions')"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_ok());

        let dependency = result.unwrap();
        assert_eq!(dependency.app, "my_complex_app");
        assert_eq!(
            dependency.migration_file.0,
            "0042_add_user_profile_and_permissions"
        );
    }

    #[test]
    fn test_migration_dependency_try_from_invalid_not_tuple() {
        // Test with non-tuple expression
        let python_code = r#"'not_a_tuple'"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Expression is not a tuple");
    }

    #[test]
    fn test_migration_dependency_try_from_invalid_tuple_length() {
        // Test with tuple having wrong number of elements
        let python_code = r#"('app_name',)"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Tuple must have exactly 2 elements");

        // Test with too many elements
        let python_code = r#"('app_name', '0001_initial', 'extra')"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Tuple must have exactly 2 elements");
    }

    #[test]
    fn test_migration_dependency_try_from_invalid_app_name() {
        // Test with non-string app name
        let python_code = r#"(123, '0001_initial')"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("First tuple element is not a string constant")
        );
    }

    #[test]
    fn test_migration_dependency_try_from_invalid_migration_name() {
        // Test with invalid migration filename format
        let python_code = r#"('app_name', 'invalid_migration')"#;
        let expr = ast::Expr::parse(python_code, "<test>").unwrap();

        let result = MigrationDependency::try_from(&expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid migration file name"));
    }

    #[test]
    fn test_max_migration_file_apply_change() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a MaxMigrationFile with new content
        let max_migration_file = MaxMigrationFile {
            current_content: MigrationFileName("0001_initial".to_string()),
            new_content: Some(MigrationFileName("0005_updated".to_string())),
        };

        // Apply the change
        let result = max_migration_file.apply_change(&migrations_dir);
        assert!(result.is_ok());

        // Verify the max_migration.txt file was created with correct content
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        assert!(max_migration_path.exists());

        let content =
            fs::read_to_string(&max_migration_path).expect("Failed to read max migration file");
        assert_eq!(content, "0005_updated\n");
    }

    #[test]
    fn test_max_migration_file_apply_change_overwrite_existing() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create an existing max_migration.txt file
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_path, "0003_old_content\n")
            .expect("Failed to create existing max migration file");

        // Create a MaxMigrationFile with new content
        let max_migration_file = MaxMigrationFile {
            current_content: MigrationFileName("0003_old_content".to_string()),
            new_content: Some(MigrationFileName("0007_new_content".to_string())),
        };

        // Apply the change
        let result = max_migration_file.apply_change(&migrations_dir);
        assert!(result.is_ok());

        // Verify the file was overwritten with new content
        let content =
            fs::read_to_string(&max_migration_path).expect("Failed to read max migration file");
        assert_eq!(content, "0007_new_content\n");
    }

    #[test]
    fn test_max_migration_file_apply_change_no_new_content() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create a MaxMigrationFile without new content
        let max_migration_file = MaxMigrationFile {
            current_content: MigrationFileName("0001_initial".to_string()),
            new_content: None,
        };

        // Apply the change - should do nothing
        let result = max_migration_file.apply_change(&migrations_dir);
        assert!(result.is_ok());

        // Verify no file was created
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        assert!(!max_migration_path.exists());
    }

    #[test]
    fn test_max_migration_file_apply_change_invalid_directory() {
        use std::path::PathBuf;

        // Use a path that doesn't exist and can't be created
        let invalid_dir = PathBuf::from("/nonexistent/invalid/path/migrations");

        let max_migration_file = MaxMigrationFile {
            current_content: MigrationFileName("0001_initial".to_string()),
            new_content: Some(MigrationFileName("0005_updated".to_string())),
        };

        // Apply the change - should fail
        let result = max_migration_file.apply_change(&invalid_dir);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Failed to write max migration file")
        );
    }

    #[test]
    fn test_merge_conflict_try_from_valid() {
        let conflict_content = r#"<<<<<<< HEAD
0001_initial.py
=======
0002_add_field.py
>>>>>>> feature-branch"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_ok());

        let conflict = result.unwrap();
        assert_eq!(conflict.head.0, "0001_initial");
        assert_eq!(conflict.incoming_change.0, "0002_add_field");
    }

    #[test]
    fn test_merge_conflict_try_from_with_whitespace() {
        let conflict_content = r#"<<<<<<< HEAD
  0003_remove_field.py  
=======
  0004_modify_model.py  
>>>>>>> refactor-branch"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_ok());

        let conflict = result.unwrap();
        assert_eq!(conflict.head.0, "0003_remove_field");
        assert_eq!(conflict.incoming_change.0, "0004_modify_model");
    }

    #[test]
    fn test_merge_conflict_try_from_missing_head_marker() {
        let conflict_content = r#"0001_initial.py
=======
0002_add_field.py
>>>>>>> feature-branch"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No merge conflict found"));
    }

    #[test]
    fn test_merge_conflict_try_from_missing_separator() {
        let conflict_content = r#"<<<<<<< HEAD
0001_initial.py
0002_add_field.py
>>>>>>> feature-branch"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No merge conflict found"));
    }

    #[test]
    fn test_merge_conflict_try_from_missing_end_marker() {
        let conflict_content = r#"<<<<<<< HEAD
0001_initial.py
=======
0002_add_field.py"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No merge conflict found"));
    }

    #[test]
    fn test_merge_conflict_try_from_empty_content() {
        let result = MergeConflict::try_from(String::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No merge conflict found"));
    }

    #[test]
    fn test_merge_conflict_try_from_multiline_content() {
        let conflict_content = r#"Some other content before
<<<<<<< HEAD
0005_complex_migration.py
Additional content
=======
0006_new_migration.py
More content here
>>>>>>> complex-feature
Some content after"#;

        let result = MergeConflict::try_from(conflict_content.to_string());
        assert!(result.is_ok());

        let conflict = result.unwrap();
        assert_eq!(
            conflict.head.0,
            "0005_complex_migration.py\nAdditional content"
        );
        assert_eq!(
            conflict.incoming_change.0,
            "0006_new_migration.py\nMore content here"
        );
    }
}
