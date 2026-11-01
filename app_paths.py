"""Reqon workspace paths and one-time, non-destructive legacy SQLite migration."""
import os
import sqlite3
import sys
import tempfile
import time
from contextlib import closing
from pathlib import Path
from PySide6.QtCore import QLockFile

DATABASE_NAME = 'reqon.sqlite3'
LEGACY_DATABASE_NAME = 'getman.sqlite3'


def user_data_base():
    if sys.platform == 'win32':
        return Path(os.environ.get('APPDATA') or Path.home() / 'AppData/Roaming')
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Application Support'
    configured = os.environ.get('XDG_DATA_HOME', '')
    return Path(configured) if configured and Path(configured).is_absolute() else Path.home() / '.local/share'


def workspace_database(directory=None):
    base = user_data_base()
    root = Path(directory) if directory is not None else base / 'Reqon'
    root.mkdir(parents=True, exist_ok=True)
    target = root / DATABASE_NAME
    if target.exists():
        return target
    # Explicit --data-dir is isolated: never import another user's/default workspace.
    candidates = [root / LEGACY_DATABASE_NAME]
    if directory is None:
        candidates += [base / 'Getman/Getman' / LEGACY_DATABASE_NAME,
                       base / 'Getman' / LEGACY_DATABASE_NAME]
    source = next((path for path in candidates if path.is_file()), None)
    if source is None:
        return target
    lock = QLockFile(str(root / '.reqon-migration.lock'))
    if not lock.tryLock(15000):
        raise OSError('Another Reqon process is migrating this workspace. Please try again.')
    staging = None
    try:
        if target.exists():
            return target
        with tempfile.NamedTemporaryFile(prefix='.reqon-migration-', suffix='.sqlite3', dir=root, delete=False) as temp:
            staging = Path(temp.name)
        deadline = time.monotonic() + 15

        def progress(status, remaining, total):
            if time.monotonic() > deadline:
                raise TimeoutError('The previous workspace is busy. Close the previous application and try again.')

        # SQLite backup includes committed WAL contents; copying the DB file alone does not.
        with closing(sqlite3.connect(source.resolve().as_uri() + '?mode=ro', uri=True)) as old:
            with closing(sqlite3.connect(staging)) as new:
                old.backup(new, pages=256, progress=progress, sleep=.05)
                if new.execute('PRAGMA quick_check').fetchall() != [('ok',)]:
                    raise sqlite3.DatabaseError('The previous workspace failed its integrity check.')
        # Publish only the completed database. Keep the old workspace for recovery.
        os.replace(staging, target)
        return target
    finally:
        if staging is not None:
            staging.unlink(missing_ok=True)
        lock.unlock()
