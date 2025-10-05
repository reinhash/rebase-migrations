"""Python library for Django migration conflict resolution during git rebases."""

import os
from .rebase_migrations import run_rebase, run_rebase_for_one_app

def _expand_path(path):
    """Expand tilde and resolve relative paths"""
    expanded_path = os.path.expanduser(path)
    absolute_path = os.path.abspath(expanded_path)
    return absolute_path


def execute(path, all_dirs=False):
    """
    Fix Django migration conflicts during git rebase.

    Args:
        path (str): Path to the Django project directory
        all_dirs (bool): If True, scan all directories (slower but comprehensive)

    Returns:
        None

    Raises:
        RuntimeError: If the migration rebase operation fails
    """

    absolute_path = _expand_path(path)
    return run_rebase(absolute_path, False, all_dirs, False)

def dry_run(path, all_dirs=False, json=False):
    """
    Preview changes to the proposed fix for Django migration conflicts during git rebase.

    Args:
        path (str): Path to the Django project directory
        all_dirs (bool): If True, scan all directories (slower but comprehensive)
        json (bool): If True, output JSON instead of tables (default: False)

    Returns:
        None

    Raises:
        RuntimeError: If the migration rebase operation fails
    """
    absolute_path = _expand_path(path)
    return run_rebase(absolute_path, True, all_dirs, json)

def execute_for_app(app_path):
    """
    Fix Django migration conflicts during git rebase for a single app.

    Args:
        app_path (str): Path to the Django app directory

    Returns:
        None

    Raises:
        RuntimeError: If the migration rebase operation fails
    """
    absolute_path = _expand_path(app_path)
    return run_rebase_for_one_app(absolute_path, False, False)

def dry_run_for_app(app_path, json=False):
    """
    Preview changes to the proposed fix for Django migration conflicts during git rebase for a single app.

    Args:
        app_path (str): Path to the Django app directory
        json (bool): If True, output JSON instead of tables (default: False)

    Returns:
        None

    Raises:
        RuntimeError: If the migration rebase operation fails
    """
    absolute_path = _expand_path(app_path)
    return run_rebase_for_one_app(absolute_path, True, json)

__all__ = ['execute', 'dry_run', 'execute_for_app', 'dry_run_for_app']