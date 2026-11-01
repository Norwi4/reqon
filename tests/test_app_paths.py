"""Real SQLite migration fixtures; never read or modify the user's workspace."""
import json
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_paths import workspace_database
from main import Store
from environment_store import EnvironmentStore


class WorkspacePathTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.mock_base = patch('app_paths.user_data_base', return_value=self.base)
        self.mock_base.start()

    def tearDown(self):
        self.mock_base.stop()
        self.temp.cleanup()

    def legacy_workspace(self, directory=None):
        directory = directory or self.base / 'Getman/Getman'
        store = Store(directory)
        cid = store.insert('collections', {'name': 'Saved collection', 'kind': 'grpc'})
        store.save_request({'name': 'Run', 'kind': 'grpc', 'server': 'localhost:50051',
                            'message': '{"id":1}', 'metadata': [['token', 'local']]}, collection_id=cid)
        env = EnvironmentStore(store)
        eid = env.create('Local', [{'key': 'host', 'value': 'localhost', 'enabled': True}], color='#b3261e')
        session = json.dumps({'tabs': [{'kind': 'environment', 'uid': env.get(eid)['uid']}], 'environment': eid})
        store.db.execute("INSERT INTO workspace_state(key,data) VALUES('session',?)", (session,))
        store.db.execute("INSERT INTO local_drafts(uid,kind,name,data) VALUES('draft','grpc','Draft','{}')")
        store.db.execute("INSERT INTO history(method,url,status,data) VALUES('gRPC','localhost',0,'{}')")
        store.db.commit()
        dump = '\n'.join(store.db.iterdump())
        old = directory / 'getman.sqlite3'
        store.db.close()
        (directory / 'reqon.sqlite3').rename(old)
        return old, dump

    def test_default_migration_preserves_entire_workspace_and_original(self):
        old, dump = self.legacy_workspace()
        before = old.read_bytes()
        store = Store()
        try:
            self.assertEqual(store.path, self.base / 'Reqon/reqon.sqlite3')
            self.assertEqual('\n'.join(store.db.iterdump()), dump)
            self.assertEqual(old.read_bytes(), before)
            self.assertEqual(store.db.execute('PRAGMA integrity_check').fetchone()[0], 'ok')
        finally:
            store.db.close()
        self.assertEqual(workspace_database(), self.base / 'Reqon/reqon.sqlite3')

    def test_wal_changes_are_included_in_snapshot(self):
        old, _ = self.legacy_workspace()
        db = sqlite3.connect(old)
        try:
            db.execute('PRAGMA journal_mode=WAL')
            db.execute("UPDATE collections SET name='Committed WAL change'")
            db.commit()
            self.assertTrue(Path(str(old) + '-wal').exists())
            store = Store()
            try:
                self.assertEqual(store.all('SELECT name FROM collections'), [('Committed WAL change',)])
            finally:
                store.db.close()
        finally:
            db.close()

    def test_existing_reqon_data_is_never_overwritten(self):
        old, _ = self.legacy_workspace()
        first = Store()
        first.db.execute("UPDATE collections SET name='Edited in Reqon'")
        first.db.commit(); first.db.close()
        with closing(sqlite3.connect(old)) as db:
            db.execute("UPDATE collections SET name='Older app edited separately'")
            db.commit()
        second = Store()
        try:
            self.assertEqual(second.all('SELECT name FROM collections'), [('Edited in Reqon',)])
        finally:
            second.db.close()

    def test_custom_directory_migrates_only_its_own_database(self):
        old, dump = self.legacy_workspace(self.base / 'custom folder')
        target = workspace_database(old.parent)
        self.assertEqual(target, old.parent / 'reqon.sqlite3')
        with closing(sqlite3.connect(target)) as db:
            self.assertEqual('\n'.join(db.iterdump()), dump)

    def test_empty_custom_directory_does_not_import_default_workspace(self):
        self.legacy_workspace()
        store = Store(self.base / 'isolated')
        try:
            self.assertEqual(store.all('SELECT name FROM collections'), [])
        finally:
            store.db.close()

    def test_corrupt_source_does_not_publish_an_empty_replacement(self):
        old = self.base / 'Getman/Getman/getman.sqlite3'
        old.parent.mkdir(parents=True)
        old.write_bytes(b'not a SQLite database')
        with self.assertRaises(sqlite3.DatabaseError):
            workspace_database()
        self.assertFalse((self.base / 'Reqon/reqon.sqlite3').exists())
        self.assertEqual(old.read_bytes(), b'not a SQLite database')
        self.assertEqual(list((self.base / 'Reqon').iterdir()), [])

    def test_flat_legacy_directory_and_legacy_links_remain_supported(self):
        self.legacy_workspace(self.base / 'Getman')
        store = Store()
        try:
            rid = store.all('SELECT id FROM requests')[0][0]
            link = store.local_link('request', rid)
            self.assertTrue(link.startswith('reqon://'))
            self.assertEqual(store.resolve_link(link.replace('reqon://', 'getman://')), ('request', rid))
        finally:
            store.db.close()


if __name__ == '__main__':
    unittest.main()
