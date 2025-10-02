use rustpython_parser::{Parse, ast};
use std::path::PathBuf;

use crate::migration::file::MigrationDependency;

pub struct MigrationParser {
    file_path: PathBuf,
    ast: ast::Suite,
}

impl MigrationParser {
    pub fn new(python_path: &PathBuf) -> Result<Self, String> {
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

    pub fn find_dependency_location(&self) -> Result<(u32, u32), String> {
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

    pub fn get_dependencies(&self) -> Vec<MigrationDependency> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::test_helpers::*;
    use std::fs;
    use tempfile::tempdir;

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
}
