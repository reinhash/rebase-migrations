"""Python library for Django migration conflict resolution during git rebases."""

import os
from .rebase_migrations import run_rebase

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
    return run_rebase(absolute_path, False, all_dirs)

def dry_run(path, all_dirs=False):
    """
    Preview changes to the proposed fix for Django migration conflicts during git rebase.
    
    Args:
        path (str): Path to the Django project directory
        all_dirs (bool): If True, scan all directories (slower but comprehensive)
    
    Returns:
        None
        
    Raises:
        RuntimeError: If the migration rebase operation fails
    """
    absolute_path = _expand_path(path)
    return run_rebase(absolute_path, True, all_dirs)

__all__ = ['execute', 'dry_run']