# Django Migration Rebase Tool

A tool that automatically renumbers Django migration files to resolve conflicts during git rebases. Available as both a standalone CLI binary and a Python library. Designed to work seamlessly with [django-linear-migrations](https://github.com/adamchainz/django-linear-migrations).

## The Problem

When rebasing Django feature branches, you often encounter migration number conflicts:

```
# Main branch has:
migrations/0001_initial.py
...
migrations/0010_main_change.py
migrations/0011_main_change.py
migrations/0012_latest_feature.py

# Your feature branch has:
migrations/0001_initial.py  
...
migrations/0010_your_feature.py  ← Conflict!
migrations/0011_another_change.py  ← Conflict!
```

This tool automatically detects and renumbers conflicting migrations during rebase.

## Features

- 🔍 Finds both staged and untracked migration files during rebase
- 🔄 Automatic Renumbering: Renumbers migrations to avoid conflicts
- 🔗 Updates migration dependencies in Python AST
- 📄 Updates `max_migration.txt` files
- 🧪 Dry Run Mode: Preview changes before applying them

## CLI Tool

### Installation

#### Download Pre-built Binary (Recommended)

Download the latest binary for your platform from the [releases page](https://github.com/reinhash/rebase-migrations/releases)

#### Install from Source (Requires Rust)

```bash
cargo install --git https://github.com/reinhash/rebase-migrations
```

#### Compile from Source (Requires Rust)

```bash
git clone https://github.com/reinhash/rebase-migrations
cd rebase-migrations
cargo build --release
```

### Usage

#### Basic Usage

```bash
# Preview changes (dry run)
rebase-migrations --dry-run

# Apply changes
rebase-migrations
```

#### Common Workflow

1. **During a rebase** when you encounter migration conflicts:
```bash
# Instead of manually renumbering, just run:
rebase-migrations --dry-run  # Preview
rebase-migrations            # Apply
git add .
git rebase --continue
```

#### Options

- `--path <PATH>`: Path to the Django project root (default: current directory)
- `--dry-run`: Show what would be changed without making modifications
- `--all-dirs`: Scan all directories without skipping common build/cache directories (slower but comprehensive)

## Python Package

Install the Python library for programmatic use:

```bash
pip install rebase-migrations
```

For detailed Python usage documentation, see [python/README.md](python/README.md) or visit the [PyPI page](https://pypi.org/project/rebase-migrations/).

## Performance Notes

By default, the tool skips common directories that are unlikely to contain Django migrations for better performance:

- **Version control**: `.git`, `.svn`, `.hg`
- **Python environments**: `venv`, `.venv`, `env`, `.env`, `virtualenv`, `__pycache__`, `.pytest_cache`, `.tox`
- **Node.js**: `node_modules`, `.npm`, `.yarn`
- **Build/cache**: `build`, `dist`, `.cache`, `target`, `.mypy_cache`, `.coverage`, `htmlcov`
- **IDE/Editor**: `.vscode`, `.idea`, `.sublime-project`, `.sublime-workspace`
- **OS files**: `.DS_Store`, `Thumbs.db`
- **Django assets**: `static`, `staticfiles`, `media`
- **Docker**: `.docker`
- **Documentation**: `docs`, `_build`

Use `--all-dirs` if you have Django apps in unconventional locations or if the default filtering is missing your migrations.

## How It Works

1. **Discovers migrations**: Finds both staged and untracked migration files in your repository
2. **Groups by app**: Organizes migrations by Django app 
3. **Finds conflicts**: Identifies migrations that would conflict with existing ones
4. **Renumbers safely**: Assigns new sequential numbers starting after the highest existing migration
5. **Updates dependencies**: Modifies Python AST to update migration dependencies
6. **Updates tracking files**: Updates `max_migration.txt`


## Requirements

- Git repository
- Python migration files following Django naming convention (`NNNN_name.py`)
- **[django-linear-migrations](https://github.com/adamchainz/django-linear-migrations)** package installed and configured

This tool automatically updates the `max_migration.txt` files that `django-linear-migrations` uses to track the latest migration in each app.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## Development

```bash
# Clone the repository
git clone https://github.com/reinhash/rebase-migrations
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