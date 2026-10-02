#!/usr/bin/env python3
"""Reset the original migration 5 on a stopped ffplayout test installation.

Requires Python 3 and SQLite >= 3.35. An SQLite backup is always created before
changes. By default the database returns to schema 4. With --reapply, the merged
migration 5 is applied in the same transaction and metadata/demuxer options are
preserved. Listener priorities remain unchanged: their original values cannot
be reconstructed after the original migration normalized them.

Examples (stop ffplayout first):
    python3 scripts/rollback_migration_5.py /path/to/ffplayout.db --dry-run
    python3 scripts/rollback_migration_5.py /path/to/ffplayout.db --reapply
"""

import argparse
from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import sqlite3
import time
import uuid


ORIGINAL_CHECKSUM = bytes.fromhex(
    "f902008b9897f771e4b0a2eb3e4a197194e0df5ce0f9c7573dc588295f7df8b"
    "751dd8c233c0eec520b259a4c2cab1dbc"
)
DEFAULT_MIGRATION = (
    Path(__file__).resolve().parent.parent
    / "migrations/00005_output_live_inputs_and_audio_processing.sql"
)
ROLLBACK_SQL = """
DROP INDEX config_output_audio_one_default;
DROP INDEX config_live_input_channel_order;
DROP TRIGGER config_live_input_priority_insert;
DROP TRIGGER config_live_input_priority_update;
ALTER TABLE config_output DROP COLUMN metadata_options;
ALTER TABLE config_live_input DROP COLUMN demuxer_options;
DELETE FROM _sqlx_migrations WHERE version = 5;
"""


def execute_sql(connection, sql):
    """Execute SQL including triggers without executescript's implicit commit."""
    statement = ""
    for line in sql.splitlines(keepends=True):
        statement += line
        if sqlite3.complete_statement(statement):
            connection.execute(statement)
            statement = ""
    if statement.strip():
        raise ValueError("Migration contains an incomplete SQL statement")


def validate_original_database(connection):
    rows = connection.execute(
        "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version"
    ).fetchall()
    if [row[0] for row in rows] != [1, 2, 3, 4, 5]:
        raise ValueError("Expected successful migrations 1 through 5 only; database left unchanged")
    if not all(row[1] == 1 for row in rows) or rows[-1][2] != ORIGINAL_CHECKSUM:
        raise ValueError("Migration 5 is not the original version; database left unchanged")
    if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
        raise ValueError("Database integrity check failed")
    if connection.execute("PRAGMA foreign_key_check").fetchall():
        raise ValueError("Database contains foreign-key violations")
    audio_columns = {
        row[1] for row in connection.execute("PRAGMA table_info(config_audio)")
    }
    if "live_loudness_enable" not in audio_columns or "loudness_scope" in audio_columns:
        raise ValueError("Audio schema does not match the original migration 5")


def create_backup(database):
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    backup = database.with_name(
        f"{database.name}.before-migration-5-rollback.{timestamp}.{uuid.uuid4().hex[:8]}.sqlite"
    )
    descriptor = os.open(backup, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(descriptor)
    try:
        # The caller holds a write reservation, so the read connection captures
        # the unchanged database, including committed data in its WAL.
        source = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=1)
        try:
            destination = sqlite3.connect(backup)
            try:
                source.backup(destination)
            finally:
                destination.close()
        finally:
            source.close()
    except Exception:
        backup.unlink(missing_ok=True)
        raise
    return backup


def reset_database(database, reapply=None, dry_run=False):
    database = Path(database).resolve(strict=True)
    if not database.is_file():
        raise ValueError("Database path must name an existing file")
    if sqlite3.sqlite_version_info < (3, 35, 0):
        raise ValueError("SQLite 3.35 or newer is required for DROP COLUMN")
    migration_sql = None
    if reapply is not None:
        migration_sql = Path(reapply).read_bytes().decode("utf-8")
        if "ADD COLUMN loudness_scope" not in migration_sql or "TO loudness_target_lufs" not in migration_sql:
            raise ValueError("--reapply must name the merged migration 5 SQL file")
    connection = sqlite3.connect(database.as_uri() + "?mode=rw", uri=True, timeout=1)
    try:
        connection.execute("PRAGMA foreign_keys = ON")
        connection.execute("BEGIN IMMEDIATE")
        validate_original_database(connection)
        metadata = connection.execute("SELECT id, metadata_options FROM config_output").fetchall()
        demuxer = connection.execute("SELECT id, demuxer_options FROM config_live_input").fetchall()
        duplicate_listeners = connection.execute(
            "SELECT config_id FROM config_live_input "
            "WHERE backend = 'rtmp' AND takeover_mode = 'connection' "
            "GROUP BY config_id HAVING COUNT(*) > 1"
        ).fetchall()
        if migration_sql is None:
            if duplicate_listeners:
                raise ValueError("Schema 4 cannot hold multiple RTMP listeners; use --reapply")
            if any(value.strip() != "{}" for _, value in metadata + demuxer):
                raise ValueError("Custom metadata/demuxer options would be lost; use --reapply to preserve them")
        if dry_run:
            connection.rollback()
            return None
        backup = create_backup(database)
        print(f"Backup: {backup}", flush=True)
        description = DEFAULT_MIGRATION.stem.partition("_")[2].replace("_", " ")
        execute_sql(connection, ROLLBACK_SQL)
        # The original unique index is incompatible with multiple listeners.
        # Reapplication drops this temporary index before anything is committed.
        index_kind = "INDEX" if duplicate_listeners else "UNIQUE INDEX"
        connection.execute(
            f"CREATE {index_kind} config_live_input_rtmp_listener "
            "ON config_live_input(config_id) "
            "WHERE backend = 'rtmp' AND takeover_mode = 'connection'"
        )
        if migration_sql is not None:
            started = time.monotonic_ns()
            execute_sql(connection, migration_sql)
            connection.executemany(
                "UPDATE config_output SET metadata_options = ? WHERE id = ?",
                [(value, identifier) for identifier, value in metadata],
            )
            connection.executemany(
                "UPDATE config_live_input SET demuxer_options = ? WHERE id = ?",
                [(value, identifier) for identifier, value in demuxer],
            )
            connection.execute(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) "
                "VALUES (5, ?, 1, ?, ?)",
                (description, hashlib.sha384(migration_sql.encode("utf-8")).digest(), time.monotonic_ns() - started),
            )
        if connection.execute("PRAGMA foreign_key_check").fetchall():
            raise ValueError("Foreign-key check failed; changes rolled back")
        if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise ValueError("Integrity check failed; changes rolled back")
        connection.commit()
        return backup
    except Exception:
        connection.rollback()
        raise
    finally:
        connection.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("database", type=Path, help="Existing SQLite database; stop ffplayout first")
    parser.add_argument("--dry-run", action="store_true", help="Validate without changing the database or creating a backup")
    parser.add_argument("--reapply", nargs="?", const=DEFAULT_MIGRATION, type=Path,
                        help="Reapply merged migration 5 and preserve settings (optional SQL file path)")
    args = parser.parse_args()
    try:
        reset_database(args.database, args.reapply, args.dry_run)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Error: {error}\n")
    if args.dry_run:
        print("Checks passed. No changes made.")
    elif args.reapply is not None:
        print("Merged migration 5 applied; metadata/demuxer options and listener priorities preserved.")
    else:
        print("Migration 5 rolled back. The next ffplayout start will apply the merged migration 5.")
        print("Listener priorities retained; their values before the original migration 5 are unknown.")


if __name__ == "__main__":
    main()
