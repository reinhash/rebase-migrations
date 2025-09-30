use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

mod rebase;
mod tables;
mod utils;

#[pyfunction]
#[pyo3(signature = (path, dry_run=false, all_dirs=false))]
/// Fix Django migration conflicts during git rebase.
///
/// Args:
///     path: Path to the Django project directory
///     dry_run: If True, preview changes without applying them (default: False)
///     all_dirs: If True, scan all directories for performance (default: False)
///
/// Raises:
///     RuntimeError: If the migration rebase operation fails
fn run_rebase(path: &str, dry_run: bool, all_dirs: bool) -> PyResult<()> {
    rebase::fix(path, dry_run, all_dirs).map_err(PyErr::new::<PyRuntimeError, _>)
}

/// A Python module implemented in Rust. The name of this function must match
/// the `lib.name` setting in the `Cargo.toml`, else Python will not be able to
/// import the module.
#[pymodule]
fn rebase_migrations(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_rebase, m)?)?;

    Ok(())
}
