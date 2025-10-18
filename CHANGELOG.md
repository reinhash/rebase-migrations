## [0.5.0] - 2025-10-18

### 🚀 Features

- Improve performance of open syscall by parallelizing them when opening migration files (one app, one thread)

### 🐛 Bug Fixes

- Ensure total migrations is calculated correctly, print summary also for single app

### ⚙️ Miscellaneous Tasks

- *(release)* Prepare release v0.5.0
## [0.4.0] - 2025-10-06

### 🚀 Features

- Add json output

### 🐛 Bug Fixes

- Add feature configuration to build python optionally

### 🚜 Refactor

- Rename MigrationGroup to DjangoApp for consistency across modules
- Update apply_change methods to accept Migration objects and streamline path handling
- Streamline apply_changes methods in Migration and DjangoProject for improved clarity and efficiency

### ⚙️ Miscellaneous Tasks

- Bump version to 0.4.0
## [0.3.0] - 2025-10-02

### 🚀 Features

- Add setup for python bindings

### 🐛 Bug Fixes

- Ensure docs include python package description
- Ensure docs are separated between CLI and python package
- Only build CLI binary in release workflow
- Make pyo3 dependency optional for CLI-only builds

### 🚜 Refactor

- Clearly name head_migrations and refactor how to find the highest migrations

### 🧪 Testing

- Augment test cases to cover edge cases

### ⚙️ Miscellaneous Tasks

- Bump version to 0.3.0
## [0.2.0] - 2025-09-16

### 🚀 Features

- Traverse dependencies for rebasing, remove git2 dependency, add pretty printing with tables

### 🚜 Refactor

- Use result when searching for migrations
- Rename methods for clarity and improve migration file handling

### 🧪 Testing

- Enhance create_test_migration_file to support multiple dependencies and add cross-app dependency test

### ⚙️ Miscellaneous Tasks

- Prepare release v0.1.0
- Update version
