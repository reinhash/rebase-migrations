## [0.1.0] - 2025-08-19

### 🚀 Features

- Enhance migration handling with dry run support and Python AST parsing
- Update dependencies in migration files
- Add functionality to find and update the highest migration number
- Add tests
- Also consider unstaged migrations
- Add GitHub Actions workflow for automated release process

### 🐛 Bug Fixes

- Find last migration by number
- Handle non-existent max migration file gracefully
- Improve error handling for empty migration groups

### 🚜 Refactor

- Remove .unwrap() and propagate errors
- Remove FIX subcommand and simplify CLI argument handling
- Clean up whitespace and formatting in rebase.rs

### 📚 Documentation

- Enhance README with additional context on django-linear-migrations and max_migration.txt updates

### ⚙️ Miscellaneous Tasks

- Update package metadata in Cargo.toml
