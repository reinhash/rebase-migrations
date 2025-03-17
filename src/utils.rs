use git2::Repository;
use std::path::{Path, PathBuf};

/// Check if the given string is a migration file name.
/// A migration file name should start with a number followed by an underscore.
/// For example: `"0001_initial.py"`, `"0002_auto_20230901_1234.py"`
pub fn is_migration_file(s: &str) -> bool {
    s.find('_')
        .is_some_and(|pos| pos > 0 && s[..pos].chars().all(|c| c.is_ascii_digit()))
}

pub fn find_relevant_migrations(repo_path: &Path) -> Vec<PathBuf> {
    let mut migrations = Vec::new();
    let repo = match Repository::open(repo_path) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to open git repository at {}: {}", repo_path.display(), e);
            return migrations;
        }
    };

    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_ignored(false)
        .include_untracked(true)  // Include untracked files
        .include_unmodified(false)
        .recurse_untracked_dirs(true);  // Recurse into untracked directories

    let statuses = match repo.statuses(Some(&mut status_opts)) {
        Ok(statuses) => statuses,
        Err(e) => {
            eprintln!("Failed to get git status: {}", e);
            return migrations;
        }
    };

    for status_entry in statuses.iter() {
        let status = status_entry.status();
        let Some(path) = status_entry.path() else {
            continue;
        };

        // Include both staged AND untracked files (common during rebase)
        let is_relevant = status.is_index_new() 
            || status.is_index_modified() 
            || status.is_index_renamed()
            || status.is_wt_new();  // Untracked files in working tree

        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        if is_relevant
            && path.contains("migrations")
            && path.ends_with(".py")
            && path != "__init__.py"
        {
            let file_name = Path::new(path).file_name().unwrap_or_default();
            let file_name_str = file_name.to_string_lossy();

            if is_migration_file(&file_name_str) {
                migrations.push(repo_path.join(path));
            }
        }
    }

    migrations
}

pub fn stringify_migration_path(migration: &Path) -> Option<String> {
    if let Some(file_name) = migration.file_name() {
        let file_name_str = file_name.to_string_lossy();
        return Some(file_name_str.to_string());
    }
    None
}

pub fn get_number_from_migration(migration: &Path) -> Option<u32> {
    let file_name_str = stringify_migration_path(migration)?;
    if is_migration_file(&file_name_str) {
        let number_str = &file_name_str[..4];
        return number_str.parse::<u32>().ok();
    }
    None
}

pub fn get_name_from_migration(migration: &Path) -> Option<String> {
    let file_name_str = stringify_migration_path(migration)?;
    if !is_migration_file(&file_name_str) {
        return None;
    }
    if let Some(pos) = file_name_str.find('_') {
        if pos > 0 {
            let name = &file_name_str[pos + 1..];
            if let Some(name_stripped) = name.strip_suffix(".py") {
                return Some(name_stripped.to_string());
            }
            return Some(name.to_string());
        }
    }
    None
}

pub fn replace_range_in_file(
    file_path: &str,
    start_offset: usize,
    end_offset: usize,
    replacement: &str,
    dry_run: bool,
) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    let content = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file {file_path}: {e}"))?;
    let new_content = format!(
        "{}{}{}",
        &content[..start_offset],
        replacement,
        &content[end_offset..]
    );
    std::fs::write(file_path, new_content)
        .map_err(|e| format!("Failed to write to file {file_path}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_is_migration_file() {
        // Valid migration files
        assert!(is_migration_file("0001_initial.py"));
        assert!(is_migration_file("0002_auto_20230901_1234.py"));
        assert!(is_migration_file("9999_final_migration.py"));
        assert!(is_migration_file("1_short.py"));
        
        // Invalid migration files
        assert!(!is_migration_file("__init__.py"));
        assert!(!is_migration_file("models.py"));
        assert!(!is_migration_file("_invalid.py"));
        assert!(!is_migration_file("invalid_0001.py"));
        assert!(!is_migration_file("0001.py"));
        assert!(!is_migration_file("abc_invalid.py"));
        assert!(!is_migration_file(""));
        assert!(!is_migration_file("0001"));
    }

    #[test]
    fn test_stringify_migration_path() {
        let path = Path::new("/path/to/0001_initial.py");
        assert_eq!(stringify_migration_path(path), Some("0001_initial.py".to_string()));
        
        let path = Path::new("0002_models.py");
        assert_eq!(stringify_migration_path(path), Some("0002_models.py".to_string()));
        
        // Directory path still has a filename component
        let path = Path::new("/path/to/directory/");
        assert_eq!(stringify_migration_path(path), Some("directory".to_string()));
        
        // Empty path should return None 
        let path = Path::new("");
        assert_eq!(stringify_migration_path(path), None);
    }

    #[test]
    fn test_get_number_from_migration() {
        let path = Path::new("0001_initial.py");
        assert_eq!(get_number_from_migration(path), Some(1));
        
        let path = Path::new("/full/path/0042_migration.py");
        assert_eq!(get_number_from_migration(path), Some(42));
        
        let path = Path::new("9999_final.py");
        assert_eq!(get_number_from_migration(path), Some(9999));
        
        // Invalid migration files
        let path = Path::new("__init__.py");
        assert_eq!(get_number_from_migration(path), None);
        
        let path = Path::new("models.py");
        assert_eq!(get_number_from_migration(path), None);
        
        let path = Path::new("invalid_0001.py");
        assert_eq!(get_number_from_migration(path), None);
    }

    #[test]
    fn test_get_name_from_migration() {
        let path = Path::new("0001_initial.py");
        assert_eq!(get_name_from_migration(path), Some("initial".to_string()));
        
        let path = Path::new("/full/path/0042_auto_20230901_1234.py");
        assert_eq!(get_name_from_migration(path), Some("auto_20230901_1234".to_string()));
        
        let path = Path::new("9999_complex_migration_name.py");
        assert_eq!(get_name_from_migration(path), Some("complex_migration_name".to_string()));
        
        // Test without .py extension
        let path = Path::new("0001_initial");
        assert_eq!(get_name_from_migration(path), Some("initial".to_string()));
        
        // Invalid migration files
        let path = Path::new("__init__.py");
        assert_eq!(get_name_from_migration(path), None);
        
        let path = Path::new("models.py");
        assert_eq!(get_name_from_migration(path), None);
        
        let path = Path::new("invalid_0001.py");
        assert_eq!(get_name_from_migration(path), None);
    }

    #[test]
    fn test_replace_range_in_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_file.txt");
        let file_path_str = file_path.to_str().unwrap();
        
        // Create test file
        let original_content = "Hello, world! This is a test file.";
        fs::write(&file_path, original_content).expect("Failed to write test file");
        
        // Test replacing a range
        replace_range_in_file(file_path_str, 7, 12, "Rust", false)
            .expect("Failed to replace range");
        
        let new_content = fs::read_to_string(&file_path).expect("Failed to read file");
        assert_eq!(new_content, "Hello, Rust! This is a test file.");
    }

    #[test]
    fn test_replace_range_in_file_dry_run() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_file.txt");
        let file_path_str = file_path.to_str().unwrap();
        
        // Create test file
        let original_content = "Hello, world! This is a test file.";
        fs::write(&file_path, original_content).expect("Failed to write test file");
        
        // Test dry run - should not modify file
        replace_range_in_file(file_path_str, 7, 12, "Rust", true)
            .expect("Failed to replace range in dry run");
        
        let content = fs::read_to_string(&file_path).expect("Failed to read file");
        assert_eq!(content, original_content); // Should be unchanged
    }

    #[test]
    fn test_replace_range_in_file_nonexistent() {
        let result = replace_range_in_file("/nonexistent/file.txt", 0, 5, "test", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
    }

    #[test]
    fn test_find_relevant_migrations_no_git_repo() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should return empty vec when no git repo exists
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_relevant_migrations_empty_repo() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let _repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");
        
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should return empty vec when no staged migrations exist
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_relevant_migrations_with_staged_files() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");
        
        // Create migrations directory
        let migrations_dir = temp_dir.path().join("app").join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        
        // Create migration files
        let migration1 = migrations_dir.join("0001_initial.py");
        let migration2 = migrations_dir.join("0002_models.py");
        let non_migration = migrations_dir.join("__init__.py");
        let regular_file = migrations_dir.join("models.py");
        
        fs::write(&migration1, "# Migration content").expect("Failed to write migration1");
        fs::write(&migration2, "# Migration content").expect("Failed to write migration2");
        fs::write(&non_migration, "# Init file").expect("Failed to write __init__.py");
        fs::write(&regular_file, "# Regular python file").expect("Failed to write models.py");
        
        // Stage the files
        let mut index = repo.index().expect("Failed to get index");
        index.add_path(Path::new("app/migrations/0001_initial.py"))
            .expect("Failed to stage migration1");
        index.add_path(Path::new("app/migrations/0002_models.py"))
            .expect("Failed to stage migration2");
        index.add_path(Path::new("app/migrations/__init__.py"))
            .expect("Failed to stage __init__.py");
        index.add_path(Path::new("app/migrations/models.py"))
            .expect("Failed to stage models.py");
        index.write().expect("Failed to write index");
        
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should only find the migration files, not __init__.py or models.py
        assert_eq!(result.len(), 2);
        
        let filenames: Vec<String> = result.iter()
            .filter_map(|path| path.file_name()?.to_str())
            .map(|s| s.to_string())
            .collect();
        
        assert!(filenames.contains(&"0001_initial.py".to_string()));
        assert!(filenames.contains(&"0002_models.py".to_string()));
        assert!(!filenames.contains(&"__init__.py".to_string()));
        assert!(!filenames.contains(&"models.py".to_string()));
    }

    #[test]
    fn test_find_relevant_migrations_only_unstaged() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let _repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");
        
        // Create migrations directory
        let migrations_dir = temp_dir.path().join("app").join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        
        // Create migration file but don't stage it (it will be untracked)
        let migration1 = migrations_dir.join("0001_initial.py");
        fs::write(&migration1, "# Migration content").expect("Failed to write migration1");
        
        // Don't stage the file - it should still be found as untracked
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should find the untracked migration file 
        assert_eq!(result.len(), 1);
        assert!(result[0].file_name().unwrap().to_str().unwrap().contains("0001_initial.py"));
    }

    #[test]
    fn test_find_relevant_migrations_outside_migrations_dir() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");
        
        // Create a migration file outside migrations directory
        let migration_file = temp_dir.path().join("0001_initial.py");
        fs::write(&migration_file, "# Migration content").expect("Failed to write migration");
        
        // Stage the file
        let mut index = repo.index().expect("Failed to get index");
        index.add_path(Path::new("0001_initial.py"))
            .expect("Failed to stage migration");
        index.write().expect("Failed to write index");
        
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should return empty vec since file is not in migrations directory
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_relevant_migrations_includes_untracked() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let repo = Repository::init(temp_dir.path()).expect("Failed to create git repo");
        
        // Create migrations directory
        let migrations_dir = temp_dir.path().join("app").join("migrations");
        fs::create_dir_all(&migrations_dir).expect("Failed to create migrations directory");
        
        // Create untracked migration files (common during rebase)
        let migration1 = migrations_dir.join("0013_new_feature.py");
        let migration2 = migrations_dir.join("0014_another_feature.py");
        
        fs::write(&migration1, "# New migration from feature branch").expect("Failed to write migration1");
        fs::write(&migration2, "# Another migration from feature branch").expect("Failed to write migration2");
        
        // Refresh the git index to make sure git sees the files
        let mut index = repo.index().expect("Failed to get index");
        index.read(true).expect("Failed to read index");
        
        // Don't stage the files - they should still be found as untracked
        let result = find_relevant_migrations(temp_dir.path());
        
        // Should find the untracked migration files
        assert_eq!(result.len(), 2, "Expected 2 untracked migration files");
        
        let filenames: Vec<String> = result.iter()
            .filter_map(|path| path.file_name()?.to_str())
            .map(|s| s.to_string())
            .collect();
        
        assert!(filenames.contains(&"0013_new_feature.py".to_string()));
        assert!(filenames.contains(&"0014_another_feature.py".to_string()));
    }
}