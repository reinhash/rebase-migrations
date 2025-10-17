#!/usr/bin/env python3
"""
Generate a large Django project with many apps and migrations for performance testing.

Usage:
    python create_test_env.py [--apps N] [--migrations M] [--output DIR]

Example:
    python create_test_env.py --apps 1000 --migrations 50 --output test_large_project
"""

import argparse
import os
from pathlib import Path


def create_migration_file(path: Path, number: int, name: str, dependencies: list[tuple[str, str]]):
    """Create a Django migration file."""
    deps_str = ", ".join([f"('{app}', '{mig}')" for app, mig in dependencies])
    if not deps_str:
        deps_str = ""

    content = f"""from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [{deps_str}]

    operations = [
        migrations.CreateModel(
            name='Model{number:04d}',
            fields=[
                ('id', models.AutoField(primary_key=True)),
                ('name', models.CharField(max_length=100)),
                ('created_at', models.DateTimeField(auto_now_add=True)),
            ],
        ),
    ]
"""

    with open(path, 'w') as f:
        f.write(content)


def create_app_with_migrations(base_dir: Path, app_number: int, num_migrations: int):
    """Create a Django app with migrations."""
    app_name = f"app{app_number:04d}"
    app_dir = base_dir / app_name
    migrations_dir = app_dir / "migrations"
    migrations_dir.mkdir(parents=True, exist_ok=True)

    # Create __init__.py
    (migrations_dir / "__init__.py").touch()

    # Create migrations 0001 to num_migrations-2 (normal chain)
    for i in range(1, num_migrations - 1):
        migration_name = f"{i:04d}_migration_{i}"
        migration_file = migrations_dir / f"{migration_name}.py"

        # Each migration depends on the previous one
        if i == 1:
            dependencies = []
        else:
            prev_migration = f"{i-1:04d}_migration_{i-1}"
            dependencies = [(app_name, prev_migration)]

        create_migration_file(migration_file, i, migration_name, dependencies)

    # Create two conflicting migrations for the last number
    last_num = num_migrations - 1
    prev_migration = f"{last_num-1:04d}_migration_{last_num-1}"

    # HEAD migration
    head_name = f"{last_num:04d}_head_migration"
    head_file = migrations_dir / f"{head_name}.py"
    create_migration_file(head_file, last_num, head_name, [(app_name, prev_migration)])

    # Feature branch migration (to be rebased)
    feature_name = f"{last_num:04d}_feature_migration"
    feature_file = migrations_dir / f"{feature_name}.py"
    create_migration_file(feature_file, last_num, feature_name, [(app_name, prev_migration)])

    # Create max_migration.txt with conflict
    max_migration_file = migrations_dir / "max_migration.txt"
    conflict_content = f"""<<<<<<< HEAD
{head_name}
=======
{feature_name}
>>>>>>> feature-branch"""

    with open(max_migration_file, 'w') as f:
        f.write(conflict_content)

    return app_name


def main():
    parser = argparse.ArgumentParser(
        description="Generate a large Django project for performance testing"
    )
    parser.add_argument(
        "--apps",
        type=int,
        default=1000,
        help="Number of Django apps to create (default: 1000)"
    )
    parser.add_argument(
        "--migrations",
        type=int,
        default=50,
        help="Number of migrations per app (default: 50)"
    )
    parser.add_argument(
        "--output",
        type=str,
        default="test_large_project",
        help="Output directory (default: test_large_project)"
    )

    args = parser.parse_args()

    output_dir = Path(args.output)

    # Clean up existing directory
    if output_dir.exists():
        print(f"Removing existing directory: {output_dir}")
        import shutil
        shutil.rmtree(output_dir)

    output_dir.mkdir(parents=True)

    print(f"Creating {args.apps} Django apps with {args.migrations} migrations each...")
    print(f"Output directory: {output_dir.absolute()}")
    print()

    created_apps = []

    for i in range(1, args.apps + 1):
        app_name = create_app_with_migrations(output_dir, i, args.migrations)
        created_apps.append(app_name)

        # Progress indicator
        if i % 100 == 0:
            print(f"Created {i}/{args.apps} apps...")

    print()
    print(f"✓ Successfully created {len(created_apps)} Django apps")
    print(f"✓ Each app has {args.migrations} migrations (including 1 conflict)")
    print(f"✓ Total migrations: {len(created_apps) * args.migrations}")
    print()
    print("To test performance, run:")
    print(f"  time rebase-migrations --path {output_dir} --dry-run")
    print()
    print("To see CPU usage during execution:")
    print(f"  # In one terminal:")
    print(f"  watch -n 0.5 'ps aux | grep rebase-migrations'")
    print(f"  # In another terminal:")
    print(f"  rebase-migrations --path {output_dir} --dry-run")


if __name__ == "__main__":
    main()
