# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust CLI tool called `rebase-migrations` that helps with Django migration rebasing. The tool analyzes staged Django migration files in a Git repository and automatically renumbers them to resolve conflicts that occur during rebasing.

## Architecture

The codebase is structured as a simple CLI application with three main modules:

- **main.rs**: Entry point that handles CLI parsing and calls the core rebase functionality
- **cli.rs**: Command-line interface definition using the `clap` crate, supporting `--path` and `--dry-run` options
- **rebase.rs**: Core business logic that:
  - Finds staged migration files using git2
  - Groups migrations by Django app
  - Parses Python AST to find and update migration dependencies
  - Renumbers migration files to avoid conflicts
  - Updates the `max_migration.txt` file if it exists

Key data structures:
- `Migration`: Represents a single Django migration file with old/new numbers and dependencies
- `MigrationGroup`: Groups migrations by Django app directory, handles the renumbering logic

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
- `walkdir`: Directory traversal (though not currently used)
- `git2`: Git repository operations to find staged files
- `regex`: Pattern matching for migration files
- `rustpython-parser`: Python AST parsing for updating migration dependencies

### Migration File Detection
Migration files are identified by the pattern: `{4-digit-number}_{name}.py` (e.g., `0001_initial.py`)

### Key Functions
- `find_staged_migrations()`: Uses git2 to find staged migration files
- `find_migration_string_location_in_file()`: Parses Python AST to locate dependency strings
- `MigrationGroup::find_last_head_migration()`: Finds the highest numbered migration not from the current branch

### Testing Strategy
The project includes comprehensive unit tests using `tempfile` for creating temporary test environments. Tests cover file parsing, migration grouping, and the complete rebase workflow.