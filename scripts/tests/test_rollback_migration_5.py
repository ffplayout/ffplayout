import hashlib
import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("rollback_migration_5", ROOT / "scripts/rollback_migration_5.py")
rollback = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rollback)
ORIGINAL_SQL = Path(__file__).parent / "fixtures/00005_original.sql"


def schema(connection):
    tables = connection.execute("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").fetchall()
    return (
        {name: connection.execute(f'PRAGMA table_info("{name}")').fetchall() for (name,) in tables},
        connection.execute("SELECT name, type, tbl_name FROM sqlite_master WHERE type IN ('index', 'trigger') ORDER BY name").fetchall(),
    )


class RollbackTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.database = Path(self.directory.name) / "ffplayout.db"
        connection = sqlite3.connect(self.database)
        connection.executescript("""
            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            );
        """)
        for version in range(1, 5):
            sql = next((ROOT / "migrations").glob(f"{version:05d}_*.sql")).read_bytes()
            connection.executescript(sql.decode())
            connection.execute("INSERT INTO _sqlx_migrations VALUES (?, 'fixture', CURRENT_TIMESTAMP, 1, ?, 0)",
                               (version, hashlib.sha384(sql).digest()))
        connection.commit()
        self.schema4 = schema(connection)
        old_sql = ORIGINAL_SQL.read_bytes()
        self.assertEqual(hashlib.sha384(old_sql).digest(), rollback.ORIGINAL_CHECKSUM)
        connection.executescript(old_sql.decode())
        connection.execute("INSERT INTO _sqlx_migrations VALUES (5, 'output metadata and live listeners', CURRENT_TIMESTAMP, 1, ?, 0)",
                           (rollback.ORIGINAL_CHECKSUM,))
        connection.commit()
        self.schema5 = schema(connection)
        connection.close()

    def read(self, sql):
        connection = sqlite3.connect(self.database)
        try:
            return connection.execute(sql).fetchall()
        finally:
            connection.close()

    def test_rollback_restores_schema4_and_backs_up_original_database(self):
        backup = rollback.reset_database(self.database)
        self.assertTrue(backup.is_file())
        connection = sqlite3.connect(self.database)
        self.assertEqual(schema(connection), self.schema4)
        connection.close()
        self.assertEqual(self.read("SELECT MAX(version) FROM _sqlx_migrations"), [(4,)])
        connection = sqlite3.connect(backup)
        self.assertEqual(schema(connection), self.schema5)
        connection.close()
        self.assertEqual(self.read("SELECT priority FROM config_live_input WHERE id = 1"), [(100,)])

    def test_dry_run_does_not_change_database_or_create_backup(self):
        self.assertIsNone(rollback.reset_database(self.database, dry_run=True))
        self.assertEqual(self.read("SELECT MAX(version) FROM _sqlx_migrations"), [(5,)])
        self.assertEqual(list(self.database.parent.glob("*.before-migration*")), [])

    def test_non_default_options_require_reapplication(self):
        connection = sqlite3.connect(self.database)
        connection.execute("UPDATE config_output SET metadata_options = '{\"title\":\"My channel\"}'")
        connection.commit()
        connection.close()
        with self.assertRaisesRegex(ValueError, "use --reapply"):
            rollback.reset_database(self.database)
        self.assertEqual(list(self.database.parent.glob("*.before-migration*")), [])
        self.assertEqual(self.read("SELECT MAX(version) FROM _sqlx_migrations"), [(5,)])

    def test_reapply_preserves_options_multiple_listeners_and_loudness_values(self):
        connection = sqlite3.connect(self.database)
        connection.execute("UPDATE config_output SET metadata_options = '{\"title\":\"My channel\"}'")
        connection.execute("UPDATE config_live_input SET demuxer_options = '{\"probesize\":\"65536\"}'")
        connection.execute("INSERT INTO config_live_input(config_id, backend, takeover_mode, priority, identifier) VALUES (1, 'rtmp', 'connection', 80, 'rtmp://127.0.0.1:1937/live/stream')")
        connection.execute("UPDATE config_audio SET live_loudness_enable = 1, live_loudness_target_lufs = -19.0")
        connection.commit()
        connection.close()
        rollback.reset_database(self.database, rollback.DEFAULT_MIGRATION)
        self.assertEqual(self.read("SELECT loudness_scope, loudness_target_lufs FROM config_audio WHERE config_id = 1"), [("live", -19.0)])
        self.assertEqual(self.read("SELECT metadata_options FROM config_output WHERE id = 1"), [('{"title":"My channel"}',)])
        self.assertEqual(self.read("SELECT demuxer_options FROM config_live_input WHERE id = 1"), [('{"probesize":"65536"}',)])
        self.assertEqual(self.read("SELECT priority FROM config_live_input ORDER BY id"), [(100,), (80,)])
        self.assertEqual(self.read("SELECT description FROM _sqlx_migrations WHERE version = 5"),
                         [("output live inputs and audio processing",)])
        expected = hashlib.sha384(rollback.DEFAULT_MIGRATION.read_bytes()).digest()
        self.assertEqual(self.read("SELECT checksum FROM _sqlx_migrations WHERE version = 5"), [(expected,)])
        with self.assertRaisesRegex(ValueError, "not the original"):
            rollback.reset_database(self.database)

    def test_backup_includes_committed_wal_data(self):
        connection = sqlite3.connect(self.database)
        self.addCleanup(connection.close)
        connection.execute("PRAGMA journal_mode = WAL")
        connection.execute("UPDATE config_audio SET live_loudness_target_lufs = -17.0")
        connection.commit()
        self.assertTrue(Path(str(self.database) + "-wal").exists())
        backup = rollback.reset_database(self.database, rollback.DEFAULT_MIGRATION)
        saved = sqlite3.connect(backup)
        self.addCleanup(saved.close)
        self.assertEqual(saved.execute("SELECT live_loudness_target_lufs FROM config_audio WHERE config_id = 1").fetchall(), [(-17.0,)])
        self.assertEqual(self.read("SELECT loudness_target_lufs FROM config_audio WHERE config_id = 1"), [(-17.0,)])

    def test_failed_reapplication_rolls_back_all_schema_changes(self):
        invalid = self.database.parent / "invalid.sql"
        invalid.write_bytes(rollback.DEFAULT_MIGRATION.read_bytes() + b"\nSELECT * FROM missing_table;\n")
        with self.assertRaises(sqlite3.OperationalError):
            rollback.reset_database(self.database, invalid)
        connection = sqlite3.connect(self.database)
        self.assertEqual(schema(connection), self.schema5)
        connection.close()
        self.assertEqual(self.read("SELECT checksum FROM _sqlx_migrations WHERE version = 5"), [(rollback.ORIGINAL_CHECKSUM,)])

    def test_other_migration_versions_are_rejected_without_changes(self):
        connection = sqlite3.connect(self.database)
        connection.execute("INSERT INTO _sqlx_migrations VALUES (6, 'fixture', CURRENT_TIMESTAMP, 1, X'00', 0)")
        connection.commit()
        connection.close()
        with self.assertRaisesRegex(ValueError, "1 through 5 only"):
            rollback.reset_database(self.database)
        self.assertEqual(self.read("SELECT MAX(version) FROM _sqlx_migrations"), [(6,)])
        self.assertEqual(list(self.database.parent.glob("*.before-migration*")), [])


if __name__ == "__main__":
    unittest.main()
