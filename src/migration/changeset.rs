use serde::Serialize;
use std::collections::HashMap;

use crate::migration::change::{MigrationDependencyChange, MigrationFileNameChange};
use crate::migration::file::{MaxMigrationResult, Migration, MigrationFileName};
use crate::migration::group::DjangoApp;
use crate::migration::project::DjangoProject;

#[derive(Debug, Clone, Serialize)]
pub struct MigrationChangeSet {
    pub migration_file_name: MigrationFileName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_rename: Option<MigrationFileNameChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_updates: Option<MigrationDependencyChange>,
}

impl TryFrom<&Migration> for MigrationChangeSet {
    type Error = String;

    fn try_from(migration: &Migration) -> Result<Self, Self::Error> {
        if migration.name_change.is_none() && migration.dependency_change.is_none() {
            return Err("Migration has no changes".to_string());
        }

        Ok(MigrationChangeSet {
            migration_file_name: migration.file_name.clone(),
            file_rename: migration.name_change.clone(),
            dependency_updates: migration.dependency_change.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MaxMigrationChangeSet {
    pub old: MigrationFileName,
    pub new: MigrationFileName,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppChangeSet {
    pub app_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_common_migration: Option<MigrationFileName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub migration_changes: Vec<MigrationChangeSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_migration_update: Option<MaxMigrationChangeSet>,
}

impl AppChangeSet {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectChangeSet {
    pub apps: HashMap<String, AppChangeSet>,
}

impl ProjectChangeSet {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e))
    }
}

impl From<&DjangoApp> for AppChangeSet {
    fn from(app: &DjangoApp) -> Self {
        let mut migration_changes = Vec::new();

        // Collect changes from head migrations
        for migration in app.head_migrations.values() {
            if let Ok(changeset) = MigrationChangeSet::try_from(migration) {
                migration_changes.push(changeset);
            }
        }

        // Collect changes from rebased migrations
        for migration in &app.rebased_migrations {
            if let Ok(changeset) = MigrationChangeSet::try_from(migration) {
                migration_changes.push(changeset);
            }
        }

        // Get max_migration.txt update
        let max_migration_update =
            if let MaxMigrationResult::Ok(max_file) = &app.max_migration_result {
                if let Some(new_content) = &max_file.new_content {
                    Some(MaxMigrationChangeSet {
                        old: max_file.current_content.clone(),
                        new: new_content.clone(),
                    })
                } else {
                    None
                }
            } else {
                None
            };

        AppChangeSet {
            app_name: app.get_app_name().to_string(),
            last_common_migration: app.last_common_migration.clone(),
            migration_changes,
            max_migration_update,
        }
    }
}

impl From<&DjangoProject> for ProjectChangeSet {
    fn from(project: &DjangoProject) -> Self {
        let mut app_changes = HashMap::new();

        for (app_name, app) in &project.apps {
            let changeset = AppChangeSet::from(app);
            app_changes.insert(app_name.clone(), changeset);
        }

        ProjectChangeSet { apps: app_changes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::file::{MaxMigrationFile, MigrationDependency};
    use crate::migration::test_helpers::create_in_memory_migration;
    use std::path::PathBuf;

    #[test]
    fn test_migration_changeset_try_from_with_both_changes() {
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
        );

        let dep_change = MigrationDependencyChange::new(
            vec![MigrationDependency {
                app: "app1".to_string(),
                migration_file: MigrationFileName::try_from("0001_old".to_string()).unwrap(),
            }],
            vec![MigrationDependency {
                app: "app1".to_string(),
                migration_file: MigrationFileName::try_from("0002_new".to_string()).unwrap(),
            }],
        );

        let migration = create_in_memory_migration(
            "0001_initial",
            "testapp",
            Some(name_change.clone()),
            Some(dep_change.clone()),
        );

        let result = MigrationChangeSet::try_from(&migration);
        assert!(result.is_ok());

        let changeset = result.unwrap();
        assert_eq!(changeset.migration_file_name.0, "0001_initial");
        assert!(changeset.file_rename.is_some());
        assert_eq!(changeset.file_rename.unwrap(), name_change);
        assert!(changeset.dependency_updates.is_some());
        assert_eq!(changeset.dependency_updates.unwrap(), dep_change);
    }

    #[test]
    fn test_migration_changeset_try_from_with_only_name_change() {
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
        );

        let migration =
            create_in_memory_migration("0001_initial", "testapp", Some(name_change.clone()), None);

        let result = MigrationChangeSet::try_from(&migration);
        assert!(result.is_ok());

        let changeset = result.unwrap();
        assert_eq!(changeset.migration_file_name.0, "0001_initial");
        assert!(changeset.file_rename.is_some());
        assert_eq!(changeset.file_rename.unwrap(), name_change);
        assert!(changeset.dependency_updates.is_none());
    }

    #[test]
    fn test_migration_changeset_try_from_with_only_dependency_change() {
        let dep_change = MigrationDependencyChange::new(
            vec![MigrationDependency {
                app: "app1".to_string(),
                migration_file: MigrationFileName::try_from("0001_old".to_string()).unwrap(),
            }],
            vec![MigrationDependency {
                app: "app1".to_string(),
                migration_file: MigrationFileName::try_from("0002_new".to_string()).unwrap(),
            }],
        );

        let migration =
            create_in_memory_migration("0001_initial", "testapp", None, Some(dep_change.clone()));

        let result = MigrationChangeSet::try_from(&migration);
        assert!(result.is_ok());

        let changeset = result.unwrap();
        assert_eq!(changeset.migration_file_name.0, "0001_initial");
        assert!(changeset.file_rename.is_none());
        assert!(changeset.dependency_updates.is_some());
        assert_eq!(changeset.dependency_updates.unwrap(), dep_change);
    }

    #[test]
    fn test_migration_changeset_try_from_no_changes() {
        let migration = create_in_memory_migration("0001_initial", "testapp", None, None);

        let result = MigrationChangeSet::try_from(&migration);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Migration has no changes");
    }

    #[test]
    fn test_app_changeset_to_json() {
        let changeset = AppChangeSet {
            app_name: "testapp".to_string(),
            last_common_migration: Some(
                MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            ),
            migration_changes: vec![],
            max_migration_update: None,
        };

        let result = changeset.to_json();
        assert!(result.is_ok());

        let json = result.unwrap();
        let expected = r#"{
  "app_name": "testapp",
  "last_common_migration": "0001_initial"
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_app_changeset_to_json_with_changes() {
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
        );

        let migration =
            create_in_memory_migration("0001_initial", "testapp", Some(name_change), None);
        let migration_changeset = MigrationChangeSet::try_from(&migration).unwrap();

        let max_migration_changeset = MaxMigrationChangeSet {
            old: MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            new: MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
        };

        let changeset = AppChangeSet {
            app_name: "testapp".to_string(),
            last_common_migration: Some(
                MigrationFileName::try_from("0001_base".to_string()).unwrap(),
            ),
            migration_changes: vec![migration_changeset],
            max_migration_update: Some(max_migration_changeset),
        };

        let result = changeset.to_json();
        assert!(result.is_ok());

        let json = result.unwrap();
        let expected = r#"{
  "app_name": "testapp",
  "last_common_migration": "0001_base",
  "migration_changes": [
    {
      "migration_file_name": "0001_initial",
      "file_rename": {
        "old_name": "0001_initial",
        "new_name": "0003_initial"
      }
    }
  ],
  "max_migration_update": {
    "old": "0001_initial",
    "new": "0003_initial"
  }
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_app_changeset_to_json_skips_empty_fields() {
        let changeset = AppChangeSet {
            app_name: "testapp".to_string(),
            last_common_migration: None,
            migration_changes: vec![],
            max_migration_update: None,
        };

        let result = changeset.to_json();
        assert!(result.is_ok());

        let json = result.unwrap();
        let expected = r#"{
  "app_name": "testapp"
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_project_changeset_to_json() {
        let app_changeset = AppChangeSet {
            app_name: "testapp".to_string(),
            last_common_migration: None,
            migration_changes: vec![],
            max_migration_update: None,
        };

        let mut apps = HashMap::new();
        apps.insert("testapp".to_string(), app_changeset);

        let project_changeset = ProjectChangeSet { apps };

        let result = project_changeset.to_json();
        assert!(result.is_ok());

        let json = result.unwrap();
        let expected = r#"{
  "apps": {
    "testapp": {
      "app_name": "testapp"
    }
  }
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_project_changeset_to_json_multiple_apps() {
        let app1 = AppChangeSet {
            app_name: "app1".to_string(),
            last_common_migration: Some(
                MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            ),
            migration_changes: vec![],
            max_migration_update: None,
        };

        let app2 = AppChangeSet {
            app_name: "app2".to_string(),
            last_common_migration: Some(
                MigrationFileName::try_from("0002_base".to_string()).unwrap(),
            ),
            migration_changes: vec![],
            max_migration_update: None,
        };

        let mut apps = HashMap::new();
        apps.insert("app1".to_string(), app1);
        apps.insert("app2".to_string(), app2);

        let project_changeset = ProjectChangeSet { apps };

        let result = project_changeset.to_json();
        assert!(result.is_ok());

        let json = result.unwrap();
        // HashMap ordering is not guaranteed, so we need to parse and verify both apps exist
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["apps"]["app1"].is_object());
        assert!(parsed["apps"]["app2"].is_object());
        assert_eq!(parsed["apps"]["app1"]["app_name"], "app1");
        assert_eq!(parsed["apps"]["app2"]["app_name"], "app2");
        assert_eq!(parsed["apps"]["app1"]["last_common_migration"], "0001_initial");
        assert_eq!(parsed["apps"]["app2"]["last_common_migration"], "0002_base");
    }

    #[test]
    fn test_migration_changeset_serialization() {
        let changeset = MigrationChangeSet {
            migration_file_name: MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            file_rename: Some(MigrationFileNameChange::new(
                MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
                MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
            )),
            dependency_updates: None,
        };

        let json = serde_json::to_string_pretty(&changeset).unwrap();
        let expected = r#"{
  "migration_file_name": "0001_initial",
  "file_rename": {
    "old_name": "0001_initial",
    "new_name": "0003_initial"
  }
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_max_migration_changeset_serialization() {
        let changeset = MaxMigrationChangeSet {
            old: MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            new: MigrationFileName::try_from("0005_updated".to_string()).unwrap(),
        };

        let json = serde_json::to_string_pretty(&changeset).unwrap();
        let expected = r#"{
  "old": "0001_initial",
  "new": "0005_updated"
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn test_app_changeset_from_django_app_empty() {
        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::None,
        };

        let changeset = AppChangeSet::from(&app);
        assert_eq!(changeset.app_name, "myapp");
        assert!(changeset.last_common_migration.is_none());
        assert!(changeset.migration_changes.is_empty());
        assert!(changeset.max_migration_update.is_none());
    }

    #[test]
    fn test_app_changeset_from_django_app_with_head_migrations() {
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            MigrationFileName::try_from("0003_initial".to_string()).unwrap(),
        );

        let migration =
            create_in_memory_migration("0001_initial", "myapp", Some(name_change), None);
        let migration_path = migration.file_path.clone();

        let mut head_migrations = HashMap::new();
        head_migrations.insert(migration_path, migration);

        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations,
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::None,
        };

        let changeset = AppChangeSet::from(&app);
        assert_eq!(changeset.app_name, "myapp");
        assert_eq!(changeset.migration_changes.len(), 1);
        assert_eq!(
            changeset.migration_changes[0].migration_file_name.0,
            "0001_initial"
        );
    }

    #[test]
    fn test_app_changeset_from_django_app_with_rebased_migrations() {
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0002_add_field".to_string()).unwrap(),
            MigrationFileName::try_from("0005_add_field".to_string()).unwrap(),
        );

        let migration =
            create_in_memory_migration("0002_add_field", "myapp", Some(name_change), None);

        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![migration],
            last_common_migration: Some(
                MigrationFileName::try_from("0001_base".to_string()).unwrap(),
            ),
            max_migration_result: MaxMigrationResult::None,
        };

        let changeset = AppChangeSet::from(&app);
        assert_eq!(changeset.app_name, "myapp");
        assert_eq!(changeset.migration_changes.len(), 1);
        assert_eq!(
            changeset.migration_changes[0].migration_file_name.0,
            "0002_add_field"
        );
        assert_eq!(changeset.last_common_migration.unwrap().0, "0001_base");
    }

    #[test]
    fn test_app_changeset_from_django_app_with_max_migration_update() {
        let max_file = MaxMigrationFile {
            current_content: MigrationFileName::try_from("0003_old".to_string()).unwrap(),
            new_content: Some(MigrationFileName::try_from("0007_new".to_string()).unwrap()),
        };

        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::Ok(max_file),
        };

        let changeset = AppChangeSet::from(&app);
        assert_eq!(changeset.app_name, "myapp");
        assert!(changeset.max_migration_update.is_some());
        let max_update = changeset.max_migration_update.unwrap();
        assert_eq!(max_update.old.0, "0003_old");
        assert_eq!(max_update.new.0, "0007_new");
    }

    #[test]
    fn test_app_changeset_from_django_app_max_migration_no_new_content() {
        let max_file = MaxMigrationFile {
            current_content: MigrationFileName::try_from("0003_current".to_string()).unwrap(),
            new_content: None,
        };

        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::Ok(max_file),
        };

        let changeset = AppChangeSet::from(&app);
        assert!(changeset.max_migration_update.is_none());
    }

    #[test]
    fn test_app_changeset_from_django_app_ignores_migrations_without_changes() {
        // Migration with no changes
        let migration_no_changes = create_in_memory_migration("0001_initial", "myapp", None, None);
        let path_no_changes = migration_no_changes.file_path.clone();

        // Migration with changes
        let name_change = MigrationFileNameChange::new(
            MigrationFileName::try_from("0002_add_field".to_string()).unwrap(),
            MigrationFileName::try_from("0004_add_field".to_string()).unwrap(),
        );
        let migration_with_changes =
            create_in_memory_migration("0002_add_field", "myapp", Some(name_change), None);
        let path_with_changes = migration_with_changes.file_path.clone();

        let mut head_migrations = HashMap::new();
        head_migrations.insert(path_no_changes, migration_no_changes);
        head_migrations.insert(path_with_changes, migration_with_changes);

        let app = DjangoApp {
            directory: PathBuf::from("/test/myapp/migrations"),
            head_migrations,
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::None,
        };

        let changeset = AppChangeSet::from(&app);
        // Only the migration with changes should be included
        assert_eq!(changeset.migration_changes.len(), 1);
        assert_eq!(
            changeset.migration_changes[0].migration_file_name.0,
            "0002_add_field"
        );
    }

    #[test]
    fn test_project_changeset_from_django_project_empty() {
        let project = DjangoProject {
            apps: HashMap::new(),
        };

        let changeset = ProjectChangeSet::from(&project);
        assert!(changeset.apps.is_empty());
    }

    #[test]
    fn test_project_changeset_from_django_project_multiple_apps() {
        let app1 = DjangoApp {
            directory: PathBuf::from("/test/app1/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![],
            last_common_migration: Some(
                MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
            ),
            max_migration_result: MaxMigrationResult::None,
        };

        let app2 = DjangoApp {
            directory: PathBuf::from("/test/app2/migrations"),
            head_migrations: HashMap::new(),
            rebased_migrations: vec![],
            last_common_migration: Some(
                MigrationFileName::try_from("0002_base".to_string()).unwrap(),
            ),
            max_migration_result: MaxMigrationResult::None,
        };

        let mut apps = HashMap::new();
        apps.insert("app1".to_string(), app1);
        apps.insert("app2".to_string(), app2);

        let project = DjangoProject { apps };

        let changeset = ProjectChangeSet::from(&project);
        assert_eq!(changeset.apps.len(), 2);
        assert!(changeset.apps.contains_key("app1"));
        assert!(changeset.apps.contains_key("app2"));
        assert_eq!(changeset.apps.get("app1").unwrap().app_name, "app1");
        assert_eq!(changeset.apps.get("app2").unwrap().app_name, "app2");
    }
}
