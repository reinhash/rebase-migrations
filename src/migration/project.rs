use std::{collections::HashMap, path::Path};
use walkdir::WalkDir;

use crate::migration::change::MigrationFileNameChange;
use crate::migration::file::{MAX_MIGRATION_TXT, MIGRATIONS, MaxMigrationResult, Migration};
use crate::migration::group::MigrationGroup;
use crate::tables::{TableOptions, get_table};

// Common directories to skip during traversal for performance
//
// We do not need ".venv" or "node_modules" or similar since our django apps will
// not be found there. But for edge cases where a user might name their django app
// "node_modules" or similar for whatever reason, we provide the all-dirs flag for
// a comprehensive scan.
const SKIP_DIRECTORIES: &[&str] = &[
    // Version control
    ".git",
    ".svn",
    ".hg",
    // Python environments
    "venv",
    "env",
    ".venv",
    ".env",
    "virtualenv",
    "__pycache__",
    ".pytest_cache",
    ".tox",
    // Node.js
    "node_modules",
    ".npm",
    ".yarn",
    // Build/cache directories
    "build",
    "dist",
    ".cache",
    "target",
    ".mypy_cache",
    ".coverage",
    "htmlcov",
    // IDE/Editor directories
    ".vscode",
    ".idea",
    ".sublime-project",
    ".sublime-workspace",
    // OS directories
    ".DS_Store",
    "Thumbs.db",
    // Django specific
    "static",
    "staticdirs",
    "staticfiles",
    "static_collected",
    "media",
    // Docker
    ".docker",
    // Documentation build
    "_build",
    "docs/_build",
    // Documentation
    "docs",
];

pub fn fix(search_path: &str, dry_run: bool, all_dirs: bool) -> Result<(), String> {
    if dry_run {
        println!("Dry run detected. No changes will be made.");
    }
    let search_path = Path::new(search_path);
    let mut django_project = DjangoProject::from_path(search_path, all_dirs)?;
    if django_project.apps.is_empty() {
        return Err("No Django apps with migrations found.".to_string());
    }
    for group in django_project.apps.values_mut() {
        if let MaxMigrationResult::Conflict(conflict) = &group.max_migration_result {
            let conflict_clone = conflict.clone();
            group.set_last_common_migration(conflict_clone.incoming_change.clone())?;
            group.create_migration_name_changes(conflict_clone);
        }
    }
    // first create all name changes within the same app, then create all dependency changes for other apps.
    django_project.create_migration_dependency_changes(true);
    django_project.create_migration_dependency_changes(false);

    if dry_run {
        django_project.changes_summary();
    } else {
        django_project.apply_changes()?;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct DjangoProject {
    pub(crate) apps: HashMap<String, MigrationGroup>,
}

impl DjangoProject {
    pub(crate) fn from_path(repo_path: &Path, all_dirs: bool) -> Result<Self, String> {
        let mut apps = HashMap::new();

        let walkdir = WalkDir::new(repo_path);
        let walkdir_iter: Box<dyn Iterator<Item = walkdir::Result<walkdir::DirEntry>>> = if all_dirs
        {
            // Scan all directories without skipping
            Box::new(walkdir.into_iter())
        } else {
            // Apply directory filtering for performance.
            Box::new(walkdir.into_iter().filter_entry(|e| {
                if e.path().is_dir() {
                    if let Some(dir_name) = e.path().file_name().and_then(|name| name.to_str()) {
                        !SKIP_DIRECTORIES.contains(&dir_name)
                    } else {
                        true
                    }
                } else {
                    true
                }
            }))
        };

        for entry in walkdir_iter.filter_map(|e| e.ok()) {
            let path = entry.path();

            if path.is_dir() && path.file_name() == Some(std::ffi::OsStr::new(MIGRATIONS)) {
                let max_migration_path = path.join(MAX_MIGRATION_TXT);
                if !max_migration_path.exists() {
                    continue;
                }

                let app_path = path.parent().ok_or_else(|| {
                    format!(
                        "Invalid app directory for migrations folder: {}",
                        path.display()
                    )
                })?;
                let migration_group = MigrationGroup::create(app_path)?;
                let app_name = migration_group.get_app_name();
                apps.insert(app_name.to_string(), migration_group);
            }
        }

        Ok(Self { apps })
    }

    /// Creates dependency changes for all migrations across all Django apps.
    ///
    /// This method coordinates the dependency update process by first building a lookup
    /// table of all migration file name changes across all apps, then delegating to
    /// each `MigrationGroup` to update its migrations' dependencies.
    ///
    /// When `same_app` is true, enables special handling for same-app dependencies
    /// where rebased migrations that depend on the last common migration will be
    /// updated to depend on the latest head migration instead.
    ///
    /// The process works in two phases:
    /// 1. Builds a lookup table mapping app names to their migration file name changes
    /// 2. Calls each `MigrationGroup` to update dependencies using the lookup table
    ///
    /// This ensures all apps have visibility into migration name changes from other
    /// apps when updating cross-app dependencies.
    fn create_migration_dependency_changes(&mut self, same_app: bool) {
        let mut migration_file_changes_lookup: HashMap<String, Vec<MigrationFileNameChange>> =
            HashMap::new();
        for group in self.apps.values() {
            let app_name = group.get_app_name().to_string();
            let mut changes: Vec<MigrationFileNameChange> = group
                .head_migrations
                .values()
                .filter_map(|m| m.name_change.clone())
                .collect();

            // Add changes from rebased migrations
            let rebased_changes: Vec<MigrationFileNameChange> = group
                .rebased_migrations
                .iter()
                .filter_map(|m| m.name_change.clone())
                .collect();
            changes.extend(rebased_changes);

            migration_file_changes_lookup.insert(app_name, changes);
        }
        for group in self.apps.values_mut() {
            group.create_migration_dependency_changes(same_app, &migration_file_changes_lookup);
        }
    }

    fn apply_changes(&mut self) -> Result<(), String> {
        for group in self.apps.values() {
            let migrations_dir = group.directory.clone();

            // Combine both migration collections for applying changes
            let all_migrations: Vec<&Migration> = group
                .head_migrations
                .values()
                .chain(group.rebased_migrations.iter())
                .collect();

            for migration in all_migrations {
                if let Some(changes) = &migration.name_change {
                    changes.apply_change(&migrations_dir)?
                }
                if let Some(changes) = &migration.dependency_change {
                    let migration_path =
                        if let Some(new_path) = migration.new_full_path(&migrations_dir) {
                            new_path
                        } else {
                            migration.file_path.clone()
                        };
                    changes.apply_change(&migration_path)?
                }
            }
            if let MaxMigrationResult::Ok(max_file) = &group.max_migration_result {
                max_file.apply_change(&migrations_dir)?;
            }
        }
        Ok(())
    }

    fn changes_summary(&self) {
        println!(
            "{}",
            get_table(TableOptions::Summary(&self.apps))
                .display()
                .unwrap()
        );
        for group in self.apps.values() {
            let has_migration_changes = group
                .head_migrations
                .values()
                .any(|m| m.name_change.is_some() || m.dependency_change.is_some())
                || group
                    .rebased_migrations
                    .iter()
                    .any(|m| m.name_change.is_some() || m.dependency_change.is_some());

            if has_migration_changes {
                println!();
                println!(
                    "{}",
                    get_table(TableOptions::MigrationChanges(group.get_app_name(), group))
                        .display()
                        .unwrap()
                );
            }
        }

        let has_max_migration_changes = self.apps.values().any(|group| {
            if let MaxMigrationResult::Ok(max_file) = &group.max_migration_result {
                max_file.new_content.is_some()
            } else {
                false
            }
        });
        if has_max_migration_changes {
            println!();
            println!(
                "{}",
                get_table(TableOptions::MaxMigrationChanges(&self.apps))
                    .display()
                    .unwrap()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::change::MigrationFileNameChange;
    use crate::migration::file::{MaxMigrationFile, MaxMigrationResult, MigrationFileName};
    use crate::migration::test_helpers::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_django_project_from_path() {
        let (temp_dir, migrations_dir) = setup_test_env();

        // Create test migrations
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("test_app", "'0001_initial'")],
        );

        // Create max_migration.txt file
        create_max_migration_file(&migrations_dir, "0002_add_field");

        let result = DjangoProject::from_path(temp_dir.path(), false);
        assert!(result.is_ok());

        let project = result.unwrap();
        assert_eq!(project.apps.len(), 1);
        assert!(project.apps.contains_key("test_app"));

        let app = project.apps.get("test_app").unwrap();
        assert_eq!(app.head_migrations.len(), 2);
    }

    #[test]
    fn test_django_project_from_path_empty() {
        let temp_dir = tempdir().expect("Failed to create temp directory");

        let result = DjangoProject::from_path(temp_dir.path(), false);
        assert!(result.is_ok());

        let project = result.unwrap();
        assert_eq!(project.apps.len(), 0);
    }

    #[test]
    fn test_create_migration_dependency_changes() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let project_path = temp_dir.path();

        // Create app_a with one migration
        let app_a_dir = project_path.join("app_a");
        let migrations_a_dir = app_a_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_a_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_a_dir, 1, "initial", vec![]);

        // Create max_migration.txt for app_a
        let max_migration_a_path = migrations_a_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_a_path, "0001_initial\n")
            .expect("Failed to write max migration file");

        // Create app_b with a migration that depends on app_a
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
        let max_migration_b_path = migrations_b_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_b_path, "0001_depend_on_a\n")
            .expect("Failed to write max migration file");

        let mut project = DjangoProject::from_path(project_path, false).unwrap();

        // Simulate renaming app_a's migration from 0001_initial to 0005_initial
        let app_a = project.apps.get_mut("app_a").unwrap();
        let migration_path = app_a
            .head_migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0001_initial.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app_a.head_migrations.get_mut(&migration_path) {
            migration.name_change = Some(MigrationFileNameChange::new(
                MigrationFileName("0001_initial".to_string()),
                MigrationFileName("0005_initial".to_string()),
            ));
        }

        // Call the method we're testing
        project.create_migration_dependency_changes(false);

        // Verify that app_b's migration dependency was updated
        let app_b = project.apps.get("app_b").unwrap();
        let migration_b = app_b
            .head_migrations
            .values()
            .find(|m| m.file_name.0 == "0001_depend_on_a")
            .unwrap();

        assert!(migration_b.dependency_change.is_some());
        let dep_change = migration_b.dependency_change.as_ref().unwrap();

        // Original dependency should be app_a.0001_initial
        assert_eq!(dep_change.old_dependencies.len(), 1);
        assert_eq!(dep_change.old_dependencies[0].app, "app_a");
        assert_eq!(
            dep_change.old_dependencies[0].migration_file.0,
            "0001_initial"
        );

        // New dependency should be app_a.0005_initial
        assert_eq!(dep_change.new_dependencies.len(), 1);
        assert_eq!(dep_change.new_dependencies[0].app, "app_a");
        assert_eq!(
            dep_change.new_dependencies[0].migration_file.0,
            "0005_initial"
        );
    }

    #[test]
    fn test_django_project_apply_changes() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let project_path = temp_dir.path();

        // Create app with migrations
        let app_dir = project_path.join("myapp");
        let migrations_dir = app_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("myapp", "'0001_initial'")],
        );

        // Create max_migration.txt file
        let max_migration_path = migrations_dir.join(MAX_MIGRATION_TXT);
        fs::write(&max_migration_path, "0002_add_field\n")
            .expect("Failed to write max migration file");

        let mut project = DjangoProject::from_path(project_path, false).unwrap();

        // Set up changes: rename migration and update max_migration
        let app = project.apps.get_mut("myapp").unwrap();

        // Add file name change
        let migration_path = app
            .head_migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0002_add_field.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.head_migrations.get_mut(&migration_path) {
            migration.name_change = Some(MigrationFileNameChange::new(
                MigrationFileName("0002_add_field".to_string()),
                MigrationFileName("0003_add_field".to_string()),
            ));
        }

        // Add max migration file change
        app.max_migration_result = MaxMigrationResult::Ok(MaxMigrationFile {
            current_content: MigrationFileName("0002_add_field".to_string()),
            new_content: Some(MigrationFileName("0003_add_field".to_string())),
        });

        // Apply all changes to disk
        let result = project.apply_changes();
        assert!(result.is_ok());

        // Verify file was renamed
        let old_file_path = migrations_dir.join("0002_add_field.py");
        let new_file_path = migrations_dir.join("0003_add_field.py");
        assert!(!old_file_path.exists());
        assert!(new_file_path.exists());

        // Verify max_migration.txt was updated
        let max_content =
            fs::read_to_string(&max_migration_path).expect("Failed to read max migration file");
        assert_eq!(max_content.trim(), "0003_add_field");
    }

    #[test]
    fn test_merge_migrations() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migrations with dependencies
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field_branch_a",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field_branch_b",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "merge_migration_for_number_2",
            vec![
                ("test_app", "'0002_add_field_branch_a'"),
                ("test_app", "'0002_add_field_branch_b'"),
            ],
        );
        create_test_migration_file(
            &migrations_dir,
            4,
            "regular_migration",
            vec![("test_app", "'0003_merge_migration_for_number_2'")],
        );
        create_test_migration_file(
            &migrations_dir,
            4,
            "to_be_rebased_migration",
            vec![("test_app", "'0003_merge_migration_for_number_2'")],
        );

        let max_migration_path = migrations_dir.join("max_migration.txt");
        let conflict_content = r#"<<<<<<< HEAD
0004_regular_migration.py
=======
0004_to_be_rebased_migration.py
>>>>>>> feature-branch"#;
        fs::write(&max_migration_path, conflict_content)
            .expect("Failed to write max migration file");

        let _result = fix(migrations_dir.to_str().unwrap(), false, true).unwrap();
        let mut django_project = DjangoProject::from_path(&migrations_dir, true).unwrap();
        let app = django_project.apps.get_mut("test_app").unwrap();

        // Check that the rebased migration was properly renumbered
        // After fix() runs, the file should be renamed from 0004 to 0005
        let old_migration_path = migrations_dir.join("0004_to_be_rebased_migration.py");
        let new_migration_path = migrations_dir.join("0005_to_be_rebased_migration.py");

        // The old file should no longer exist, and the new file should exist
        assert!(
            !old_migration_path.exists(),
            "Old migration file should be renamed"
        );
        assert!(
            new_migration_path.exists(),
            "New migration file should exist"
        );

        // Check the migration object in the app (it should be keyed by the new path)
        let migration_0005_to_be_rebased = app.head_migrations.get(&new_migration_path).unwrap();
        assert_eq!(migration_0005_to_be_rebased.file_name.number(), 5);
        assert_eq!(
            migration_0005_to_be_rebased.file_name.name(),
            "to_be_rebased_migration"
        );
        assert_eq!(
            migration_0005_to_be_rebased.file_name.0,
            "0005_to_be_rebased_migration"
        );

        // The migration should have the correct dependencies
        assert_eq!(migration_0005_to_be_rebased.dependencies.len(), 1);
        assert_eq!(migration_0005_to_be_rebased.dependencies[0].app, "test_app");
        // After renumbering, it should depend on the head migration (0004_regular_migration)
        assert_eq!(
            migration_0005_to_be_rebased.dependencies[0]
                .migration_file
                .0,
            "0004_regular_migration"
        );

        // Verify the regular migration (from HEAD) stays at 0004 and doesn't change
        let migration_0004_regular_path = migrations_dir.join("0004_regular_migration.py");
        let migration_0004_regular = app
            .head_migrations
            .get(&migration_0004_regular_path)
            .unwrap();

        // The regular migration should not be renamed - it stays at 0004
        assert_eq!(migration_0004_regular.file_name.number(), 4);
        assert_eq!(migration_0004_regular.file_name.name(), "regular_migration");
        assert_eq!(migration_0004_regular.file_name.0, "0004_regular_migration");

        // It should still depend on the merge migration
        assert_eq!(migration_0004_regular.dependencies.len(), 1);
        assert_eq!(migration_0004_regular.dependencies[0].app, "test_app");
        assert_eq!(
            migration_0004_regular.dependencies[0].migration_file.0,
            "0003_merge_migration_for_number_2"
        );

        // Check that max_migration.txt was updated to point to the highest migration
        let max_migration_path = migrations_dir.join("max_migration.txt");
        let max_migration_content =
            fs::read_to_string(&max_migration_path).expect("max_migration.txt should exist");
        assert_eq!(max_migration_content.trim(), "0005_to_be_rebased_migration");
    }

    #[test]
    fn test_multiple_head_merge_migrations() {
        let (temp_dir, migrations_dir) = setup_test_env();

        // Create a scenario where HEAD also has multiple migrations with the same number.
        // This should trigger an error since the tool currently only supports merge migrations
        // in HEAD, not to be rebased.
        // Timeline:
        //
        // 0001_initial
        //     │
        // 0002_shared_feature_a
        //     │
        // 0003_shared_feature_b
        //     │
        // 0004_shared_feature_c
        //     │
        // 0005_shared_feature_d
        //     │
        // 0006_shared_feature_e
        //     │
        // 0007_shared_feature_f
        //     │
        //     ├─────────────────────┐ (branches diverge at migration 8)
        //     │                     │
        // 0008_branch_a_feature_1   0008_branch_b_feature_1
        // 0008_branch_a_feature_2   0008_branch_b_feature_2
        //     │\                   /│
        //     │ \─────────────────/ │
        //     │                     │
        // 0009_branch_a_merge       0009_branch_b_merge

        // Create shared migrations (1-7)
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "shared_feature_a",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "shared_feature_b",
            vec![("test_app", "'0002_shared_feature_a'")],
        );
        create_test_migration_file(
            &migrations_dir,
            4,
            "shared_feature_c",
            vec![("test_app", "'0003_shared_feature_b'")],
        );
        create_test_migration_file(
            &migrations_dir,
            5,
            "shared_feature_d",
            vec![("test_app", "'0004_shared_feature_c'")],
        );
        create_test_migration_file(
            &migrations_dir,
            6,
            "shared_feature_e",
            vec![("test_app", "'0005_shared_feature_d'")],
        );
        create_test_migration_file(
            &migrations_dir,
            7,
            "shared_feature_f",
            vec![("test_app", "'0006_shared_feature_e'")],
        );

        // Create branch A migrations (8-9)
        create_test_migration_file(
            &migrations_dir,
            8,
            "branch_a_feature_1",
            vec![("test_app", "'0007_shared_feature_f'")],
        );
        create_test_migration_file(
            &migrations_dir,
            8,
            "branch_a_feature_2",
            vec![("test_app", "'0007_shared_feature_f'")],
        );
        create_test_migration_file(
            &migrations_dir,
            9,
            "branch_a_merge",
            vec![
                ("test_app", "'0008_branch_a_feature_1'"),
                ("test_app", "'0008_branch_a_feature_2'"),
            ],
        );

        // Create branch B migrations (8-9)
        create_test_migration_file(
            &migrations_dir,
            8,
            "branch_b_feature_1",
            vec![("test_app", "'0007_shared_feature_f'")],
        );
        create_test_migration_file(
            &migrations_dir,
            8,
            "branch_b_feature_2",
            vec![("test_app", "'0007_shared_feature_f'")],
        );
        create_test_migration_file(
            &migrations_dir,
            9,
            "branch_b_merge",
            vec![
                ("test_app", "'0008_branch_b_feature_1'"),
                ("test_app", "'0008_branch_b_feature_2'"),
            ],
        );

        // Create max_migration.txt showing conflict between HEAD merge migrations and rebased
        let max_migration_path = migrations_dir.join("max_migration.txt");
        let conflict_content = r#"<<<<<<< HEAD
0009_branch_a_merge.py
=======
0009_branch_b_merge.py
>>>>>>> feature-branch"#;
        fs::write(&max_migration_path, conflict_content)
            .expect("Failed to write max migration file");

        let result = fix(temp_dir.path().to_str().unwrap(), false, true);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Merge migration detected in rebased migration: 0009_branch_b_merge. Currently, merge migrations cannot be resolved properly when they are not part of the HEAD branch. In fact, you can use this tool to avoid merge migrations by rebasing your feature branch on the latest HEAD migration."
        );
    }

    #[test]
    fn test_rebase_with_multiple_head_and_feature_migrations() {
        let (temp_dir, migrations_dir) = setup_test_env();
        // Create a realistic rebase scenario with multiple migrations on both HEAD and feature branch
        // Timeline:
        //
        // 0001_initial
        //     │
        // 0002_add_user_model
        //     │
        // 0003_add_profile
        //     │
        //     ├─────────────────────┐ (branches diverge)
        //     │                     │
        //     │                     │ (feature branch)
        // 0004_add_posts            0004_add_comments
        //     │                     0005_add_likes
        // 0005_add_tags             0006_add_follows
        //     │
        // 0006_add_categories
        //     │ (HEAD)
        //
        // Expected after rebase:
        // - feature migrations should be renumbered to 0007, 0008, 0009
        // - dependencies on 0003 should remain unchanged
        // - max_migration.txt should be updated to 0009_add_follows

        // Create shared migrations (1-3)
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_user_model",
            vec![("test_app", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_dir,
            3,
            "add_profile",
            vec![("test_app", "'0002_add_user_model'")],
        );

        // Create HEAD migrations (4-6)
        create_test_migration_file(
            &migrations_dir,
            4,
            "add_posts",
            vec![("test_app", "'0003_add_profile'")],
        );
        create_test_migration_file(
            &migrations_dir,
            5,
            "add_tags",
            vec![("test_app", "'0004_add_posts'")],
        );
        create_test_migration_file(
            &migrations_dir,
            6,
            "add_categories",
            vec![("test_app", "'0005_add_tags'")],
        );

        // Create feature branch migrations (4-6) - these will be rebased
        create_test_migration_file(
            &migrations_dir,
            4,
            "add_comments",
            vec![("test_app", "'0003_add_profile'")],
        );
        create_test_migration_file(
            &migrations_dir,
            5,
            "add_likes",
            vec![("test_app", "'0004_add_comments'")],
        );
        create_test_migration_file(
            &migrations_dir,
            6,
            "add_follows",
            vec![("test_app", "'0005_add_likes'")],
        );

        // Create max_migration.txt showing conflict
        let max_migration_path = migrations_dir.join("max_migration.txt");
        let conflict_content = r#"<<<<<<< HEAD
0006_add_categories
=======
0006_add_follows
>>>>>>> feature-branch"#;
        fs::write(&max_migration_path, conflict_content)
            .expect("Failed to write max migration file");

        // Run the fix
        let result = fix(temp_dir.path().to_str().unwrap(), false, true);
        assert!(result.is_ok(), "Fix should succeed: {:?}", result.err());

        // Verify the migrations were renumbered correctly
        assert!(
            !migrations_dir.join("0004_add_comments.py").exists(),
            "Old 0004_add_comments should not exist"
        );
        assert!(
            !migrations_dir.join("0005_add_likes.py").exists(),
            "Old 0005_add_likes should not exist"
        );
        assert!(
            !migrations_dir.join("0006_add_follows.py").exists(),
            "Old 0006_add_follows should not exist"
        );

        assert!(
            migrations_dir.join("0007_add_comments.py").exists(),
            "New 0007_add_comments should exist"
        );
        assert!(
            migrations_dir.join("0008_add_likes.py").exists(),
            "New 0008_add_likes should exist"
        );
        assert!(
            migrations_dir.join("0009_add_follows.py").exists(),
            "New 0009_add_follows should exist"
        );

        // Verify HEAD migrations are unchanged
        assert!(
            migrations_dir.join("0004_add_posts.py").exists(),
            "HEAD migration 0004_add_posts should remain"
        );
        assert!(
            migrations_dir.join("0005_add_tags.py").exists(),
            "HEAD migration 0005_add_tags should remain"
        );
        assert!(
            migrations_dir.join("0006_add_categories.py").exists(),
            "HEAD migration 0006_add_categories should remain"
        );

        // Verify dependency updates in rebased migrations
        let comments_content = fs::read_to_string(migrations_dir.join("0007_add_comments.py"))
            .expect("Failed to read 0007_add_comments.py");
        assert!(
            comments_content.contains("'0006_add_categories'"),
            "0007_add_comments should depend on 0006_add_categories (last HEAD migration)"
        );

        let likes_content = fs::read_to_string(migrations_dir.join("0008_add_likes.py"))
            .expect("Failed to read 0008_add_likes.py");
        assert!(
            likes_content.contains("'0007_add_comments'"),
            "0008_add_likes should depend on 0007_add_comments"
        );

        let follows_content = fs::read_to_string(migrations_dir.join("0009_add_follows.py"))
            .expect("Failed to read 0009_add_follows.py");
        assert!(
            follows_content.contains("'0008_add_likes'"),
            "0009_add_follows should depend on 0008_add_likes"
        );

        // Verify max_migration.txt was updated
        let max_migration_content =
            fs::read_to_string(&max_migration_path).expect("Failed to read max_migration.txt");
        assert_eq!(
            max_migration_content.trim(),
            "0009_add_follows",
            "max_migration.txt should be updated to highest rebased migration"
        );
    }

    #[test]
    fn test_rebase_with_cross_app_dependencies() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let project_path = temp_dir.path();

        // Create a scenario with two apps where app_b depends on app_a migrations
        // Timeline:
        //
        // app_a:                      app_b:
        // 0001_initial                0001_initial
        //     │                           │
        // 0002_add_model                  │
        //     │                           │
        //     ├───────────────┐           ├───────────────┐
        //     │               │           │               │
        // 0003_add_field      │       0002_link_to_a      │
        //     │ (HEAD)        │       (depends on         │
        //                     │        app_a:0002)        │
        //                     │           │ (HEAD)        │
        //                     │                           │
        //           0003_update_model         0002_feature_b
        //           (feature branch)          (depends on app_a:0003
        //                                      and app_b:0001)
        //                                      (feature branch)
        //
        // Expected after rebase:
        // - app_a:0003_update_model → app_a:0004_update_model
        // - app_b:0002_feature_b → app_b:0003_feature_b
        // - app_b:0003_feature_b should depend on app_a:0004_update_model

        // Set up app_a
        let app_a_dir = project_path.join("app_a");
        let migrations_a_dir = app_a_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_a_dir).expect("Failed to create app_a migrations");

        create_test_migration_file(&migrations_a_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_a_dir,
            2,
            "add_model",
            vec![("app_a", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_a_dir,
            3,
            "add_field",
            vec![("app_a", "'0002_add_model'")],
        );
        create_test_migration_file(
            &migrations_a_dir,
            3,
            "update_model",
            vec![("app_a", "'0002_add_model'")],
        );

        let max_migration_a_path = migrations_a_dir.join("max_migration.txt");
        let conflict_a = r#"<<<<<<< HEAD
0003_add_field
=======
0003_update_model
>>>>>>> feature-branch"#;
        fs::write(&max_migration_a_path, conflict_a)
            .expect("Failed to write app_a max_migration.txt");

        // Set up app_b
        let app_b_dir = project_path.join("app_b");
        let migrations_b_dir = app_b_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_b_dir).expect("Failed to create app_b migrations");

        create_test_migration_file(&migrations_b_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_b_dir,
            2,
            "link_to_a",
            vec![("app_b", "'0001_initial'"), ("app_a", "'0002_add_model'")],
        );
        create_test_migration_file(
            &migrations_b_dir,
            2,
            "feature_b",
            vec![
                ("app_b", "'0001_initial'"),
                ("app_a", "'0003_update_model'"),
            ],
        );

        let max_migration_b_path = migrations_b_dir.join("max_migration.txt");
        let conflict_b = r#"<<<<<<< HEAD
0002_link_to_a
=======
0002_feature_b
>>>>>>> feature-branch"#;
        fs::write(&max_migration_b_path, conflict_b)
            .expect("Failed to write app_b max_migration.txt");

        // Run the fix
        let result = fix(project_path.to_str().unwrap(), false, true);
        assert!(result.is_ok(), "Fix should succeed: {:?}", result.err());

        // Verify app_a migrations were renumbered
        assert!(
            migrations_a_dir.join("0004_update_model.py").exists(),
            "app_a:0003_update_model should be renumbered to 0004"
        );
        assert!(
            !migrations_a_dir.join("0003_update_model.py").exists(),
            "app_a:0003_update_model should not exist anymore"
        );

        // Verify app_b migrations were renumbered
        assert!(
            migrations_b_dir.join("0003_feature_b.py").exists(),
            "app_b:0002_feature_b should be renumbered to 0003"
        );
        assert!(
            !migrations_b_dir.join("0002_feature_b.py").exists(),
            "app_b:0002_feature_b should not exist anymore"
        );

        // Verify cross-app dependency was updated in app_b
        let feature_b_content = fs::read_to_string(migrations_b_dir.join("0003_feature_b.py"))
            .expect("Failed to read 0003_feature_b.py");
        assert!(
            feature_b_content.contains("('app_a', '0004_update_model')"),
            "app_b:0003_feature_b should depend on app_a:0004_update_model, got:\n{}",
            feature_b_content
        );
        assert!(
            feature_b_content.contains("('app_b', '0002_link_to_a')"),
            "app_b:0003_feature_b should depend on app_b:0002_link_to_a (last HEAD migration)"
        );

        // Verify max_migration.txt files were updated
        let max_a_content = fs::read_to_string(&max_migration_a_path)
            .expect("Failed to read app_a max_migration.txt");
        assert_eq!(max_a_content.trim(), "0004_update_model");

        let max_b_content = fs::read_to_string(&max_migration_b_path)
            .expect("Failed to read app_b max_migration.txt");
        assert_eq!(max_b_content.trim(), "0003_feature_b");
    }

    #[test]
    fn test_rebase_single_head_multiple_rebased_with_cross_app() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let project_path = temp_dir.path();

        // Create a scenario where one app has 1 HEAD + 3 rebased migrations,
        // while another app has 2 HEAD + 1 rebased with bidirectional cross-app dependencies
        // Timeline:
        //
        // app_users:                       app_posts:
        // 0001_initial                     0001_initial
        //     │                                │
        //     ├──────────────┐             0002_add_post_model
        //     │              │                 │
        // 0002_add_field     │                 ├──────────────┐
        //     │ (HEAD)       │                 │              │
        //                    │             0003_add_likes     │
        //          0002_add_profile ◄──────┐   │ (HEAD)       │
        //          (depends on             |                  │
        //           users:0001)            |       0003_add_comments
        //                    │             |       (depends on
        //          0003_add_settings       └────── users:0002_add_profile
        //          (depends on                      and posts:0002)
        //           users:0002_add_profile)         (rebased)
        //                    │
        //          0004_add_preferences
        //          (depends on
        //           users:0003_add_settings
        //           and posts:0003_add_comments)
        //          (rebased)
        //
        // Expected after rebase:
        // - app_users: 0002_add_profile → 0003_add_profile
        //              0003_add_settings → 0004_add_settings
        //              0004_add_preferences → 0005_add_preferences
        // - app_posts: 0003_add_comments → 0004_add_comments
        // - app_posts:0004_add_comments should depend on app_users:0003_add_profile
        // - app_users:0005_add_preferences should depend on app_posts:0004_add_comments (bidirectional!)

        // Set up app_users
        let app_users_dir = project_path.join("app_users");
        let migrations_users_dir = app_users_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_users_dir).expect("Failed to create app_users migrations");

        create_test_migration_file(&migrations_users_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_users_dir,
            2,
            "add_field",
            vec![("app_users", "'0001_initial'")],
        );
        // Rebased migrations
        create_test_migration_file(
            &migrations_users_dir,
            2,
            "add_profile",
            vec![("app_users", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_users_dir,
            3,
            "add_settings",
            vec![("app_users", "'0002_add_profile'")],
        );
        create_test_migration_file(
            &migrations_users_dir,
            4,
            "add_preferences",
            vec![
                ("app_users", "'0003_add_settings'"),
                ("app_posts", "'0003_add_comments'"),
            ],
        );

        let max_migration_users_path = migrations_users_dir.join("max_migration.txt");
        let conflict_users = r#"<<<<<<< HEAD
0002_add_field
=======
0004_add_preferences
>>>>>>> feature-branch"#;
        fs::write(&max_migration_users_path, conflict_users)
            .expect("Failed to write app_users max_migration.txt");

        // Set up app_posts
        let app_posts_dir = project_path.join("app_posts");
        let migrations_posts_dir = app_posts_dir.join(MIGRATIONS);
        fs::create_dir_all(&migrations_posts_dir).expect("Failed to create app_posts migrations");

        create_test_migration_file(&migrations_posts_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_posts_dir,
            2,
            "add_post_model",
            vec![("app_posts", "'0001_initial'")],
        );
        create_test_migration_file(
            &migrations_posts_dir,
            3,
            "add_likes",
            vec![("app_posts", "'0002_add_post_model'")],
        );
        // Rebased migration with cross-app dependency
        create_test_migration_file(
            &migrations_posts_dir,
            3,
            "add_comments",
            vec![
                ("app_posts", "'0002_add_post_model'"),
                ("app_users", "'0002_add_profile'"),
            ],
        );

        let max_migration_posts_path = migrations_posts_dir.join("max_migration.txt");
        let conflict_posts = r#"<<<<<<< HEAD
0003_add_likes
=======
0003_add_comments
>>>>>>> feature-branch"#;
        fs::write(&max_migration_posts_path, conflict_posts)
            .expect("Failed to write app_posts max_migration.txt");

        // Run the fix
        let result = fix(project_path.to_str().unwrap(), false, true);
        assert!(result.is_ok(), "Fix should succeed: {:?}", result.err());

        // Verify app_users migrations were renumbered
        assert!(
            migrations_users_dir.join("0003_add_profile.py").exists(),
            "app_users:0002_add_profile should be renumbered to 0003"
        );
        assert!(
            migrations_users_dir.join("0004_add_settings.py").exists(),
            "app_users:0003_add_settings should be renumbered to 0004"
        );
        assert!(
            migrations_users_dir
                .join("0005_add_preferences.py")
                .exists(),
            "app_users:0004_add_preferences should be renumbered to 0005"
        );
        assert!(
            !migrations_users_dir.join("0002_add_profile.py").exists(),
            "Old app_users:0002_add_profile should not exist"
        );

        // Verify app_posts migration was renumbered
        assert!(
            migrations_posts_dir.join("0004_add_comments.py").exists(),
            "app_posts:0003_add_comments should be renumbered to 0004"
        );
        assert!(
            !migrations_posts_dir.join("0003_add_comments.py").exists(),
            "Old app_posts:0003_add_comments should not exist"
        );

        // Verify HEAD migrations remain unchanged
        assert!(
            migrations_users_dir.join("0002_add_field.py").exists(),
            "HEAD migration app_users:0002_add_field should remain"
        );
        assert!(
            migrations_posts_dir.join("0003_add_likes.py").exists(),
            "HEAD migration app_posts:0003_add_likes should remain"
        );

        // Verify dependencies in app_users rebased migrations
        let profile_content = fs::read_to_string(migrations_users_dir.join("0003_add_profile.py"))
            .expect("Failed to read 0003_add_profile.py");
        assert!(
            profile_content.contains("('app_users', '0002_add_field')"),
            "app_users:0003_add_profile should depend on 0002_add_field (last HEAD migration)"
        );

        let settings_content =
            fs::read_to_string(migrations_users_dir.join("0004_add_settings.py"))
                .expect("Failed to read 0004_add_settings.py");
        assert!(
            settings_content.contains("('app_users', '0003_add_profile')"),
            "app_users:0004_add_settings should depend on 0003_add_profile"
        );

        let preferences_content =
            fs::read_to_string(migrations_users_dir.join("0005_add_preferences.py"))
                .expect("Failed to read 0005_add_preferences.py");
        assert!(
            preferences_content.contains("('app_users', '0004_add_settings')"),
            "app_users:0005_add_preferences should depend on 0004_add_settings"
        );
        assert!(
            preferences_content.contains("('app_posts', '0004_add_comments')"),
            "app_users:0005_add_preferences should have updated bidirectional cross-app dependency to app_posts:0004_add_comments, got:\n{}",
            preferences_content
        );

        // Verify cross-app dependency in app_posts rebased migration
        let comments_content =
            fs::read_to_string(migrations_posts_dir.join("0004_add_comments.py"))
                .expect("Failed to read 0004_add_comments.py");
        assert!(
            comments_content.contains("('app_posts', '0003_add_likes')"),
            "app_posts:0004_add_comments should depend on app_posts:0003_add_likes (last HEAD migration)"
        );
        assert!(
            comments_content.contains("('app_users', '0003_add_profile')"),
            "app_posts:0004_add_comments should have updated cross-app dependency to app_users:0003_add_profile, got:\n{}",
            comments_content
        );

        // Verify max_migration.txt files were updated
        let max_users_content = fs::read_to_string(&max_migration_users_path)
            .expect("Failed to read app_users max_migration.txt");
        assert_eq!(max_users_content.trim(), "0005_add_preferences");

        let max_posts_content = fs::read_to_string(&max_migration_posts_path)
            .expect("Failed to read app_posts max_migration.txt");
        assert_eq!(max_posts_content.trim(), "0004_add_comments");
    }
}
