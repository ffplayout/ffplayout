#!/usr/bin/env python3
"""Roll back migration 7 on a stopped ffplayout installation.

Requires Python 3 and SQLite >= 3.35. Creates a consistent SQLite backup beside
its database before changing anything, keeping at most five schema backups.

The default rollback returns to schema 6 and removes migration 7's settings.
Output rows remain, but schema 6 allows only one active output per channel.
Use --reapply to reapply the repository's migration 7 in the same transaction,
refresh its SQLx checksum and preserve all migration 7 settings.

Examples (stop ffplayout first):
    python3 scripts/rollback_migration_7.py /path/to/ffplayout.db --dry-run
    python3 scripts/rollback_migration_7.py /path/to/ffplayout.db
    python3 scripts/rollback_migration_7.py /path/to/ffplayout.db --reapply
"""

import argparse
from contextlib import closing
from datetime import date
import hashlib
import os
from pathlib import Path
import re
import sqlite3
import time
import uuid


DEFAULT_MIGRATION = (
    Path(__file__).resolve().parent.parent / "migrations/00007_output_instances.sql"
)
OUTPUT_COLUMNS = (
    "priority", "configured", "label", "inherit_resolution", "share_encoding",
    "encoding_source_id", "encoding_variant", "public_moq",
)


def execute_sql(connection, sql):
    """Keep statements, including triggers, inside the caller's transaction."""
    statement = ""
    for line in sql.splitlines(keepends=True):
        statement += line
        if sqlite3.complete_statement(statement):
            connection.execute(statement)
            statement = ""
    if statement.strip():
        raise ValueError("Incomplete SQL statement")


def check_integrity(connection):
    if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
        raise ValueError("Database integrity check failed")
    if connection.execute("PRAGMA foreign_key_check").fetchall():
        raise ValueError("Database contains foreign-key violations")


def validate_database(connection, reapply):
    rows = connection.execute(
        "SELECT version, success FROM _sqlx_migrations ORDER BY version"
    ).fetchall()
    if rows != [(version, 1) for version in range(1, 8)]:
        raise ValueError("Expected successful migrations 1 through 7 only")
    columns = {row[1] for row in connection.execute("PRAGMA table_info(config_output)")}
    if not set(OUTPUT_COLUMNS).issubset(columns):
        raise ValueError("Output schema does not match the supported migration 7")
    if "program_format" not in {
        row[1] for row in connection.execute("PRAGMA table_info(config)")
    }:
        raise ValueError("Missing config.program_format from migration 7")
    check_integrity(connection)
    if not reapply:
        duplicates = connection.execute(
            "SELECT config_id FROM config_output WHERE active = 1 "
            "GROUP BY config_id HAVING COUNT(*) > 1"
        ).fetchall()
        if duplicates:
            raise ValueError(
                "Schema 6 allows only one active output per channel. "
                "Disable additional outputs first, or use --reapply. "
                f"Affected configuration IDs: {[row[0] for row in duplicates]}"
            )
        if connection.execute(
            "SELECT 1 FROM config_output WHERE active = 1 "
            "AND encoding_source_id IS NOT NULL LIMIT 1"
        ).fetchone():
            raise ValueError(
                "An active output uses shared encoding, which schema 6 cannot represent. "
                "Select independent encoding first, or use --reapply."
            )


def create_backup(database):
    backup = database.with_name(f"{database.name}.{date.today()}.schema-7.bak")
    temporary = database.with_name(f".{database.name}.{uuid.uuid4().hex}.tmp")
    descriptor = os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(descriptor)
    try:
        # The caller holds BEGIN IMMEDIATE. Use a separate read connection to
        # capture committed WAL data without backing up an active write transaction.
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=5)) as source:
            destination = sqlite3.connect(temporary)
            try:
                source.backup(destination)
                check_integrity(destination)
            finally:
                destination.close()
        with temporary.open("rb") as file:
            os.fsync(file.fileno())
        os.replace(temporary, backup)
    finally:
        temporary.unlink(missing_ok=True)
    pattern = re.compile(
        re.escape(database.name) + r"\.(\d{4}-\d{2}-\d{2})\.schema-\d+\.bak"
    )
    older = []
    for path in database.parent.iterdir():
        match = pattern.fullmatch(path.name)
        if path == backup or not match or path.is_symlink() or not path.is_file():
            continue
        try:
            date.fromisoformat(match[1])
        except ValueError:
            continue
        older.append(path)
    older.sort(key=lambda path: (path.stat().st_mtime_ns, path.name), reverse=True)
    for path in older[4:]:
        path.unlink()
    return backup


def reset_database(database, reapply=None, dry_run=False):
    database = Path(database).resolve(strict=True)
    if not database.is_file():
        raise ValueError("Database path must name an existing file")
    if sqlite3.sqlite_version_info < (3, 35, 0):
        raise ValueError("SQLite 3.35 or newer is required for DROP COLUMN")
    migration_sql = Path(reapply).read_bytes().decode("utf-8") if reapply is not None else None
    connection = sqlite3.connect(database.as_uri() + "?mode=rw", uri=True, timeout=5)
    try:
        connection.execute("PRAGMA foreign_keys = ON")
        connection.execute("BEGIN IMMEDIATE")
        validate_database(connection, migration_sql is not None)
        settings = connection.execute(
            f"SELECT id, {', '.join(OUTPUT_COLUMNS)} FROM config_output"
        ).fetchall()
        formats = connection.execute("SELECT id, program_format FROM config").fetchall()
        backup = None
        if not dry_run:
            backup = create_backup(database)
            print(f"Backup: {backup}", flush=True)
        connection.execute("DROP INDEX config_output_active")
        for column in OUTPUT_COLUMNS:
            connection.execute(f"ALTER TABLE config_output DROP COLUMN {column}")
        connection.execute("ALTER TABLE config DROP COLUMN program_format")
        connection.execute("DELETE FROM _sqlx_migrations WHERE version = 7")
        # --reapply drops this transitional index before committing, allowing
        # the multiple active outputs that migration 7 introduced.
        kind = "INDEX" if migration_sql is not None else "UNIQUE INDEX"
        connection.execute(
            f"CREATE {kind} config_output_one_active "
            "ON config_output(config_id) WHERE active = 1"
        )
        if migration_sql is not None:
            started = time.monotonic_ns()
            execute_sql(connection, migration_sql)
            assignments = ', '.join(f"{column} = ?" for column in OUTPUT_COLUMNS)
            connection.executemany(
                f"UPDATE config_output SET {assignments} WHERE id = ?",
                [(*row[1:], row[0]) for row in settings],
            )
            connection.executemany(
                "UPDATE config SET program_format = ? WHERE id = ?",
                [(value, identifier) for identifier, value in formats],
            )
            description = Path(reapply).stem.partition("_")[2].replace("_", " ")
            connection.execute(
                "INSERT INTO _sqlx_migrations "
                "(version, description, success, checksum, execution_time) VALUES (7, ?, 1, ?, ?)",
                (description, hashlib.sha384(migration_sql.encode("utf-8")).digest(),
                 time.monotonic_ns() - started),
            )
        check_integrity(connection)
        if dry_run:
            connection.rollback()
        else:
            connection.commit()
        return backup
    except Exception:
        connection.rollback()
        raise
    finally:
        connection.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("database", type=Path, help="Existing database; stop ffplayout first")
    parser.add_argument("--dry-run", action="store_true", help="Test the complete transaction and roll it back; no backup")
    parser.add_argument("--reapply", nargs="?", const=DEFAULT_MIGRATION, type=Path,
                        help="Reapply migration 7 and preserve its settings (optional SQL file path)")
    args = parser.parse_args()
    try:
        reset_database(args.database, args.reapply, args.dry_run)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Error: {error}\n")
    if args.dry_run:
        print("Transaction validated and rolled back. No backup created.")
    elif args.reapply is not None:
        print("Migration 7 reapplied; output settings and program format preserved.")
    else:
        print("Migration 7 rolled back to schema 6; migration 7 settings removed.")
        print("Starting the current ffplayout version will apply migration 7 again.")


if __name__ == "__main__":
    main()
