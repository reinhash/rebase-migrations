# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust CLI tool called `rebase-migrations` that helps with Django migration rebasing. The tool analyzes Django migration files in a Git repository and automatically renumbers them to resolve conflicts that occur during rebasing, with beautiful colored table output for dry-run mode.

## Architecture

The codebase is structured as a CLI application with four main modules:

- **main.rs**: Entry point that handles CLI parsing and calls the core rebase functionality
- **cli.rs**: Command-line interface definition using the `clap` crate, supporting `--path` and `--dry-run` options
- **rebase.rs**: Core business logic that:
  - Discovers Django apps with migrations directories
  - Detects migration conflicts from `max_migration.txt` files with Git merge markers
  - Parses Python AST to find and update migration dependencies
  - Renumbers migration files to avoid conflicts
  - Updates the `max_migration.txt` file if it exists
- **tables.rs**: Beautiful colored table output using `cli_table` for dry-run mode
- **utils.rs**: Utility functions for file operations

Key data structures:
- `DjangoProject`: Root container managing all Django apps and their migrations
- `MigrationGroup`: Groups migrations by Django app directory, handles renumbering logic
- `Migration`: Represents a single Django migration file with old/new numbers and dependencies
- `MigrationFileName`: Type-safe wrapper for migration filenames with validation
- `MigrationDependency`: Represents a Django migration dependency (app, filename) pair

## Common Commands

### Build and Run
```bash
cargo build                    # Build the project
cargo run -- --help          # Show help
cargo run -- --path .        # Run on current directory
cargo run -- --dry-run       # Preview changes without applying them
```

### Testing
```bash
cargo test                    # Run all tests
cargo test test_name          # Run specific test
cargo test -- --nocapture    # Run tests with output
```

### Code Quality
```bash
cargo clippy                  # Run linter
cargo clippy -- -D warnings  # Treat warnings as errors
cargo check                   # Fast syntax checking
cargo fmt                     # Format code
```

## Development Notes

### Dependencies
- `clap`: CLI argument parsing
- `walkdir`: Directory traversal for finding Django apps
- `regex`: Pattern matching for migration files
- `rustpython-parser`: Python AST parsing for updating migration dependencies
- `cli_table`: Beautiful colored table output for dry-run mode

### Migration File Detection
Migration files are identified by the pattern: `{4-digit-number}_{name}.py` (e.g., `0001_initial.py`)

**Important**: The tool only processes migrations directories that contain a `max_migration.txt` file. Migrations folders without this file are ignored during discovery, even if they are named "migrations".

### Conflict Detection
The tool detects migration conflicts by:
1. Looking for `max_migration.txt` files with Git merge conflict markers (`<<<<<<< HEAD`, `=======`, `>>>>>>> `)
2. Extracting the HEAD and rebased branch migration names from the conflict markers
3. Using these to determine which migrations need renumbering

### Key Functions
- `DjangoProject::from_path()`: Discovers all Django apps in the repository
- `MigrationGroup::find_max_migration_conflict()`: Detects conflicts in max_migration.txt files
- `MigrationGroup::set_last_common_migration()`: Finds the common ancestor migration
- `MigrationGroup::create_migration_name_changes()`: Generates new migration numbers
- `MigrationParser::get_dependencies()`: Parses Python AST to extract migration dependencies
- `fix()`: Main entry point that orchestrates the entire rebase process

### TryFrom Implementations
The codebase uses clean `TryFrom<&ast::Expr>` implementations for parsing Python AST:
- `MigrationFileName::try_from()`: Extracts migration filename from tuple expressions
- `MigrationDependency::try_from()`: Extracts full dependency information from tuple expressions

### Table Output
Dry-run mode shows colored tables with:
- **Summary Table**: Overview of all apps with change counts
- **Migration Changes**: Detailed view of file renames and dependency updates per app
- **Max Migration Changes**: Updates to max_migration.txt files
- **Color Coding**: Blue (file renames), Magenta (dependencies), Green (max_migration updates)

### Testing Strategy
The project includes comprehensive unit tests using `tempfile` for creating temporary test environments. Tests cover file parsing, migration grouping, and the complete rebase workflow.