use serde::Serialize;
use std::collections::HashMap;

use crate::migration::change::{MigrationDependencyChange, MigrationFileNameChange};
use crate::migration::file::MaxMigrationResult;
use crate::migration::group::DjangoApp;
use crate::migration::project::DjangoProject;

#[derive(Debug, Serialize)]
pub struct JsonMigrationChange {
    pub migration: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_rename: Option<MigrationFileNameChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_updates: Option<MigrationDependencyChange>,
}

#[derive(Debug, Serialize)]
pub struct JsonMaxMigrationChange {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
pub struct JsonAppChanges {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub migration_changes: Vec<JsonMigrationChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_migration_update: Option<JsonMaxMigrationChange>,
}

#[derive(Debug, Serialize)]
pub struct JsonProjectChanges {
    pub apps: HashMap<String, JsonAppChanges>,
}

impl TryFrom<&DjangoApp> for JsonAppChanges {
    type Error = String;

    fn try_from(app: &DjangoApp) -> Result<Self, Self::Error> {
        let mut migration_changes = Vec::new();

        // Collect changes from head migrations
        for migration in app.head_migrations.values() {
            if migration.name_change.is_some() || migration.dependency_change.is_some() {
                migration_changes.push(JsonMigrationChange {
                    migration: migration.file_name.0.clone(),
                    file_rename: migration.name_change.clone(),
                    dependency_updates: migration.dependency_change.clone(),
                });
            }
        }

        // Collect changes from rebased migrations
        for migration in &app.rebased_migrations {
            if migration.name_change.is_some() || migration.dependency_change.is_some() {
                migration_changes.push(JsonMigrationChange {
                    migration: migration.file_name.0.clone(),
                    file_rename: migration.name_change.clone(),
                    dependency_updates: migration.dependency_change.clone(),
                });
            }
        }

        // Get max_migration.txt update
        let max_migration_update = if let MaxMigrationResult::Ok(max_file) =
            &app.max_migration_result
        {
            if let Some(new_content) = &max_file.new_content {
                Some(JsonMaxMigrationChange {
                    old: max_file.current_content.0.clone(),
                    new: new_content.0.clone(),
                })
            } else {
                None
            }
        } else {
            None
        };

        Ok(JsonAppChanges {
            migration_changes,
            max_migration_update,
        })
    }
}

impl JsonAppChanges {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("Failed to serialize to JSON: {}", e))
    }
}

impl TryFrom<&DjangoProject> for JsonProjectChanges {
    type Error = String;

    fn try_from(project: &DjangoProject) -> Result<Self, Self::Error> {
        let mut json_apps = HashMap::new();

        for (app_name, app) in &project.apps {
            let app_changes = JsonAppChanges::try_from(app)?;
            // Only include apps that have changes
            if !app_changes.migration_changes.is_empty()
                || app_changes.max_migration_update.is_some()
            {
                json_apps.insert(app_name.clone(), app_changes);
            }
        }

        Ok(JsonProjectChanges { apps: json_apps })
    }
}

impl JsonProjectChanges {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("Failed to serialize to JSON: {}", e))
    }
}
