use crate::migration::changeset::AppChangeSet;
use crate::migration::group::DjangoApp;
use cli_table::{Cell, Color, Style, Table};
use std::collections::HashMap;

pub enum TableOptions<'a> {
    Summary(&'a HashMap<String, DjangoApp>),
    SingleAppSummary(&'a str, &'a DjangoApp),
    MigrationChanges(&'a str, &'a DjangoApp),
    MaxMigrationChanges(&'a HashMap<String, DjangoApp>),
    SingleAppMaxMigrationChanges(&'a str, &'a DjangoApp),
}

pub fn get_table(options: TableOptions<'_>) -> cli_table::TableStruct {
    match options {
        TableOptions::Summary(apps) => get_summary_table(apps),
        TableOptions::SingleAppSummary(app_name, app) => {
            get_single_app_summary_table(app_name, app)
        }
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

            let total_migrations = group.head_migrations.len() + group.rebased_migrations.len();

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

fn get_single_app_summary_table(_app_name: &str, app: &DjangoApp) -> cli_table::TableStruct {
    let changeset = AppChangeSet::from(app);

    let total_migrations = app.head_migrations.len() + app.rebased_migrations.len();

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

    vec![vec![
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
    ]]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::change::MigrationFileNameChange;
    use crate::migration::file::{MaxMigrationResult, MigrationFileName};
    use crate::migration::test_helpers::create_in_memory_migration;
    use std::path::PathBuf;

    #[test]
    fn test_total_migrations_count_includes_all_migrations() {
        // Create a scenario where we have 5 head migrations + 2 rebased migrations = 7 total
        // But only 2 of them have changes (1 file rename, 1 dependency update)
        // The "Total Migrations" count should show 7.

        let mut head_migrations = HashMap::new();

        // Create 5 head migrations, only 1 has changes
        let migration_with_change = create_in_memory_migration(
            "0001_initial",
            "testapp",
            Some(MigrationFileNameChange::new(
                MigrationFileName::try_from("0001_initial".to_string()).unwrap(),
                MigrationFileName::try_from("0005_initial".to_string()).unwrap(),
            )),
            None,
        );
        head_migrations.insert(
            migration_with_change.file_path.clone(),
            migration_with_change,
        );

        // Create 4 more head migrations without changes
        for i in 2..=5 {
            let migration = create_in_memory_migration(
                &format!("000{}_migration_{}", i, i),
                "testapp",
                None,
                None,
            );
            head_migrations.insert(migration.file_path.clone(), migration);
        }

        // Create 2 rebased migrations, only 1 has changes
        let rebased_with_change = create_in_memory_migration(
            "0006_rebased",
            "testapp",
            Some(MigrationFileNameChange::new(
                MigrationFileName::try_from("0006_rebased".to_string()).unwrap(),
                MigrationFileName::try_from("0010_rebased".to_string()).unwrap(),
            )),
            None,
        );

        let rebased_without_change =
            create_in_memory_migration("0007_rebased_2", "testapp", None, None);

        let rebased_migrations = vec![rebased_with_change, rebased_without_change];

        let app = DjangoApp {
            directory: PathBuf::from("test/testapp/migrations"),
            head_migrations,
            rebased_migrations,
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::None,
        };

        // Create the changeset
        let changeset = AppChangeSet::from(&app);

        // Verify that only 2 migrations appear in migration_changes (those with actual changes)
        assert_eq!(
            changeset.migration_changes.len(),
            2,
            "Only migrations with changes should appear in migration_changes"
        );

        // Verify the CORRECT total count includes all migrations
        let total_migrations = app.head_migrations.len() + app.rebased_migrations.len();
        assert_eq!(
            total_migrations, 7,
            "Total migrations should be 5 head + 2 rebased = 7"
        );

        // Verify the calculation matches what we use in the table
        let file_renames = changeset
            .migration_changes
            .iter()
            .filter(|m| m.file_rename.is_some())
            .count();
        assert_eq!(file_renames, 2, "Should have 2 file renames");

        // Test the single app summary table and verify the output
        let table = get_single_app_summary_table("testapp", &app);
        let table_output = table.display().unwrap().to_string();

        // The table should contain "7" in the Total Migrations column
        assert!(
            table_output.contains("7"),
            "Table should show total migrations count of 7, got:\n{}",
            table_output
        );
        // Verify it shows the correct app name
        assert!(
            table_output.contains("testapp"),
            "Table should show app name 'testapp'"
        );

        // Test the multi-app summary table and verify the output
        let mut apps = HashMap::new();
        apps.insert("testapp".to_string(), app);
        let table = get_summary_table(&apps);
        let table_output = table.display().unwrap().to_string();

        // The table should contain "7" in the Total Migrations column
        assert!(
            table_output.contains("7"),
            "Multi-app table should show total migrations count of 7, got:\n{}",
            table_output
        );
        assert!(
            table_output.contains("testapp"),
            "Multi-app table should show app name 'testapp'"
        );
    }

    #[test]
    fn test_total_migrations_count_with_no_changes() {
        // Edge case: All migrations exist but none have changes
        let mut head_migrations = HashMap::new();

        for i in 1..=3 {
            let migration =
                create_in_memory_migration(&format!("000{}_migration", i), "testapp", None, None);
            head_migrations.insert(migration.file_path.clone(), migration);
        }

        let app = DjangoApp {
            directory: PathBuf::from("test/testapp/migrations"),
            head_migrations,
            rebased_migrations: vec![],
            last_common_migration: None,
            max_migration_result: MaxMigrationResult::None,
        };

        let changeset = AppChangeSet::from(&app);

        // No migrations with changes
        assert_eq!(
            changeset.migration_changes.len(),
            0,
            "No migrations should have changes"
        );

        // But total should still be 3
        let total_migrations = app.head_migrations.len() + app.rebased_migrations.len();
        assert_eq!(total_migrations, 3, "Total should be 3 migrations");

        // Test single app table and verify the output shows "3"
        let table = get_single_app_summary_table("testapp", &app);
        let table_output = table.display().unwrap().to_string();
        assert!(
            table_output.contains("3"),
            "Table should show total migrations count of 3, got:\n{}",
            table_output
        );

        // Test multi-app table and verify the output shows "3"
        let mut apps = HashMap::new();
        apps.insert("testapp".to_string(), app);
        let table = get_summary_table(&apps);
        let table_output = table.display().unwrap().to_string();
        assert!(
            table_output.contains("3"),
            "Multi-app table should show total migrations count of 3, got:\n{}",
            table_output
        );
    }
}
