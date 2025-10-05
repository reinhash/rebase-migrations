// This library is only built when the python-build feature is enabled (for Python bindings)
// When building the CLI binary, this file is not compiled at all thanks to required-features in Cargo.toml

#![cfg(feature = "python-build")]

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use std::path::Path;

mod migration;
mod tables;
mod utils;

#[pyfunction]
#[pyo3(signature = (path, dry_run=false, all_dirs=false, json=false))]
/// Fix Django migration conflicts during git rebase.
///
/// Args:
///     path: Path to the Django project directory
///     dry_run: If True, preview changes without applying them (default: False)
///     all_dirs: If True, scan all directories for performance (default: False)
///     json: If True, return JSON output instead of printing tables (requires dry_run=True) (default: False)
///
/// Raises:
///     RuntimeError: If the migration rebase operation fails
fn run_rebase(path: &str, dry_run: bool, all_dirs: bool, json: bool) -> PyResult<()> {
    migration::project::rebase_apps(path, dry_run, all_dirs, json)
        .map_err(PyErr::new::<PyRuntimeError, _>)
}

#[pyfunction]
#[pyo3(signature = (app_path, dry_run=false, json=false))]
/// Fix Django migration conflicts during git rebase.
///
/// Args:
///     app_path: Path to the Django App directory
///     dry_run: If True, preview changes without applying them (default: False)
///     json: If True, return JSON output instead of printing tables (requires dry_run=True) (default: False)
///
/// Raises:
///     RuntimeError: If the migration rebase operation fails
fn run_rebase_for_one_app(app_path: &str, dry_run: bool, json: bool) -> PyResult<()> {
    let app_path = Path::new(app_path);
    migration::project::rebase_app(app_path, dry_run, json).map_err(PyErr::new::<PyRuntimeError, _>)
}

/// A Python module implemented in Rust. The name of this function must match
/// the `lib.name` setting in the `Cargo.toml`, else Python will not be able to
/// import the module.
#[pymodule]
fn rebase_migrations(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_rebase, m)?)?;
    m.add_function(wrap_pyfunction!(run_rebase_for_one_app, m)?)?;

    Ok(())
}
