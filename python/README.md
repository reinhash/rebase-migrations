# rebase-migrations

A Python library for Django migration conflict resolution during git rebases. Automatically renumbers Django migration files to resolve conflicts when rebasing feature branches.

## The Problem

When rebasing Django feature branches, you often encounter migration number conflicts:

```
# Main branch has:
migrations/0010_main_change.py
migrations/0011_main_change.py
migrations/0012_latest_feature.py

# Your feature branch has:
migrations/0010_your_feature.py     ← Conflict!
migrations/0011_another_change.py   ← Conflict!
```

This library automatically detects and renumbers conflicting migrations during rebase.

## Installation

```bash
pip install rebase-migrations
```

## Usage

### Basic Usage

```python
import rebase_migrations

# Preview changes
rebase_migrations.dry_run('/path/to/django/project')

# Apply changes
rebase_migrations.execute('/path/to/django/project')
```

### JSON Output

Get machine-readable JSON output for programmatic processing:

```python
import rebase_migrations
import json

json_output = rebase_migrations.dry_run(
    '/path/to/django/project',
    json=True
)

if json_output:
    data = json.loads(json_output)
    print(json.dumps(data, indent=2))
```

Example JSON structure:
```json
{
  "apps": {
    "myapp": {
      "app_name": "myapp",
      "last_common_migration": "0001_initial",
      "migration_changes": [
        {
          "migration_file_name": "0002_add_field",
          "file_rename": {
            "old_name": "0002_add_field",
            "new_name": "0004_add_field"
          }
        }
      ],
      "max_migration_update": {
        "old": "0003_main_branch",
        "new": "0004_add_field"
      }
    }
  }
}
```

### Path Handling

The library supports various path formats:

```python
# Absolute paths
rebase_migrations.execute('/home/user/my-django-project')

# Relative paths
rebase_migrations.execute('.')  # Current directory
rebase_migrations.execute('../my-project')

# Tilde expansion
rebase_migrations.execute('~/my-django-project')
```

### Parameters

#### `execute(path, all_dirs=False)`

Apply migration changes immediately.

- **`path`** (str): Path to the Django project directory
- **`all_dirs`** (bool, optional): If True, scan all directories for migrations. Default: False
  - When False: Skips common directories like `node_modules`, `venv`, `.git`, etc. for better performance
  - When True: Comprehensive scan of all directories (slower but finds migrations in unusual locations)

**Returns:** `None`

#### `dry_run(path, all_dirs=False, json=False)`

Preview changes without applying them.

- **`path`** (str): Path to the Django project directory
- **`all_dirs`** (bool, optional): If True, scan all directories for migrations. Default: False
- **`json`** (bool, optional): If True, return JSON output as a string. Default: False
  - Returns `None` if `json=False`
  - Returns JSON string if `json=True`

**Returns:** `Optional[str]` - JSON string if `json=True`, otherwise `None`

#### Single App Functions

For working with a single Django app:

- **`execute_for_app(app_path)`** - Apply changes to a single app
- **`dry_run_for_app(app_path, json=False)`** - Preview changes to a single app

Both accept the same parameters as their project-level counterparts, but take an `app_path` instead of a project `path`.

## How It Works

1. **Discovers migrations**: Finds Django migration files in your project
2. **Detects conflicts**: Identifies migration number conflicts from git rebase
3. **Renumbers safely**: Assigns new sequential numbers to resolve conflicts
4. **Updates dependencies**: Modifies migration dependencies to maintain integrity
5. **Updates tracking files**: Updates `max_migration.txt` files for django-linear-migrations

## Requirements

- Python 3.8+
- Django project with migration files following naming convention (`NNNN_name.py`)
- Git repository
- **[django-linear-migrations](https://github.com/adamchainz/django-linear-migrations)** package installed and configured

The library automatically updates `max_migration.txt` files that `django-linear-migrations` uses to track the latest migration in each app.

## Features

- 🔍 **Automatic Detection**: Finds migration conflicts during rebase
- 🔄 **Smart Renumbering**: Renumbers migrations to resolve conflicts
- 🔗 **Dependency Updates**: Updates migration dependencies automatically
- 📄 **File Updates**: Updates `max_migration.txt` tracking files
- 🧪 **Dry Run Mode**: Preview changes before applying them
- 📊 **JSON Output**: Machine-readable output for automation and programmatic use
- 🚀 **Fast Performance**: Skips irrelevant directories by default
- 🛠️ **Path Flexibility**: Supports relative paths, tilde expansion, and absolute paths

## CLI Alternative

If you prefer command-line usage, a standalone CLI binary is also available. See the [main repository](https://github.com/reinhash/rebase-migrations) for installation instructions.

## License

MIT License - see [LICENSE](https://github.com/reinhash/rebase-migrations/blob/main/LICENSE) for details.

## Contributing

Contributions welcome! Please see the [main repository](https://github.com/reinhash/rebase-migrations) for contributing guidelines.