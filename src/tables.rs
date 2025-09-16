use crate::rebase::{Migration, MigrationGroup};
use cli_table::{Cell, Color, Style, Table};
use std::collections::HashMap;

pub enum TableOptions<'a> {
    Summary(&'a HashMap<String, MigrationGroup>),
    MigrationChanges(&'a str, &'a HashMap<std::path::PathBuf, Migration>),
    MaxMigrationChanges(&'a HashMap<String, MigrationGroup>),
}

pub fn get_table(options: TableOptions<'_>) -> cli_table::TableStruct {
    match options {
        TableOptions::Summary(groups) => get_summary_table(groups),
        TableOptions::MigrationChanges(app_name, migrations) => {
            get_migration_changes_table(app_name, migrations)
        }
        TableOptions::MaxMigrationChanges(groups) => get_max_migration_changes_table(groups),
    }
}

fn get_summary_table(groups: &HashMap<String, MigrationGroup>) -> cli_table::TableStruct {
    let table = groups
        .values()
        .map(|group| {
            let app_name = group.get_app_name();
            let total_migrations = group.migrations.len();

            let file_renames = group
                .migrations
                .values()
                .filter(|m| m.name_change.is_some())
                .count();

            let dependency_updates = group
                .migrations
                .values()
                .filter(|m| m.dependency_change.is_some())
                .count();

            let max_migration_update = group
                .max_migration_file
                .as_ref()
                .and_then(|f| f.new_content.as_ref())
                .map(|_| "Yes")
                .unwrap_or("No");

            vec![
                app_name.cell().bold(true),
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
        ]);
    table
}

fn get_migration_changes_table(
    app_name: &str,
    migrations: &HashMap<std::path::PathBuf, Migration>,
) -> cli_table::TableStruct {
    let table = migrations
        .values()
        .filter(|migration| {
            migration.name_change.is_some() || migration.dependency_change.is_some()
        })
        .map(|migration| {
            let migration_name = &migration.file_name.0;
            let file_changes = migration
                .name_change
                .as_ref()
                .map(|change| change.to_string())
                .unwrap_or_else(|| "No changes".to_string());

            let dependency_changes = migration
                .dependency_change
                .as_ref()
                .map(|change| change.to_string())
                .unwrap_or_else(|| "No changes".to_string());

            let has_name_change = migration.name_change.is_some();
            let has_dependency_change = migration.dependency_change.is_some();

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
            format!("Migration ({})", app_name)
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
        ]);
    table
}

fn get_max_migration_changes_table(
    groups: &HashMap<String, MigrationGroup>,
) -> cli_table::TableStruct {
    let table = groups
        .values()
        .filter_map(|group| {
            group.max_migration_file.as_ref().and_then(|max_file| {
                max_file.new_content.as_ref().map(|new_content| {
                    vec![
                        group.get_app_name().cell().bold(true),
                        max_file
                            .current_content
                            .0
                            .clone()
                            .cell()
                            .foreground_color(Some(Color::Red)),
                        new_content
                            .0
                            .clone()
                            .cell()
                            .foreground_color(Some(Color::Green)),
                    ]
                })
            })
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
        ]);
    table
}
