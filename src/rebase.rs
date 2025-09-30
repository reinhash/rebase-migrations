use rustpython_parser::{Parse, ast};
use std::collections::HashSet;
use std::fmt::Display;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

use crate::tables::{TableOptions, get_table};
use crate::utils::{MergeConflict, replace_range_in_file};

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

#[derive(Debug)]
struct DjangoProject {
    apps: HashMap<String, MigrationGroup>,
}

impl DjangoProject {
    fn from_path(repo_path: &Path, all_dirs: bool) -> Result<Self, String> {
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

            if path.is_dir() && path.file_name() == Some(std::ffi::OsStr::new("migrations")) {
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
                .migrations
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
                .migrations
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
                .migrations
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

pub struct MigrationParser {
    file_path: PathBuf,
    ast: ast::Suite,
}

impl MigrationParser {
    fn new(python_path: &PathBuf) -> Result<Self, String> {
        let python_source = std::fs::read_to_string(python_path)
            .map_err(|e| format!("Failed to read file {}: {}", python_path.display(), e))?;

        let ast = ast::Suite::parse(
            &python_source,
            python_path
                .to_str()
                .expect("Failed to convert path to string"),
        )
        .map_err(|e| format!("Failed to parse python statements: {e}"))?;

        Ok(Self {
            file_path: python_path.clone(),
            ast,
        })
    }

    fn find_dependency_location(&self) -> Result<(u32, u32), String> {
        let migration_class = self.find_migration_class()?;
        let dependencies_assignment = self.find_dependencies_assignment(migration_class)?;
        let start = u32::from(dependencies_assignment.range.start());
        let end = u32::from(dependencies_assignment.range.end());
        Ok((start, end))
    }

    fn find_migration_class(&self) -> Result<&ast::StmtClassDef, String> {
        for statement in &self.ast {
            if let ast::Stmt::ClassDef(class) = statement {
                if &class.name.to_string() == "Migration" {
                    return Ok(class);
                }
            }
        }
        Err(format!(
            "Migration class not found in file {}",
            self.file_path.display()
        ))
    }

    fn find_dependencies_assignment<'a>(
        &self,
        migration_class: &'a ast::StmtClassDef,
    ) -> Result<&'a ast::StmtAssign, String> {
        for item in &migration_class.body {
            if let ast::Stmt::Assign(assign) = item {
                if self.is_dependencies_assignment(assign) {
                    return Ok(assign);
                }
            }
        }
        Err(format!(
            "Dependencies assignment not found in Migration class in file {}",
            self.file_path.display()
        ))
    }

    fn is_dependencies_assignment(&self, assign: &ast::StmtAssign) -> bool {
        assign
            .targets
            .iter()
            .any(|target| matches!(target, ast::Expr::Name(name) if &name.id == "dependencies"))
    }

    fn extract_dependency_tuples<'a>(
        &self,
        assignment: &'a ast::StmtAssign,
    ) -> Result<&'a Vec<ast::Expr>, String> {
        match assignment.value.as_ref() {
            ast::Expr::List(dep_list) => Ok(&dep_list.elts),
            _ => Err(format!(
                "Dependencies should be a list in file {}",
                self.file_path.display()
            )),
        }
    }

    fn get_dependencies(&self) -> Vec<MigrationDependency> {
        let empty_vec: Vec<MigrationDependency> = Vec::new();
        let migration_class = match self.find_migration_class() {
            Ok(class) => class,
            Err(_) => return empty_vec,
        };
        let dependencies_assignment = match self.find_dependencies_assignment(migration_class) {
            Ok(assignment) => assignment,
            Err(_) => return empty_vec,
        };
        let dependency_tuples = match self.extract_dependency_tuples(dependencies_assignment) {
            Ok(tuples) => tuples,
            Err(_) => return empty_vec,
        };

        let mut result = Vec::new();

        for expr in dependency_tuples {
            if let Ok(dependency) = MigrationDependency::try_from(expr) {
                result.push(dependency);
            }
        }
        result
    }
}

/// Always starts with 4 digits, then an underscore, then the name
/// e.g. 0001_initial.py
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
    fn from_number_and_name(number: u32, name: &str) -> Self {
        Self::try_from(format!("{:04}_{}", number, name)).unwrap()
    }

    fn name(&self) -> String {
        self.0.splitn(2, '_').nth(1).unwrap().to_string()
    }

    fn number(&self) -> u32 {
        self.0
            .splitn(2, '_')
            .next()
            .expect("we validate on create, this cannot fail")
            .parse()
            .expect("we validate on create, this cannot fail")
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFileNameChange {
    pub old_name: MigrationFileName,
    pub new_name: MigrationFileName,
}

impl MigrationFileNameChange {
    fn new(old_name: MigrationFileName, new_name: MigrationFileName) -> Self {
        Self { old_name, new_name }
    }

    fn apply_change(&self, migrations_dir: &PathBuf) -> Result<(), String> {
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
    fn new(
        old_dependencies: Vec<MigrationDependency>,
        new_dependencies: Vec<MigrationDependency>,
    ) -> Self {
        Self {
            old_dependencies,
            new_dependencies,
        }
    }

    fn apply_change(&self, migration_path: &PathBuf) -> Result<(), String> {
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
        let mut migration_stack = Vec::new();
        migration_stack.push(initial_migration);

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
            Err(format!("Migration is of another app."))
        }
    }
}

#[derive(Debug)]
struct MigrationIterator {
    dependency_iterator: MigrationDependencyIterator,
}

impl Iterator for MigrationIterator {
    type Item = Migration;

    fn next(&mut self) -> Option<Self::Item> {
        self.dependency_iterator.next()
    }
}

#[derive(Debug, Clone)]
pub struct Migration {
    pub file_name: MigrationFileName,
    file_path: PathBuf,
    app_name: String,
    dependencies: Vec<MigrationDependency>,
    from_rebased_branch: bool,
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
    fn iter(&self) -> MigrationIterator {
        MigrationIterator {
            dependency_iterator: MigrationDependencyIterator::new(self.clone()),
        }
    }

    fn new_full_path(&self, directory: &Path) -> Option<PathBuf> {
        let name_change = self.name_change.clone()?;
        let new_path = directory.join(name_change.new_name.0);
        Some(new_path.with_extension("py"))
    }

    /// Check that no merge migration exists in one of the rebased migrations.
    fn is_merge_migration(&self) -> Result<(), String> {
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
pub struct MigrationGroup {
    pub migrations: HashMap<PathBuf, Migration>,
    pub directory: PathBuf,
    pub last_common_migration: Option<MigrationFileName>,
    pub max_migration_result: MaxMigrationResult,
    pub rebased_migrations: Vec<Migration>,
}

#[derive(Debug)]
pub enum MaxMigrationResult {
    Ok(MaxMigrationFile),
    Conflict(MergeConflict),
    None,
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
    fn apply_change(&self, migrations_dir: &PathBuf) -> Result<(), String> {
        if let Some(new_content) = &self.new_content {
            let max_migration_path = migrations_dir.join("max_migration").with_extension("txt");
            let content = format!("{}\n", new_content.0);
            std::fs::write(&max_migration_path, content)
                .map_err(|e| format!("Failed to write max migration file: {e}"))?;
        }
        Ok(())
    }
}

impl MigrationGroup {
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
    fn create_migration_dependency_changes(
        &mut self,
        same_app: bool,
        lookup: &HashMap<String, Vec<MigrationFileNameChange>>,
    ) {
        // if same_app is true, check all rebased migrations dependencies if there are affected.
        // - if the migration has the dependency of self.last_common_migration it needs to be set to the last head migration
        // - for all other rebased migrations, set their dependencies based on the lookup
        let app_name = self.get_app_name().to_string();
        let head_migration = self.find_highest_migration(true).cloned();

        // Combine both migration collections for processing
        let mut all_migrations: Vec<&mut Migration> = self.migrations.values_mut().collect();
        all_migrations.extend(self.rebased_migrations.iter_mut());

        for migration in all_migrations {
            // same app and rebased migration
            if same_app == true && migration.from_rebased_branch {
                let mut updated_dependencies = migration.dependencies.clone();
                let mut has_changes = false;

                for (i, dependency) in migration.dependencies.iter().enumerate() {
                    if dependency.app == app_name {
                        // 1. Case: the migration depends on the last common migration
                        if let Some(common_migration) = &self.last_common_migration {
                            if dependency.migration_file == *common_migration {
                                updated_dependencies[i] = MigrationDependency {
                                    app: app_name.clone(),
                                    migration_file: head_migration
                                        .as_ref()
                                        .expect("We must have a head migration here")
                                        .file_name
                                        .clone(),
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

    fn create_migration_name_changes(&mut self, conflict: MergeConflict) {
        if self.last_common_migration.is_none() {
            return;
        }
        // Find the highest migration number from head (non-rebased) migrations
        let highest_head_number = self
            .migrations
            .values()
            .map(|m| m.file_name.number())
            .max()
            .unwrap();

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

    fn set_last_common_migration(
        &mut self,
        max_rebased_migration: MigrationFileName,
    ) -> Result<(), String> {
        let rebased_migration_path = self
            .directory
            .join(max_rebased_migration.0)
            .with_extension("py");
        let rebased_migration = Migration::try_from(rebased_migration_path).unwrap();
        for migration in rebased_migration.iter() {
            if self.migrations.contains_key(&migration.file_path) {
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
        if head_only {
            return self
                .migrations
                .values()
                .filter(|m| !m.from_rebased_branch)
                .map(|m| m.file_name.number())
                .max();
        }
        return self.migrations.values().map(|m| m.file_name.number()).max();
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
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No migrations are found that match the filtering criteria
    /// - Multiple migrations exist with the same highest number (indicates corruption)
    fn find_highest_migration(&self, head_only: bool) -> Result<&Migration, String> {
        // return an Error if there is more than one migration with the highest number.
        // Otherwise there should only be one migration. Return it.
        let highest_number = self
            .find_highest_migration_number(head_only)
            .ok_or_else(|| "No migrations found".to_string())?;
        let migrations_with_highest_number: Vec<&Migration> = self
            .migrations
            .values()
            .filter(|m| m.file_name.number() == highest_number)
            .collect();
        if migrations_with_highest_number.len() > 1 {
            if head_only {
                let head_migrations: Vec<&Migration> = migrations_with_highest_number
                    .into_iter()
                    .filter(|m| !m.from_rebased_branch)
                    .collect();
                if head_migrations.len() == 1 {
                    return Ok(head_migrations.into_iter().next().unwrap());
                }
            }
            Err(format!(
                "Multiple migrations found with the highest number: {}",
                highest_number
            ))
        } else {
            Ok(migrations_with_highest_number.into_iter().next().unwrap())
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
}

impl MigrationGroup {
    fn create(app_path: &Path) -> Result<Self, String> {
        let directory = app_path.join("migrations");

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
        return Ok(Self {
            migrations,
            directory,
            last_common_migration: None,
            max_migration_result: max_migration_result,
            rebased_migrations: Vec::new(),
        });
    }

    fn load_max_migration_file(directory: &Path) -> MaxMigrationResult {
        let max_migration_path = directory.join("max_migration.txt");
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
            let _ok = group.set_last_common_migration(conflict_clone.incoming_change.clone())?;
            group.create_migration_name_changes(conflict_clone);
        }
    }
    // first create all name changes within the same app, then create all dependency changes for other apps.
    django_project.create_migration_dependency_changes(true);
    django_project.create_migration_dependency_changes(false);

    if dry_run == true {
        django_project.changes_summary();
    } else {
        django_project.apply_changes()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::MergeConflict;
    use std::fs;
    use tempfile::{TempDir, tempdir};

    /// Helper function to create a test environment with temp directories
    fn setup_test_env() -> (TempDir, PathBuf) {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let app_dir = temp_dir.path().join("test_app");
        let migrations_dir = app_dir.join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        (temp_dir, migrations_dir)
    }

    /// Helper function to create a max_migration.txt file
    fn create_max_migration_file(migrations_dir: &Path, migration_name: &str) {
        let max_migration_path = migrations_dir.join("max_migration.txt");
        fs::write(&max_migration_path, format!("{}\n", migration_name))
            .expect("Failed to write max migration file");
    }

    fn create_test_migration_file(
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
                .map(|(dep_app, dep_migration)| {
                    format!("        ('{}', {}),", dep_app, dep_migration)
                })
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
        assert_eq!(app.migrations.len(), 2);
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
        let migrations_a_dir = app_a_dir.join("migrations");
        fs::create_dir_all(&migrations_a_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_a_dir, 1, "initial", vec![]);

        // Create max_migration.txt for app_a
        let max_migration_a_path = migrations_a_dir.join("max_migration.txt");
        fs::write(&max_migration_a_path, "0001_initial\n")
            .expect("Failed to write max migration file");

        // Create app_b with a migration that depends on app_a
        let app_b_dir = project_path.join("app_b");
        let migrations_b_dir = app_b_dir.join("migrations");
        fs::create_dir_all(&migrations_b_dir).expect("Failed to create migrations directory");
        create_test_migration_file(
            &migrations_b_dir,
            1,
            "depend_on_a",
            vec![("app_a", "'0001_initial'")],
        );

        // Create max_migration.txt for app_b
        let max_migration_b_path = migrations_b_dir.join("max_migration.txt");
        fs::write(&max_migration_b_path, "0001_depend_on_a\n")
            .expect("Failed to write max migration file");

        let mut project = DjangoProject::from_path(project_path, false).unwrap();

        // Simulate renaming app_a's migration from 0001_initial to 0005_initial
        let app_a = project.apps.get_mut("app_a").unwrap();
        let migration_path = app_a
            .migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0001_initial.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app_a.migrations.get_mut(&migration_path) {
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
            .migrations
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
        let migrations_dir = app_dir.join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_dir, 1, "initial", vec![]);
        create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("myapp", "'0001_initial'")],
        );

        // Create max_migration.txt file
        let max_migration_path = migrations_dir.join("max_migration.txt");
        fs::write(&max_migration_path, "0002_add_field\n")
            .expect("Failed to write max migration file");

        let mut project = DjangoProject::from_path(project_path, false).unwrap();

        // Set up changes: rename migration and update max_migration
        let app = project.apps.get_mut("myapp").unwrap();

        // Add file name change
        let migration_path = app
            .migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0002_add_field.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.migrations.get_mut(&migration_path) {
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
    fn test_migration_parser_new() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create a valid migration file
        let migration_path = create_test_migration_file(&migrations_dir, 1, "initial", vec![]);

        // Test successful parsing
        let result = MigrationParser::new(&migration_path);
        assert!(result.is_ok());

        let parser = result.unwrap();
        assert_eq!(parser.file_path, migration_path);
        // AST should be created successfully (we can't easily test its contents without deep inspection)
    }

    #[test]
    fn test_migration_parser_new_nonexistent_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let nonexistent_path = temp_dir.path().join("nonexistent.py");

        let result = MigrationParser::new(&nonexistent_path);
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.contains("Failed to read file"));
        }
    }

    #[test]
    fn test_migration_parser_new_invalid_python() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let invalid_python_path = temp_dir.path().join("invalid.py");

        // Write invalid Python syntax
        fs::write(
            &invalid_python_path,
            "this is not valid python syntax $$$ %%%",
        )
        .expect("Failed to write invalid Python file");

        let result = MigrationParser::new(&invalid_python_path);
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.contains("Failed to parse python statements"));
        }
    }

    #[test]
    fn test_migration_parser_get_dependencies_no_dependencies() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migration with no dependencies
        let migration_path = create_test_migration_file(&migrations_dir, 1, "initial", vec![]);

        let parser = MigrationParser::new(&migration_path).unwrap();
        let dependencies = parser.get_dependencies();

        assert_eq!(dependencies.len(), 0);
    }

    #[test]
    fn test_migration_parser_get_dependencies_single_dependency() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migration with one dependency
        let migration_path = create_test_migration_file(
            &migrations_dir,
            2,
            "add_field",
            vec![("myapp", "'0001_initial'")],
        );

        let parser = MigrationParser::new(&migration_path).unwrap();
        let dependencies = parser.get_dependencies();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].app, "myapp");
        assert_eq!(dependencies[0].migration_file.0, "0001_initial");
    }

    #[test]
    fn test_migration_parser_get_dependencies_multiple_dependencies() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migration with multiple dependencies
        let migration_path = create_test_migration_file(
            &migrations_dir,
            3,
            "complex",
            vec![
                ("myapp", "'0001_initial'"),
                ("otherapp", "'0002_create_model'"),
                ("thirdapp", "'0001_setup'"),
            ],
        );

        let parser = MigrationParser::new(&migration_path).unwrap();
        let dependencies = parser.get_dependencies();

        assert_eq!(dependencies.len(), 3);

        // Check first dependency
        assert_eq!(dependencies[0].app, "myapp");
        assert_eq!(dependencies[0].migration_file.0, "0001_initial");

        // Check second dependency
        assert_eq!(dependencies[1].app, "otherapp");
        assert_eq!(dependencies[1].migration_file.0, "0002_create_model");

        // Check third dependency
        assert_eq!(dependencies[2].app, "thirdapp");
        assert_eq!(dependencies[2].migration_file.0, "0001_setup");
    }

    #[test]
    fn test_migration_parser_get_dependencies_cross_app() {
        let (_temp_dir, migrations_dir) = setup_test_env();

        // Create migration that depends on different apps
        let migration_path = create_test_migration_file(
            &migrations_dir,
            1,
            "depend_on_auth",
            vec![
                ("auth", "'0012_alter_user_first_name_max_length'"),
                ("contenttypes", "'0002_remove_content_type_name'"),
            ],
        );

        let parser = MigrationParser::new(&migration_path).unwrap();
        let dependencies = parser.get_dependencies();

        assert_eq!(dependencies.len(), 2);

        // Check auth dependency
        assert_eq!(dependencies[0].app, "auth");
        assert_eq!(
            dependencies[0].migration_file.0,
            "0012_alter_user_first_name_max_length"
        );

        // Check contenttypes dependency
        assert_eq!(dependencies[1].app, "contenttypes");
        assert_eq!(
            dependencies[1].migration_file.0,
            "0002_remove_content_type_name"
        );
    }

    #[test]
    fn test_migration_parser_get_dependencies_malformed_file() {
        // TODO: should this error instead??
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let malformed_path = temp_dir.path().join("malformed.py");

        // Create a Python file that doesn't follow Django migration structure
        let content = r#"
# This is not a proper Django migration
def some_function():
    pass

class NotAMigration:
    def do_something(self):
        return []
"#;
        fs::write(&malformed_path, content).expect("Failed to write malformed file");

        let parser = MigrationParser::new(&malformed_path).unwrap();
        let dependencies = parser.get_dependencies();

        // Should return empty vector for malformed migration files
        assert_eq!(dependencies.len(), 0);
    }

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
    fn test_migration_filename_change_apply_change() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join("migrations");
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
        let migrations_dir = temp_dir.path().join("migrations");
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
        let migrations_dir = temp_dir.path().join("migrations");
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
        let migrations_dir = temp_dir.path().join("migrations");
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
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        let migration_file = migrations_dir.join("nonexistent.py");

        let dependency_change = MigrationDependencyChange::new(vec![], vec![]);

        // Apply change to non-existent file - should fail
        let result = dependency_change.apply_change(&migration_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_max_migration_file_apply_change() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join("migrations");
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
        let max_migration_path = migrations_dir.join("max_migration.txt");
        assert!(max_migration_path.exists());

        let content =
            fs::read_to_string(&max_migration_path).expect("Failed to read max migration file");
        assert_eq!(content, "0005_updated\n");
    }

    #[test]
    fn test_max_migration_file_apply_change_overwrite_existing() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");

        // Create an existing max_migration.txt file
        let max_migration_path = migrations_dir.join("max_migration.txt");
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
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let migrations_dir = temp_dir.path().join("migrations");
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
        let max_migration_path = migrations_dir.join("max_migration.txt");
        assert!(!max_migration_path.exists());
    }

    #[test]
    fn test_max_migration_file_apply_change_invalid_directory() {
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
        let max_migration_path = migrations_dir.join("max_migration.txt");
        fs::write(&max_migration_path, "0004_update_model\n")
            .expect("Failed to write max migration file");

        let mut project =
            DjangoProject::from_path(migrations_dir.parent().unwrap().parent().unwrap(), false)
                .unwrap();
        let app = project.apps.get_mut("test_app").unwrap();

        // Mark migrations 3 and 4 as from rebased branch
        for (path, migration) in app.migrations.iter_mut() {
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
        app.set_last_common_migration(mock_conflict.incoming_change.clone());
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
            .migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0001_initial.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.migrations.get_mut(&migration_0001_path) {
            migration.name_change = Some(MigrationFileNameChange::new(
                MigrationFileName("0001_initial".to_string()),
                MigrationFileName("0003_initial".to_string()),
            ));
        }

        // Mark migration 0002 as from rebased branch
        let migration_0002_path = app
            .migrations
            .keys()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == "0002_add_field.py")
            .cloned()
            .unwrap();

        if let Some(migration) = app.migrations.get_mut(&migration_0002_path) {
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
        let migration_0002 = app.migrations.get(&migration_0002_path).unwrap();
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
        let migrations_a_dir = app_a_dir.join("migrations");
        fs::create_dir_all(&migrations_a_dir).expect("Failed to create migrations directory");
        create_test_migration_file(&migrations_a_dir, 1, "initial", vec![]);

        // Create max_migration.txt for app_a
        create_max_migration_file(&migrations_a_dir, "0001_initial");

        // Create app_b with migration that depends on app_a
        let app_b_dir = project_path.join("app_b");
        let migrations_b_dir = app_b_dir.join("migrations");
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
            .migrations
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

    // Ensure that when there are merge migrations in the migrations folder
    // the tool still works as expected. The problem with these merge migrations is that
    // they have the same number and thus migrations are not linear anymore.
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
        let migration_0005_to_be_rebased = app.migrations.get(&new_migration_path).unwrap();
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
        let migration_0004_regular = app.migrations.get(&migration_0004_regular_path).unwrap();

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
}
