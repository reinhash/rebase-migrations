use crate::rebase::MigrationFileName;

#[derive(Debug, Clone)]
pub struct MergeConflict {
    pub head: MigrationFileName,
    pub incoming_change: MigrationFileName,
}

impl TryFrom<String> for MergeConflict {
    type Error = String;

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

pub fn replace_range_in_file(
    file_path: &str,
    start_offset: usize,
    end_offset: usize,
    replacement: &str,
) -> Result<(), String> {
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
    fn test_replace_range_in_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_file.txt");
        let file_path_str = file_path.to_str().unwrap();

        // Create test file
        let original_content = "Hello, world! This is a test file.";
        fs::write(&file_path, original_content).expect("Failed to write test file");

        // Test replacing a range
        replace_range_in_file(file_path_str, 7, 12, "Rust").expect("Failed to replace range");

        let new_content = fs::read_to_string(&file_path).expect("Failed to read file");
        assert_eq!(new_content, "Hello, Rust! This is a test file.");
    }

    #[test]
    fn test_replace_range_in_file_nonexistent() {
        let result = replace_range_in_file("/nonexistent/file.txt", 0, 5, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
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
