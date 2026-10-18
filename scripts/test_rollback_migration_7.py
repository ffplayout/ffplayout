"""Run with: python3 -m unittest discover -s scripts -p test_rollback_migration_7.py"""

from contextlib import closing
from datetime import date
import hashlib
from pathlib import Path
import sqlite3
import tempfile
import unittest

from rollback_migration_7 import DEFAULT_MIGRATION, reset_database


class RollbackMigration7Tests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="rollback-migration-7-")
        self.addCleanup(self.directory.cleanup)
        self.database = Path(self.directory.name) / "ffplayout.db"
        self.connection = sqlite3.connect(self.database)
        self.addCleanup(self.connection.close)
        self.connection.execute("PRAGMA foreign_keys = ON")
        self.connection.execute("PRAGMA journal_mode = WAL")
        self.connection.execute("PRAGMA wal_autocheckpoint = 0")
        self.connection.execute(
            "CREATE TABLE _sqlx_migrations(version BIGINT PRIMARY KEY, "
            "description TEXT NOT NULL, installed_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP, "
            "success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)"
        )
        for migration in sorted(DEFAULT_MIGRATION.parent.glob("*.sql")):
            version = int(migration.name.split('_', 1)[0])
            if version > 7:
                continue
            sql = migration.read_text()
            self.connection.executescript(sql)
            self.connection.execute(
                "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) "
                "VALUES (?, ?, 1, ?, 0)",
                (version, migration.stem.partition('_')[2], hashlib.sha384(sql.encode()).digest()),
            )
            self.connection.commit()
        self.output, self.config = self.connection.execute(
            "SELECT id, config_id FROM config_output WHERE active = 1 LIMIT 1"
        ).fetchone()
        self.connection.execute("UPDATE config_output SET label = 'Keep me', public_moq = 1 WHERE id = ?", (self.output,))
        self.connection.execute("UPDATE config SET program_format = 'custom program' WHERE id = ?", (self.config,))
        self.connection.commit()

    def snapshot(self):
        return list(self.connection.iterdump())

    def test_dry_run_preserves_database_and_creates_no_backup(self):
        before = self.snapshot()
        self.assertIsNone(reset_database(self.database, dry_run=True))
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(list(self.database.parent.glob("*.bak")), [])

    def test_rollback_restores_schema6_and_backup_includes_wal_data(self):
        backup = reset_database(self.database)
        self.assertEqual(backup.name, f"ffplayout.db.{date.today()}.schema-7.bak")
        self.assertEqual(self.connection.execute("SELECT MAX(version) FROM _sqlx_migrations").fetchone(), (6,))
        columns = {row[1] for row in self.connection.execute("PRAGMA table_info(config_output)")}
        self.assertNotIn("priority", columns)
        index = self.connection.execute("SELECT sql FROM sqlite_master WHERE name = 'config_output_one_active'").fetchone()[0]
        self.assertIn("UNIQUE", index)
        with closing(sqlite3.connect(backup)) as saved:
            self.assertEqual(saved.execute("SELECT MAX(version) FROM _sqlx_migrations").fetchone(), (7,))
            self.assertEqual(saved.execute("SELECT label FROM config_output WHERE id = ?", (self.output,)).fetchone(), ("Keep me",))
            self.assertEqual(saved.execute("PRAGMA integrity_check").fetchone(), ("ok",))

    def test_multiple_active_outputs_refuse_plain_rollback_but_reapply_preserves_everything(self):
        other = self.connection.execute("SELECT id FROM config_output WHERE config_id = ? AND id != ? LIMIT 1", (self.config, self.output)).fetchone()[0]
        self.connection.execute("UPDATE config_output SET active = 1, encoding_source_id = ? WHERE id = ?", (self.output, other))
        self.connection.commit()
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "only one active"):
            reset_database(self.database)
        self.assertEqual(self.snapshot(), before)
        reset_database(self.database, reapply=DEFAULT_MIGRATION, dry_run=True)
        self.assertEqual(self.snapshot(), before)
        reset_database(self.database, reapply=DEFAULT_MIGRATION)
        self.assertEqual(self.connection.execute("SELECT COUNT(*) FROM config_output WHERE config_id = ? AND active = 1", (self.config,)).fetchone(), (2,))
        self.assertEqual(self.connection.execute("SELECT encoding_source_id FROM config_output WHERE id = ?", (other,)).fetchone(), (self.output,))
        self.assertEqual(self.connection.execute("SELECT label, public_moq FROM config_output WHERE id = ?", (self.output,)).fetchone(), ("Keep me", 1))
        self.assertEqual(self.connection.execute("SELECT program_format FROM config WHERE id = ?", (self.config,)).fetchone(), ("custom program",))
        self.assertEqual(self.connection.execute("SELECT checksum FROM _sqlx_migrations WHERE version = 7").fetchone(), (hashlib.sha384(DEFAULT_MIGRATION.read_bytes()).digest(),))

    def test_invalid_reapply_rolls_back_every_change(self):
        migration = self.database.parent / "00007_invalid.sql"
        migration.write_text("INVALID SQL;\n")
        before = self.snapshot()
        with self.assertRaises(sqlite3.Error):
            reset_database(self.database, reapply=migration)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(len(list(self.database.parent.glob("*.bak"))), 1)

    def test_backup_failure_prevents_rollback(self):
        (self.database.parent / f"ffplayout.db.{date.today()}.schema-7.bak").mkdir()
        before = self.snapshot()
        with self.assertRaises(OSError):
            reset_database(self.database)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(list(self.database.parent.glob(".*.tmp")), [])

    def test_backup_retention_leaves_five_and_preserves_manual_files(self):
        for schema in range(7):
            (self.database.parent / f"ffplayout.db.2026-01-01.schema-{schema}.bak").write_text("older")
        manual = self.database.parent / "ffplayout.db.manual.bak"
        manual.write_text("keep")
        reset_database(self.database)
        self.assertEqual(len(list(self.database.parent.glob("*.bak"))), 6)
        self.assertEqual(manual.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
