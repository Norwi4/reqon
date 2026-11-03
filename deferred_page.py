"""A saved, untouched request tab whose editor is created when first selected."""
from PySide6.QtWidgets import QWidget
import json


class DeferredPage(QWidget):
    busy = False

    def __init__(self, window, entry, name, kind, request_id=None, collection_id=None, folder_id=None):
        super().__init__()
        self.window=window
        self.session_entry=dict(entry)
        self.title=name
        self.kind=kind
        self.request_id=request_id
        self.collection_id=collection_id
        self.folder_id=folder_id
        if entry['kind']=='draft':self._draft_id=entry['uid']

    def collect(self):
        # Reading an untouched tab must not change the user's current selection.
        if self.request_id:
            return json.loads(self.window.store.entity('request',self.request_id)['data'])
        rows=self.window.store.all('SELECT data FROM local_drafts WHERE uid=?',(self._draft_id,))
        return json.loads(rows[0][0]) if rows else {}
