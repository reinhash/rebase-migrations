use crate::migration::changeset::AppChangeSet;
use crate::migration::group::DjangoApp;
use cli_table::{Cell, Color, Style, Table};
use std::collections::HashMap;

pub enum TableOptions<'a> {
    Summary(&'a HashMap<String, DjangoApp>),
    MigrationChanges(&'a str, &'a DjangoApp),
    MaxMigrationChanges(&'a HashMap<String, DjangoApp>),
    SingleAppMaxMigrationChanges(&'a str, &'a DjangoApp),
}

pub fn get_table(options: TableOptions<'_>) -> cli_table::TableStruct {
    match options {
        TableOptions::Summary(apps) => get_summary_table(apps),
        TableOptions::MigrationChanges(_app_name, app) => {
            let changeset = AppChangeSet::from(app);
            get_migration_changes_table_from_changeset(&changeset)
        }
        TableOptions::MaxMigrationChanges(apps) => get_max_migration_changes_table(apps),
        TableOptions::SingleAppMaxMigrationChanges(app_name, app) => {
            get_single_app_max_migration_changes_table(app_name, app)
        }
    }
}

fn get_summary_table(groups: &HashMap<String, DjangoApp>) -> cli_table::TableStruct {
    groups
        .values()
        .map(|group| {
            let changeset = AppChangeSet::from(group);

            let total_migrations = changeset.migration_changes.len();

            let file_renames = changeset
                .migration_changes
                .iter()
                .filter(|m| m.file_rename.is_some())
                .count();

            let dependency_updates = changeset
                .migration_changes
                .iter()
                .filter(|m| m.dependency_updates.is_some())
                .count();

            let max_migration_update = if changeset.max_migration_update.is_some() {
                "Yes"
            } else {
                "No"
            };

            vec![
                changeset.app_name.cell().bold(true),
                total_migrations.cell(),
                file_renames.cell().foreground_color(if file_renames > 0 {
                    Some(Color::Blue)
                } else {
                    None
                }),
                dependency_updates
                    .cell()
                    .foreground_color(if dependency_updates > 0 {
                        Some(Color::Magenta)
                    } else {
                        None
                    }),
                max_migration_update
                    .cell()
                    .foreground_color(if max_migration_update == "Yes" {
                        Some(Color::Green)
                    } else {
                        None
                    }),
            ]
        })
        .collect::<Vec<_>>()
        .table()
        .title(vec![
            "App".cell().bold(true).foreground_color(Some(Color::Cyan)),
            "Total Migrations".cell().bold(true),
            "File Renames"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Blue)),
            "Dependency Updates"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Magenta)),
            "max_migration.txt Updated"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Green)),
        ])
}

fn get_max_migration_changes_table(apps: &HashMap<String, DjangoApp>) -> cli_table::TableStruct {
    apps.values()
        .filter_map(|app| {
            let changeset = AppChangeSet::from(app);
            if let Some(max_change) = changeset.max_migration_update {
                Some(vec![
                    changeset.app_name.cell().bold(true),
                    max_change
                        .old
                        .0
                        .clone()
                        .cell()
                        .foreground_color(Some(Color::Red)),
                    max_change
                        .new
                        .0
                        .clone()
                        .cell()
                        .foreground_color(Some(Color::Green)),
                ])
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .table()
        .title(vec![
            "App".cell().bold(true).foreground_color(Some(Color::Cyan)),
            "Current max_migration.txt"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Red)),
            "New max_migration.txt"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Green)),
        ])
}

fn get_single_app_max_migration_changes_table(
    _app_name: &str,
    app: &DjangoApp,
) -> cli_table::TableStruct {
    let changeset = AppChangeSet::from(app);
    get_single_app_max_migration_changes_table_from_changeset(&changeset)
}

// Table functions using ChangeSet

fn get_migration_changes_table_from_changeset(changeset: &AppChangeSet) -> cli_table::TableStruct {
    changeset
        .migration_changes
        .iter()
        .map(|migration_change| {
            let migration_name = &migration_change.migration_file_name.0;
            let file_changes = migration_change
                .file_rename
                .as_ref()
                .map(|change| change.to_string())
                .unwrap_or_else(|| "No changes".to_string());

            let dependency_changes = migration_change
                .dependency_updates
                .as_ref()
                .map(|change| change.to_string())
                .unwrap_or_else(|| "No changes".to_string());

            let has_name_change = migration_change.file_rename.is_some();
            let has_dependency_change = migration_change.dependency_updates.is_some();

            vec![
                migration_name.cell().bold(true),
                file_changes.cell().foreground_color(if has_name_change {
                    Some(Color::Blue)
                } else {
                    Some(Color::Black)
                }),
                dependency_changes
                    .cell()
                    .foreground_color(if has_dependency_change {
                        Some(Color::Magenta)
                    } else {
                        Some(Color::Black)
                    }),
            ]
        })
        .collect::<Vec<_>>()
        .table()
        .title(vec![
            format!("Migration ({})", changeset.app_name)
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Cyan)),
            "File Name Changes"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Blue)),
            "Dependency Changes"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Magenta)),
        ])
}

fn get_single_app_max_migration_changes_table_from_changeset(
    changeset: &AppChangeSet,
) -> cli_table::TableStruct {
    if let Some(max_change) = &changeset.max_migration_update {
        vec![vec![
            changeset.app_name.clone().cell().bold(true),
            max_change
                .old
                .0
                .clone()
                .cell()
                .foreground_color(Some(Color::Red)),
            max_change
                .new
                .0
                .clone()
                .cell()
                .foreground_color(Some(Color::Green)),
        ]]
        .table()
        .title(vec![
            "App".cell().bold(true).foreground_color(Some(Color::Cyan)),
            "Current max_migration.txt"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Red)),
            "New max_migration.txt"
                .cell()
                .bold(true)
                .foreground_color(Some(Color::Green)),
        ])
    } else {
        // Return an empty table if there are no changes
        vec![vec!["No changes".cell()]]
            .table()
            .title(vec!["max_migration.txt".cell().bold(true)])
    }
}
