use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::migration::change::{MigrationDependencyChange, MigrationFileNameChange};
use crate::migration::file::{
    MAX_MIGRATION_TXT, MIGRATIONS, MaxMigrationFile, MaxMigrationResult, MergeConflict, Migration,
    MigrationDependency, MigrationFileName,
};

#[derive(Debug)]
pub struct DjangoApp {
    pub head_migrations: HashMap<PathBuf, Migration>,
    pub directory: PathBuf,
    pub last_common_migration: Option<MigrationFileName>,
    pub max_migration_result: MaxMigrationResult,
    pub rebased_migrations: Vec<Migration>,
}

impl TryFrom<&Path> for DjangoApp {
    type Error = String;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        if !value.join(MIGRATIONS).exists() {
            return Err("Provided path does not contain migrations folder".into());
        }
        let migrations_path = value.join(MIGRATIONS);
        if !migrations_path.join(MAX_MIGRATION_TXT).exists() {
            return Err("Provided path does not contain max_migrations.txt".into());
        }
        let django_app = DjangoApp::create(value)?;
        Ok(django_app)
    }
}

impl DjangoApp {
    /// Updates migration dependencies within this group based on file name changes.
    ///
    /// This method handles two distinct scenarios for updating migration dependencies:
    ///
    /// **Same-app processing** (`same_app = true`): Updates dependencies for rebased
    /// migrations within the current app. Special handling includes:
    /// - Dependencies on the last common migration are updated to point to the latest head migration
    /// - Other dependencies are updated based on the lookup table of file name changes
    ///
    /// **Cross-app processing** (`same_app = false`): Updates dependencies that reference
    /// migrations in other apps, using the lookup table to find renamed migration files.
    ///
    /// The method preserves existing dependency changes and only creates new
    /// `MigrationDependencyChange` objects when actual updates are needed.
    ///
    /// # Panics
    ///
    /// Panics if a head migration is expected but not found when updating dependencies
    /// on the last common migration.
    pub fn create_migration_dependency_changes(
        &mut self,
        same_app: bool,
        lookup: &HashMap<String, Vec<MigrationFileNameChange>>,
    ) {
        // if same_app is true, check all rebased migrations dependencies if there are affected.
        // - if the migration has the dependency of self.last_common_migration it needs to be set to the last head migration
        // - for all other rebased migrations, set their dependencies based on the lookup
        let app_name = self.get_app_name().to_string();
        let head_migration = self
            .find_highest_migration(true)
            .cloned()
            .expect("We must have a head migration here");

        // Combine both migration collections for processing
        let mut all_migrations: Vec<&mut Migration> = self.head_migrations.values_mut().collect();
        all_migrations.extend(self.rebased_migrations.iter_mut());

        for migration in all_migrations {
            // same app and rebased migration
            if same_app && migration.from_rebased_branch {
                let mut updated_dependencies = migration.dependencies.clone();
                let mut has_changes = false;

                for (i, dependency) in migration.dependencies.iter().enumerate() {
                    if dependency.app == app_name {
                        // 1. Case: the migration depends on the last common migration
                        if let Some(common_migration) = &self.last_common_migration {
                            if dependency.migration_file == *common_migration {
                                updated_dependencies[i] = MigrationDependency {
                                    app: app_name.clone(),
                                    migration_file: head_migration.file_name.clone(),
                                };
                                has_changes = true;
                                continue;
                            }
                        }

                        // 2. Case: check if there is any change in the lookup in the same app
                        if let Some(changes) = lookup.get(&dependency.app) {
                            for change in changes {
                                if dependency.migration_file == change.old_name {
                                    updated_dependencies[i] = MigrationDependency {
                                        app: app_name.clone(),
                                        migration_file: change.new_name.clone(),
                                    };
                                    has_changes = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if has_changes {
                    migration.dependency_change = Some(MigrationDependencyChange::new(
                        migration.dependencies.clone(),
                        updated_dependencies,
                    ));
                }
            } else {
                // not the same app
                let mut updated_dependencies = migration
                    .dependency_change
                    .as_ref()
                    .map(|dc| dc.new_dependencies.clone())
                    .unwrap_or_else(|| migration.dependencies.clone());
                let mut has_changes = migration.dependency_change.is_some();

                for dependency in updated_dependencies.iter_mut() {
                    if dependency.app != app_name {
                        if let Some(changes) = lookup.get(&dependency.app) {
                            for change in changes {
                                if dependency.migration_file == change.old_name {
                                    dependency.migration_file = change.new_name.clone();
                                    has_changes = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if has_changes {
                    migration.dependency_change = Some(MigrationDependencyChange::new(
                        migration.dependencies.clone(),
                        updated_dependencies,
                    ));
                }
            }
        }
    }

    pub fn create_migration_name_changes(&mut self, conflict: MergeConflict) {
        if self.last_common_migration.is_none() {
            return;
        }
        let highest_head_number = self
            .find_highest_migration_number(true)
            .expect("we must have migrations at this stage");

        let last_incoming_migration = Migration::try_from(
            self.directory
                .join(conflict.incoming_change.0.clone())
                .with_extension("py"),
        )
        .unwrap();
        let mut rebased_migrations = Vec::new();
        for migration in last_incoming_migration.iter() {
            if &migration.file_name == self.last_common_migration.as_ref().unwrap() {
                break;
            }
            let mut rebased_migration = migration;
            rebased_migration.from_rebased_branch = true;
            rebased_migrations.push(rebased_migration);
        }
        rebased_migrations.sort_by_key(|m| m.file_name.number());

        // Renumber rebased migrations starting from highest_head_number + 1
        let mut new_migration_number = highest_head_number + 1;
        let mut highest_new_migration = None;
        for migration in rebased_migrations.iter_mut() {
            let new_migration_name = MigrationFileName::from_number_and_name(
                new_migration_number,
                &migration.file_name.name(),
            );
            migration.name_change = Some(MigrationFileNameChange::new(
                migration.file_name.clone(),
                new_migration_name.clone(),
            ));
            highest_new_migration = Some(new_migration_name);
            new_migration_number += 1;
        }
        self.rebased_migrations = rebased_migrations;

        // Update max_migration_file if we have rebased migrations and a max_migration.txt file
        if let (Some(highest_new), MaxMigrationResult::Conflict(merge_conflict)) =
            (highest_new_migration, &mut self.max_migration_result)
        {
            self.max_migration_result = MaxMigrationResult::Ok(MaxMigrationFile {
                current_content: merge_conflict.head.clone(),
                new_content: Some(highest_new),
            });
        }
    }

    pub fn set_last_common_migration(
        &mut self,
        max_rebased_migration: MigrationFileName,
    ) -> Result<(), String> {
        let rebased_migration_path = self
            .directory
            .join(max_rebased_migration.0)
            .with_extension("py");
        let rebased_migration = Migration::try_from(rebased_migration_path).unwrap();
        for migration in rebased_migration.iter() {
            if self.head_migrations.contains_key(&migration.file_path) {
                self.last_common_migration = Some(migration.file_name);
                break;
            }
            migration.is_merge_migration()?;
        }
        Ok(())
    }

    /// Finds the highest migration number among migrations in this group.
    ///
    /// When `head_only` is true, only considers migrations from the HEAD branch
    /// (excludes rebased migrations). When false, considers all migrations regardless
    /// of their branch origin.
    ///
    /// Returns `None` if no migrations are found that match the filtering criteria.
    fn find_highest_migration_number(&self, head_only: bool) -> Option<u32> {
        let migration = self.find_highest_migration(head_only).ok()?;
        Some(migration.file_name.number())
    }

    /// Finds the single migration with the highest number in this group.
    ///
    /// When `head_only` is true, only considers migrations from the HEAD branch
    /// (excludes rebased migrations). When false, considers all migrations regardless
    /// of their branch origin.
    ///
    /// # Returns
    ///
    /// Returns the migration with the highest number, ensuring there is exactly one
    /// migration with that number.
    fn find_highest_migration(&self, head_only: bool) -> Result<&Migration, String> {
        match &self.max_migration_result {
            MaxMigrationResult::Conflict(merge_conflict) => {
                if head_only {
                    // Return the head migration from the conflict
                    self.head_migrations
                        .values()
                        .find(|m| m.file_name == merge_conflict.head)
                        .ok_or_else(|| {
                            format!("Head migration {} not found in head_migrations", merge_conflict.head)
                        })
                } else {
                    // Return the rebased migration from the conflict
                    self.rebased_migrations
                        .iter()
                        .find(|m| m.file_name == merge_conflict.incoming_change)
                        .ok_or_else(|| {
                            format!("Rebased migration {} not found in rebased_migrations", merge_conflict.incoming_change)
                        })
                }
            }
            MaxMigrationResult::Ok(max_migration_file) => {
                // Find the migration matching the max_migration.txt content
                if head_only {
                    self.head_migrations
                        .values()
                        .find(|m| m.file_name == max_migration_file.current_content)
                        .ok_or_else(|| {
                            format!("Migration {} not found in head_migrations", max_migration_file.current_content)
                        })
                } else {
                    // Check both head and rebased migrations
                    self.head_migrations
                        .values()
                        .chain(self.rebased_migrations.iter())
                        .find(|m| m.file_name == max_migration_file.current_content)
                        .ok_or_else(|| {
                            format!("Migration {} not found", max_migration_file.current_content)
                        })
                }
            }
            MaxMigrationResult::None => Err(
                "Could not find the highest migration since there is no clear indication in the max_migration.txt for this app.".into(),
            ),
        }
    }

    /// Returns the Django app name from the migration directory.
    /// The app name is the folder name on level above the migration directory.
    pub fn get_app_name(&self) -> &str {
        let levels: Vec<_> = self.directory.components().collect();
        levels[levels.len() - 2]
            .as_os_str()
            .to_str()
            .expect("We must be able to convert the app name to a string")
    }

    /// Applies all migration changes (file renames and dependency updates) and max_migration.txt updates.
    pub fn apply_changes(&self) -> Result<(), String> {
        // Combine both migration collections for applying changes
        let all_migrations: Vec<&Migration> = self
            .head_migrations
            .values()
            .chain(self.rebased_migrations.iter())
            .collect();

        for migration in all_migrations {
            migration.apply_changes()?
        }
        if let MaxMigrationResult::Ok(max_file) = &self.max_migration_result {
            max_file.apply_change(&self.directory)?;
        }
        Ok(())
    }

    /// Returns a JSON representation of all changes.
    pub fn to_json(&self) -> Result<String, String> {
        let json_changes = crate::json_output::JsonAppChanges::try_from(self)?;
        json_changes.to_json()
    }

    /// Displays a summary of all changes that will be applied.
    pub fn changes_summary(&self) {
        let has_migration_changes = self
            .head_migrations
            .values()
            .any(|m| m.name_change.is_some() || m.dependency_change.is_some())
            || self
                .rebased_migrations
                .iter()
                .any(|m| m.name_change.is_some() || m.dependency_change.is_some());

        if has_migration_changes {
            println!(
                "{}",
                crate::tables::get_table(crate::tables::TableOptions::MigrationChanges(
                    self.get_app_name(),
                    self
                ))
                .display()
                .unwrap()
            );
        }

        let has_max_migration_changes =
            if let MaxMigrationResult::Ok(max_file) = &self.max_migration_result {
                max_file.new_content.is_some()
            } else {
                false
            };

        if has_max_migration_changes {
            println!();
            println!(
                "{}",
                crate::tables::get_table(
                    crate::tables::TableOptions::SingleAppMaxMigrationChanges(
                        self.get_app_name(),
                        self
                    )
                )
                .display()
                .unwrap()
            );
        }
    }
}

impl DjangoApp {
    pub fn create(app_path: &Path) -> Result<Self, String> {
        let directory = app_path.join(MIGRATIONS);

        // 1. open max migration file
        // 2. check for conflict
        //   - with conflict: parse first HEAD, then FEATURE branch migration
        //   - no conflict: parse the file as indicated by max_migration

        let mut migrations = HashMap::new();
        let max_migration_result = Self::load_max_migration_file(&directory);
        let head = match &max_migration_result {
            MaxMigrationResult::Ok(max_migration_file) => {
                max_migration_file.current_content.clone()
            }
            MaxMigrationResult::Conflict(merge_conflict) => merge_conflict.head.clone(),
            MaxMigrationResult::None => {
                return Err(format!(
                    "Failed to parse max_migration_file under path {}",
                    directory.to_str().unwrap()
                ));
            }
        };
        let migration_path = directory.join(head.0).with_extension("py");
        let head_migration = Migration::try_from(migration_path)?;
        for migration in head_migration.iter() {
            migrations.insert(migration.file_path.clone(), migration);
        }
        Ok(Self {
            head_migrations: migrations,
            directory,
            last_common_migration: None,
            max_migration_result,
            rebased_migrations: Vec::new(),
        })
    }

    fn load_max_migration_file(directory: &Path) -> MaxMigrationResult {
        let max_migration_path = directory.join(MAX_MIGRATION_TXT);
        if !max_migration_path.exists() {
            return MaxMigrationResult::None;
        }
        let content = fs::read_to_string(&max_migration_path).unwrap();
        let content = content.trim();

        if content.is_empty() {
            return MaxMigrationResult::None;
        }
        if let Ok(merge_conflict) = MergeConflict::try_from(content.to_string()) {
            return MaxMigrationResult::Conflict(merge_conflict);
        } else if let Ok(migration_file) = MigrationFileName::try_from(content.to_string()) {
            let max_migration_file = MaxMigrationFile::from(migration_file);
            return MaxMigrationResult::Ok(max_migration_file);
        }
        MaxMigrationResult::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::change::MigrationFileNameChange;
    use crate::migration::file::{MergeConflict, MigrationFileName};
    use crate::migration::project::DjangoProject;
    use crate::migration::test_helpers::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_migration_group_create_migration_name_changes() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create test migrations - some from head, some from rebased branch
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "remove_field",
            vec![("test_app", "'0002_add_field'")],
        );
        create_test_migration_file(
            &migrations_dir,
            4,
            "update_model",
            vec![("test_app", "'0003_remove_field'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "rebased_remove_field",
            vec![("test_app", "'0002_add_field'")],
        );
        create_test_migration_file(
            &migrations_dir,
            4,
            "rebased_update_model",
            vec![("test_app", "'0003_rebased_remove_field'")],
        );

        // Create max_migration.txt file
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_path, "0004_update_model\n")
            .expect("Failed to write max migration file");

        let mut project =
            DjangoProject::from_path(migrations_dir.parent().unwrap().parent().unwrap(), false)
                .unwrap();
        let app = project.apps.get_mut("test_app").unwrap();

        // Mark migrations 3 and 4 as from rebased branch
        for (path, migration) in app.head_migrations.iter_mut() {
            let filename = path.file_name().unwrap().to_str().unwrap();
            if filename == "0003_rebased_remove_field.py"
                || filename == "0004_rebased_update_model.py"
            {
                migration.from_rebased_branch = true;
            }
        }

        // Apply migration name changes
        let mock_conflict = MergeConflict {
            head: MigrationFileName("0002_add_field".to_string()),
            incoming_change: MigrationFileName("0004_rebased_update_model".to_string()),
        };
        let _ = app.set_last_common_migration(mock_conflict.incoming_change.clone());
        app.create_migration_name_changes(mock_conflict);

        // Verify that rebased migrations got renamed and are now in rebased_migrations
        let migration_0003 = app
            .rebased_migrations
            .iter()
            .find(|m| m.file_name.0 == "0003_rebased_remove_field")
            .unwrap();
        let migration_0004 = app
            .rebased_migrations
            .iter()
            .find(|m| m.file_name.0 == "0004_rebased_update_model")
            .unwrap();

        // Migration 0003 should be renamed to 0003 (highest head is 2, so rebased start at 3)
        assert!(migration_0003.name_change.is_some());
        let name_change_0003 = migration_0003.name_change.as_ref().unwrap();
        assert_eq!(name_change_0003.old_name.0, "0003_rebased_remove_field");
        assert_eq!(name_change_0003.new_name.0, "0005_rebased_remove_field");

        // Migration 0004 should be renamed to 0004
        assert!(migration_0004.name_change.is_some());
        let name_change_0004 = migration_0004.name_change.as_ref().unwrap();
        assert_eq!(name_change_0004.old_name.0, "0004_rebased_update_model");
        assert_eq!(name_change_0004.new_name.0, "0006_rebased_update_model");
    }

    #[test]
    fn test_migration_group_create_migration_dependency_changes() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migrations with dependencies
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("test_app", "'0001_initial'")],
        );

        // Create max_migration.txt file
        create_max_migration_file(&migrations_dir, "0002_add_field");

        let mut project =
            DjangoProject::from_path(migrations_dir.parent().unwrap().parent().unwrap(), false)
                .unwrap();
        let app = project.apps.get_mut("test_app").unwrap();

        // Set up scenario: migration 0001 gets renamed and 0002 is from rebased branch
        let migration_0001_path = app
            .head_migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0001_initial.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.head_migrations.get_mut(&migration_0001_path) {
            migration.name_change = Some(MigrationFileNameChange::new(
                MigrationFileName("0001_initial".to_string()),
                MigrationFileName("0003_initial".to_string()),
            ));
        }

        // Mark migration 0002 as from rebased branch
        let migration_0002_path = app
            .head_migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0002_add_field.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.head_migrations.get_mut(&migration_0002_path) {
            migration.from_rebased_branch = true;
        }

        // Create lookup table (simulating what DjangoProject does)
        let mut lookup = std::collections::HashMap::new();
        lookup.insert(
            "test_app".to_string(),
            vec![MigrationFileNameChange::new(
                MigrationFileName("0001_initial".to_string()),
                MigrationFileName("0003_initial".to_string()),
            )],
        );

        // Test same_app = true (rebased migration within same app)
        app.create_migration_dependency_changes(true, &lookup);

        // Verify that migration 0002 has its dependency updated
        let migration_0002 = app.head_migrations.get(&migration_0002_path).unwrap();
        assert!(migration_0002.dependency_change.is_some());

        let dep_change = migration_0002.dependency_change.as_ref().unwrap();
        assert_eq!(dep_change.old_dependencies.len(), 1);
        assert_eq!(dep_change.old_dependencies[0].app, "test_app");
        assert_eq!(
            dep_change.old_dependencies[0].migration_file.0,
            "0001_initial"
        );

        assert_eq!(dep_change.new_dependencies.len(), 1);
        assert_eq!(dep_change.new_dependencies[0].app, "test_app");
        assert_eq!(
            dep_change.new_dependencies[0].migration_file.0,
            "0003_initial"
        );
    }

    #[test]
    fn test_migration_group_create_migration_dependency_changes_cross_app() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let project_path = temp_dir.path();

        // Create app_a with migration
        let app_a_dir = project_path.join("app_a");
        let migrations_a_dir = app_a_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_a_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_a_dir, 1, "initial", vec![]);

        // Create max_migration.txt for app_a
        create_max_migration_file(&migrations_a_dir, "0001_initial");

        // Create app_b with migration that depends on app_a
        let app_b_dir = project_path.join("app_b");
        let migrations_b_dir = app_b_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_b_dir).expect("Failed to create migrations directory");
        create_test_migration_file(
            &migrations_b_dir,
            1,
            "depend_on_a",
            vec![("app_a", "'0001_initial'")],
        );

        // Create max_migration.txt for app_b
        create_max_migration_file(&migrations_b_dir, "0001_depend_on_a");

        let mut project = DjangoProject::from_path(project_path, false).unwrap();

        // Create lookup table with app_a migration rename
        let mut lookup = std::collections::HashMap::new();
        lookup.insert(
            "app_a".to_string(),
            vec![MigrationFileNameChange::new(
                MigrationFileName("0001_initial".to_string()),
                MigrationFileName("0005_initial".to_string()),
            )],
        );

        // Test same_app = false (cross-app dependency update)
        let app_b = project.apps.get_mut("app_b").unwrap();
        app_b.create_migration_dependency_changes(false, &lookup);

        // Verify that app_b's migration has its dependency updated
        let migration_b = app_b
            .head_migrations
            .values()
            .find(|m| m.file_name.0 == "0001_depend_on_a")
            .unwrap();

        assert!(migration_b.dependency_change.is_some());
        let dep_change = migration_b.dependency_change.as_ref().unwrap();

        assert_eq!(dep_change.old_dependencies.len(), 1);
        assert_eq!(dep_change.old_dependencies[0].app, "app_a");
        assert_eq!(
            dep_change.old_dependencies[0].migration_file.0,
            "0001_initial"
        );

        assert_eq!(dep_change.new_dependencies.len(), 1);
        assert_eq!(dep_change.new_dependencies[0].app, "app_a");
        assert_eq!(
            dep_change.new_dependencies[0].migration_file.0,
            "0005_initial"
        );
    }

    #[test]
    fn test_django_app_apply_changes() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create test migrations - HEAD branch
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "head_change",
            vec![("test_app", "'0002_add_field'")],
        );
        // Feature branch migration
        create_test_migration_file(
            &migrations_dir,
            3,
            "rebased_change",
            vec![("test_app", "'0002_add_field'")],
        );

        // Create max_migration.txt with conflict
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        let conflict_content = r#"<<<<<<< HEAD
0003_head_change
=======
0003_rebased_change
>>>>>>> feature-branch"#;
        fs::write(&max_migration_path, conflict_content)
            .expect("Failed to write max migration file");

        // Load the app
        let mut app = DjangoApp::create(migrations_dir.parent().unwrap()).unwrap();

        // Set up the conflict and create changes
        if let MaxMigrationResult::Conflict(conflict) = &app.max_migration_result {
            let conflict = conflict.clone();
            app.set_last_common_migration(conflict.incoming_change.clone())
                .unwrap();
            app.create_migration_name_changes(conflict);
        }
        let empty_lookup = HashMap::new();
        app.create_migration_dependency_changes(true, &empty_lookup);

        // Apply changes
        let result = app.apply_changes();
        assert!(result.is_ok(), "apply_changes should succeed");

        // Verify file was renamed (from 0003 to 0004)
        assert!(
            !migrations_dir.join("0003_rebased_change.py").exists(),
            "Old file should be renamed"
        );
        assert!(
            migrations_dir.join("0004_rebased_change.py").exists(),
            "New file should exist (renumbered from 0003 to 0004)"
        );

        // Verify max_migration.txt was updated
        let max_content = fs::read_to_string(&max_migration_path).unwrap();
        assert_eq!(max_content.trim(), "0004_rebased_change");
    }

    #[test]
    fn test_django_app_changes_summary() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create test migrations - HEAD branch
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "head_change",
            vec![("test_app", "'0002_add_field'")],
        );
        // Feature branch migration
        create_test_migration_file(
            &migrations_dir,
            3,
            "rebased_change",
            vec![("test_app", "'0002_add_field'")],
        );

        // Create max_migration.txt with conflict
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        let conflict_content = r#"<<<<<<< HEAD
0003_head_change
=======
0003_rebased_change
>>>>>>> feature-branch"#;
        fs::write(&max_migration_path, conflict_content)
            .expect("Failed to write max migration file");

        // Load the app
        let mut app = DjangoApp::create(migrations_dir.parent().unwrap()).unwrap();

        // Set up the conflict and create changes
        if let MaxMigrationResult::Conflict(conflict) = &app.max_migration_result {
            let conflict = conflict.clone();
            app.set_last_common_migration(conflict.incoming_change.clone())
                .unwrap();
            app.create_migration_name_changes(conflict);
        }
        let empty_lookup = HashMap::new();
        app.create_migration_dependency_changes(true, &empty_lookup);

        // This should not panic - just testing that it runs without error
        app.changes_summary();

        // Verify that changes were created
        assert!(
            app.rebased_migrations
                .iter()
                .any(|m| m.name_change.is_some()),
            "Should have name changes"
        );
    }

    #[test]
    fn test_django_app_try_from_valid_path() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create a simple migration
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);

        // Create max_migration.txt
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_path, "0001_initial\n")
            .expect("Failed to write max migration file");

        // Test TryFrom
        let app_path = migrations_dir.parent().unwrap();
        let result = DjangoApp::try_from(app_path);

        assert!(result.is_ok(), "Should successfully create DjangoApp");
        let app = result.unwrap();
        assert_eq!(app.head_migrations.len(), 1);
    }

    #[test]
    fn test_django_app_try_from_no_migrations_folder() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let app_path = temp_dir.path().join("test_app");
        fs::create_dir_all(&app_path).expect("Failed to create app directory");

        let result = DjangoApp::try_from(app_path.as_path());

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Provided path does not contain migrations folder"
        );
    }

    #[test]
    fn test_django_app_try_from_no_max_migration_txt() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let app_path = temp_dir.path().join("test_app");
        let migrations_dir = app_path.join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        let result = DjangoApp::try_from(app_path.as_path());

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Provided path does not contain max_migrations.txt"
        );
    }
}
