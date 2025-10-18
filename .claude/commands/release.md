# Create a New Release

Guide for creating a new release of rebase-migrations, which includes both Rust CLI binaries and a Python package.

This repository has a dual release process:
- **Rust CLI**: Binaries are automatically built and released to GitHub when a tag is pushed (via GitHub Actions)
- **Python Package**: Distributed via PyPI using `maturin publish`

## Pre-Release Checklist

1. Ensure all changes are committed
2. Review the changelog to understand what will be included
3. Verify tests pass: `cargo test`
4. Verify code quality: `cargo clippy -- -D warnings`

## Release Steps

### 1. Determine the New Version

Use semantic versioning (MAJOR.MINOR.PATCH). The current version is found in:
- `Cargo.toml`: `[package] version = "X.Y.Z"`
- `pyproject.toml`: `[project] version = "X.Y.Z"`
- `src/cli.rs`: `.version("X.Y.Z")` in the clap command builder

### 2. Update Version Numbers

Update the version in all three locations to match the new release version:

```bash
# Edit Cargo.toml - change the version field in [package]
# Edit pyproject.toml - change the version field in [project]
# Edit src/cli.rs - change the version in .version("X.Y.Z")
```

All three files MUST have identical version numbers for consistency.

### 3. Generate/Update Changelog

The project uses `git-cliff` for changelog generation. Generate the changelog for the new release:

```bash
git-cliff --latest > /tmp/latest.md  # Preview the changelog
git-cliff > CHANGELOG.md              # Update the full changelog
```

Commit these changes:

```bash
git add Cargo.toml pyproject.toml src/cli.rs CHANGELOG.md
git commit -m "chore(release): prepare release vX.Y.Z"
```

### 4. Create Git Tag

Create a semantic version tag (without 'v' prefix - the GitHub Actions workflow expects `X.Y.Z` format):

```bash
git tag X.Y.Z
```

### 5. Push to GitHub

Push both the commits and the tag to trigger the GitHub Actions workflow:

```bash
git push origin main           # Push commits to main branch
git push origin X.Y.Z          # Push the tag to trigger CI/CD
```

This will trigger the GitHub Actions workflow defined in `.github/workflows/release.yml`, which:
- Builds binaries for Linux, macOS (Intel), macOS (Apple Silicon), and Windows
- Creates a GitHub Release with all binary assets attached

### 6. Publish Python Package to PyPI

After the GitHub Release is created, publish the Python package to PyPI:

```bash
# Ensure your PyPI credentials are configured (usually in ~/.pypirc or via environment variables)
maturin publish --skip-existing
```

The `--skip-existing` flag prevents errors if wheels already exist for this version.

### 7. Verify Release

After pushing the tag and publishing:

1. **GitHub Release**: Check https://github.com/reinhash/rebase-migrations/releases to verify:
   - The release was created with the correct version
   - All binary assets are present (Linux, macOS Intel, macOS Apple Silicon, Windows)
   - Changelog is included in the release notes

2. **Python Package**: Verify on PyPI:
   ```bash
   uv pip install rebase-migrations==X.Y.Z
   ```

## Environment Requirements

### For Building and Publishing
- Rust toolchain (rustc, cargo)
- Python 3.8+
- maturin: `uv pip install maturin`
- uv: for Python package management

### For Full Release
- git
- GitHub CLI (gh) - optional but useful for viewing releases

## PyPI Authentication

`maturin publish` requires PyPI credentials. Set up one of the following:

1. **Token-based (recommended)**:
   ```bash
   export MATURIN_PYPI_TOKEN="pypi-AgEIcHlwaS5vcmc..."
   ```

2. **~/.pypirc file**:
   ```ini
   [distutils]
   index-servers = pypi

   [pypi]
   repository = https://upload.pypi.org/legacy/
   username = __token__
   password = pypi-AgEIcHlwaS5vcmc...
   ```

3. **GitHub trusted publishing** (set up in PyPI account settings):
   - No local credentials needed when publishing from GitHub Actions

## Configuration Files Reference

- **Cargo.toml**: Rust package metadata and dependencies
- **pyproject.toml**: Python package metadata, uses maturin as build backend
- **cliff.toml**: Configuration for git-cliff changelog generation
- **.github/workflows/release.yml**: GitHub Actions workflow for building binaries on tag push

## Common Issues

### Version Mismatch
If Cargo.toml, pyproject.toml, and src/cli.rs have different versions, the releases will be inconsistent. Always update all three files.

### Tag Format
The GitHub Actions workflow only triggers on tags matching the pattern `[0-9]+.[0-9]+.[0-9]+` (semantic versioning without 'v' prefix).

### Changelog Not Updated
If git-cliff doesn't include recent commits, ensure they follow the conventional commits format (e.g., `feat: ...`, `fix: ...`, `chore: ...`).

### maturin publish Fails
Verify that:
- PyPI credentials are configured and valid
- The version in `pyproject.toml` matches the git tag
- You have permission to publish to the rebase-migrations package on PyPI

## Release Workflow Summary

```bash
# 1. Update versions in Cargo.toml, pyproject.toml, and src/cli.rs to X.Y.Z

# 2. Generate changelog and commit
git-cliff > CHANGELOG.md
git add Cargo.toml pyproject.toml src/cli.rs CHANGELOG.md
git commit -m "chore(release): prepare release vX.Y.Z"

# 3. Create tag and push (triggers GitHub Actions)
git tag X.Y.Z
git push origin main X.Y.Z

# 4. Wait for GitHub Actions to complete, then publish to PyPI
maturin publish --skip-existing

# 5. Verify releases on GitHub and PyPI
```
