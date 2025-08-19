# Django Migration Rebase Tool

A Rust CLI tool that automatically renumbers Django migration files to resolve conflicts during git rebases.

## The Problem

When rebasing Django feature branches, you often encounter migration number conflicts:

```
# Main branch has:
migrations/0001_initial.py
migrations/0012_latest_feature.py

# Your feature branch has:
migrations/0001_initial.py  
migrations/0010_your_feature.py  ← Conflict!
migrations/0011_another_change.py  ← Conflict!
```

This tool automatically detects and renumbers conflicting migrations during rebase.

## Features

- 🔍 **Smart Detection**: Finds both staged and untracked migration files during rebase
- 🔄 **Automatic Renumbering**: Renumbers migrations to avoid conflicts  
- 🔗 **Dependency Updates**: Updates migration dependencies in Python AST
- 📄 **Max Migration Tracking**: Updates `max_migration.txt` files
- 🧪 **Dry Run Mode**: Preview changes before applying them
- ⚡ **Fast**: Written in Rust for performance

## Installation

### Download Pre-built Binary (Recommended)

Download the latest binary for your platform from the [releases page](https://github.com/yourusername/rebase-migrations/releases):

#### macOS (Intel)
```bash
curl -L https://github.com/yourusername/rebase-migrations/releases/latest/download/rebase-migrations-macos-x86_64 -o rebase-migrations
chmod +x rebase-migrations
sudo mv rebase-migrations /usr/local/bin/
```

#### macOS (Apple Silicon)
```bash
curl -L https://github.com/yourusername/rebase-migrations/releases/latest/download/rebase-migrations-macos-aarch64 -o rebase-migrations
chmod +x rebase-migrations
sudo mv rebase-migrations /usr/local/bin/
```

#### Linux (x86_64)
```bash
curl -L https://github.com/yourusername/rebase-migrations/releases/latest/download/rebase-migrations-linux-x86_64 -o rebase-migrations
chmod +x rebase-migrations
sudo mv rebase-migrations /usr/local/bin/
```

#### Windows
Download `rebase-migrations-windows-x86_64.exe` from the releases page and add it to your PATH.

### Install from Source (Requires Rust)

```bash
cargo install --git https://github.com/yourusername/rebase-migrations
```

## Usage

### Basic Usage

```bash
# Preview changes (dry run)
rebase-migrations --path . --dry-run

# Apply changes
rebase-migrations --path .
```

### Common Workflow

1. **During a rebase** when you encounter migration conflicts:
```bash
# Instead of manually renumbering, just run:
rebase-migrations --path . --dry-run  # Preview
rebase-migrations --path .           # Apply
git add .
git rebase --continue
```

2. **Before committing** new migrations:
```bash
rebase-migrations --path . --dry-run  # Check for conflicts
```

### Options

- `--path <PATH>`: Path to the Django project root (default: current directory)
- `--dry-run`: Show what would be changed without making modifications

## How It Works

1. **Discovers migrations**: Finds both staged and untracked migration files in your repository
2. **Groups by app**: Organizes migrations by Django app 
3. **Finds conflicts**: Identifies migrations that would conflict with existing ones
4. **Renumbers safely**: Assigns new sequential numbers starting after the highest existing migration
5. **Updates dependencies**: Modifies Python AST to update migration dependencies
6. **Updates tracking files**: Updates `max_migration.txt` files if present

## Example

Before:
```
myapp/migrations/
├── 0001_initial.py
├── 0010_existing.py      # From main branch
├── 0008_new_feature.py   # From your branch - CONFLICT!
└── 0009_another.py       # From your branch - CONFLICT!
```

After running `rebase-migrations`:
```
myapp/migrations/
├── 0001_initial.py
├── 0010_existing.py      
├── 0011_new_feature.py   # Renumbered!
└── 0012_another.py       # Renumbered!
```

## Requirements

- Git repository
- Django project with standard migration structure
- Python migration files following Django naming convention (`NNNN_name.py`)

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## Development

```bash
# Clone the repository
git clone https://github.com/yourusername/rebase-migrations
cd rebase-migrations

# Run tests
cargo test

# Build
cargo build --release

# Install locally for testing
cargo install --path .
```

## License

MIT License - see LICENSE file for details.