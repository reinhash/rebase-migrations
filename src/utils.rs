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
}
