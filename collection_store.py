"""Local collection hierarchy and atomic operations, independent of Qt."""
import base64
import hashlib
import json
import sqlite3
import uuid
from contextlib import contextmanager, closing
from pathlib import Path

TABLES = {'collection': 'collections', 'folder': 'folders', 'request': 'requests', 'example': 'examples'}


class CollectionStore:
    def migrate_collections(self):
        self._transaction_depth = 0
        columns = {table: {r[1] for r in self.all(f'PRAGMA table_info({table})')}
                   for table in ('collections', 'folders', 'requests')}
        if 'uid' not in columns['collections']:
            backup = self.path.with_name(self.path.stem+'.before-0.6.sqlite3')
            if not backup.exists():
                with closing(sqlite3.connect(backup)) as target:
                    self.db.backup(target)
        additions = {
            'collections': {'sort_order': 'INTEGER NOT NULL DEFAULT 0', 'starred': 'INTEGER NOT NULL DEFAULT 0',
                            'expanded': 'INTEGER NOT NULL DEFAULT 0', 'description': "TEXT NOT NULL DEFAULT ''",
                            'variables': "TEXT NOT NULL DEFAULT '{}'", 'uid': "TEXT NOT NULL DEFAULT ''"},
            'folders': {'parent_id': 'INTEGER REFERENCES folders(id) ON DELETE CASCADE',
                        'sort_order': 'INTEGER NOT NULL DEFAULT 0', 'expanded': 'INTEGER NOT NULL DEFAULT 0',
                        'uid': "TEXT NOT NULL DEFAULT ''"},
            'requests': {'sort_order': 'INTEGER NOT NULL DEFAULT 0', 'expanded': 'INTEGER NOT NULL DEFAULT 0',
                         'uid': "TEXT NOT NULL DEFAULT ''"}}
        with self.db:
            for table, fields in additions.items():
                for column, definition in fields.items():
                    if column not in columns[table]:
                        self.db.execute(f'ALTER TABLE {table} ADD COLUMN {column} {definition}')
                for i, (id,) in enumerate(self.all(f'SELECT id FROM {table} ORDER BY name COLLATE NOCASE,id')):
                    self.db.execute(f'UPDATE {table} SET uid=? WHERE id=? AND uid=\'\'', (uuid.uuid4().hex, id))
                    if 'sort_order' not in columns[table]:
                        self.db.execute(f'UPDATE {table} SET sort_order=? WHERE id=?', (i, id))
            self.db.execute('''CREATE TABLE IF NOT EXISTS examples (
                id INTEGER PRIMARY KEY, request_id INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
                name TEXT NOT NULL, data TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0,
                uid TEXT NOT NULL UNIQUE)''')
            self.db.execute('CREATE INDEX IF NOT EXISTS folders_parent ON folders(collection_id,parent_id)')
            self.db.execute('CREATE INDEX IF NOT EXISTS requests_folder ON requests(collection_id,folder_id)')
            self.db.execute('''CREATE TABLE IF NOT EXISTS local_drafts (
                uid TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, data TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)''')
            self.db.execute('CREATE TABLE IF NOT EXISTS workspace_state (key TEXT PRIMARY KEY, data TEXT NOT NULL)')

    @contextmanager
    def atomic(self):
        outer = not self._transaction_depth
        self._transaction_depth += 1
        try:
            yield
            if outer: self.db.commit()
        except Exception:
            if outer: self.db.rollback()
            raise
        finally:
            self._transaction_depth -= 1

    def entity(self, kind, id):
        cursor = self.db.execute(f'SELECT * FROM {TABLES[kind]} WHERE id=?', (id,))
        row = cursor.fetchone()
        if row is None: raise ValueError('This item no longer exists.')
        return dict(zip([c[0] for c in cursor.description], row))

    def children(self, cid=None, fid=None, request_id=None):
        if request_id is not None:
            return [('example', *r) for r in self.all('SELECT id,name,sort_order FROM examples WHERE request_id=? ORDER BY sort_order,id', (request_id,))]
        if cid is None:
            return [('collection', *r) for r in self.all('SELECT id,name,sort_order FROM collections ORDER BY sort_order,id')]
        items = [('folder', *r) for r in self.all('SELECT id,name,sort_order FROM folders WHERE collection_id=? AND parent_id IS ?', (cid, fid))]
        items += [('request', *r) for r in self.all('SELECT id,name,sort_order FROM requests WHERE collection_id=? AND folder_id IS ?', (cid, fid))]
        return sorted(items, key=lambda r: (r[3], r[0], r[1]))

    def insertion_values(self, table, values):
        values = dict(values)
        if table in TABLES.values():
            values.setdefault('uid', uuid.uuid4().hex)
            if table == 'collections': siblings = self.children()
            elif table == 'examples': siblings = self.children(request_id=values['request_id'])
            else: siblings = self.children(values['collection_id'], values.get('parent_id' if table == 'folders' else 'folder_id'))
            values.setdefault('sort_order', max((r[3] for r in siblings), default=-1) + 1)
        return values

    def folder_ids(self, id):
        return [r[0] for r in self.all('''WITH RECURSIVE branch(id) AS (
            SELECT id FROM folders WHERE id=? UNION ALL
            SELECT f.id FROM folders f JOIN branch b ON f.parent_id=b.id) SELECT id FROM branch''', (id,))]

    def rename_entity(self, kind, id, name):
        name = name.strip()
        if not name: raise ValueError('Name cannot be empty.')
        with self.atomic():
            row = self.entity(kind, id)
            self.db.execute(f'UPDATE {TABLES[kind]} SET name=? WHERE id=?', (name, id))
            if kind == 'request':
                data = json.loads(row['data']); data['name'] = name
                self.db.execute('UPDATE requests SET data=? WHERE id=?', (json.dumps(data), id))

    def duplicate_entity(self, kind, id, suffix=' Copy'):
        with self.atomic():
            source = self.entity(kind, id)
            def copy_row(typ, row, **changes):
                values = {k: v for k, v in row.items() if k not in ('id', 'uid', 'sort_order')}
                values.update(changes)
                if typ == 'request':
                    data = json.loads(values['data']); data['name'] = values['name']; values['data'] = json.dumps(data)
                return self.insert(TABLES[typ], values)
            def copy_request(row, cid, fid, name=None):
                rid = copy_row('request', row, collection_id=cid, folder_id=fid, name=name or row['name'])
                for _, eid, *_ in self.children(request_id=row['id']):
                    copy_row('example', self.entity('example', eid), request_id=rid)
                return rid
            def branch(old_cid, old_fid, new_cid, new_fid):
                for typ, child, *_ in self.children(old_cid, old_fid):
                    row = self.entity(typ, child)
                    if typ == 'folder':
                        fid = copy_row(typ, row, collection_id=new_cid, parent_id=new_fid)
                        branch(old_cid, child, new_cid, fid)
                    else: copy_request(row, new_cid, new_fid)
            if kind == 'collection':
                result = copy_row(kind, source, name=source['name']+suffix, starred=0)
                branch(id, None, result, None)
            elif kind == 'folder':
                result = copy_row(kind, source, name=source['name']+suffix)
                branch(source['collection_id'], id, source['collection_id'], result)
            elif kind == 'request':
                result = copy_request(source, source['collection_id'], source['folder_id'], source['name']+suffix)
            else: result = copy_row(kind, source, name=source['name']+suffix)
            return result

    def delete_entity(self, kind, id):
        with self.atomic():
            self.entity(kind, id)
            if kind == 'folder':
                # Requests have an older ON DELETE SET NULL FK. Delete the whole branch explicitly.
                for fid in reversed(self.folder_ids(id)):
                    self.db.execute('DELETE FROM requests WHERE folder_id=?', (fid,))
                    self.db.execute('DELETE FROM folders WHERE id=?', (fid,))
            else: self.db.execute(f'DELETE FROM {TABLES[kind]} WHERE id=?', (id,))

    def reconcile_kind(self, cid):
        kinds = {json.loads(r[0]).get('kind', 'http') for r in self.all('SELECT data FROM requests WHERE collection_id=?', (cid,))}
        if kinds: self.db.execute('UPDATE collections SET kind=? WHERE id=?', ('mixed' if len(kinds) > 1 else next(iter(kinds)), cid))

    def move_entity(self, kind, id, cid=None, fid=None, before=None):
        with self.atomic():
            source = self.entity(kind, id)
            if kind not in ('collection', 'folder', 'request'): raise ValueError('Move the parent request to move its examples.')
            if kind != 'collection':
                self.entity('collection', cid)
                if fid is not None and self.entity('folder', fid)['collection_id'] != cid:
                    raise ValueError('The folder is in another collection.')
                if kind == 'folder':
                    descendants = self.folder_ids(id)
                    if fid in descendants: raise ValueError('A folder cannot be moved into itself or its subfolders.')
                    for child in descendants:
                        self.db.execute('UPDATE folders SET collection_id=? WHERE id=?', (cid, child))
                        self.db.execute('UPDATE requests SET collection_id=? WHERE folder_id=?', (cid, child))
                    self.db.execute('UPDATE folders SET parent_id=? WHERE id=?', (fid, id))
                else: self.db.execute('UPDATE requests SET collection_id=?,folder_id=? WHERE id=?', (cid, fid, id))
                self.reconcile_kind(cid)
            siblings = [(k, i) for k, i, *_ in self.children(cid if kind != 'collection' else None, fid) if (k, i) != (kind, id)]
            if before is not None and before not in siblings: raise ValueError('The destination has changed. Please try again.')
            siblings.insert(siblings.index(before) if before in siblings else len(siblings), (kind, id))
            for order, (typ, item) in enumerate(siblings):
                self.db.execute(f'UPDATE {TABLES[typ]} SET sort_order=? WHERE id=?', (order, item))

    def sort_children(self, kind, id, reverse=False):
        row = self.entity(kind, id)
        children = self.children(id if kind == 'collection' else row['collection_id'], None if kind == 'collection' else id)
        with self.atomic():
            for order, (typ, item, *_) in enumerate(sorted(children, key=lambda r: r[2].casefold(), reverse=reverse)):
                self.db.execute(f'UPDATE {TABLES[typ]} SET sort_order=? WHERE id=?', (order, item))

    def local_link(self, kind, id):
        return f"reqon://{kind}/{self.entity(kind, id)['uid']}"

    def resolve_link(self, link):
        from urllib.parse import urlparse
        url = urlparse(link.strip())
        if url.scheme not in ('reqon', 'getman') or url.netloc not in TABLES: raise ValueError('Enter a reqon:// local link.')
        found = self.all(f'SELECT id FROM {TABLES[url.netloc]} WHERE uid=?', (url.path.strip('/'),))
        if not found: raise ValueError('This item is not in this local workspace.')
        return url.netloc, found[0][0]

    def export_document(self, cid):
        collection = self.entity('collection', cid)
        native = collection['kind'] != 'http'
        def portable(data):
            data = dict(data); descriptor = data.get('descriptor_file')
            if descriptor and Path(descriptor).is_file():
                data['descriptor_base64'] = base64.b64encode(Path(descriptor).read_bytes()).decode()
                data['descriptor_file'] = ''
            return data
        def http_request(data):
            def fields(rows):
                return [{'key': k, 'value': v, 'description': rest[0] if rest else '',
                         'disabled': not rest[1] if len(rest) > 1 else False} for k, v, *rest in rows]
            req = {'method': data.get('method', 'GET'), 'url': {'raw': data.get('url', ''),
                   'query': fields(data.get('param_rows', data.get('params', [])))},
                   'header': fields(data.get('header_rows', data.get('headers', [])))}
            mode = data.get('body_type')
            if mode == 'raw': req['body'] = {'mode': 'raw', 'raw': data.get('body', '')}
            if mode == 'x-www-form-urlencoded': req['body'] = {'mode': 'urlencoded', 'urlencoded': fields(data.get('form_rows', data.get('form', [])))}
            if data.get('auth_type') == 'Bearer Token': req['auth'] = {'type': 'bearer', 'bearer': [{'key': 'token', 'value': data.get('auth_value', ''), 'type': 'string'}]}
            return req
        def branch(fid=None):
            result = []
            for typ, id, name, _ in self.children(cid, fid):
                row = self.entity(typ, id)
                if typ == 'folder':
                    result.append({'type': 'folder', 'name': name, 'items': branch(id)} if native else {'name': name, 'item': branch(id)})
                    continue
                data = portable(json.loads(row['data']))
                examples = []
                for _, eid, ename, _ in self.children(request_id=id):
                    snapshot = json.loads(self.entity('example', eid)['data'])
                    snapshot['request'] = portable(snapshot.get('request', {}))
                    examples.append({'name': ename, 'data': snapshot})
                if native: result.append({'type': 'request', 'data': data, 'examples': examples})
                else:
                    responses = []
                    for example in examples:
                        response = example['data'].get('response', {})
                        headers = response.get('headers', [])
                        if isinstance(headers, dict): headers = headers.items()
                        responses.append({'name': example['name'], 'originalRequest': http_request(example['data']['request']),
                                          'status': response.get('reason', 'OK'), 'code': response.get('status', 200),
                                          'header': [{'key': k, 'value': v} for k, v in headers], 'body': response.get('body', ''),
                                          'x-getman': example['data']})
                    result.append({'name': name, 'request': http_request(data), 'response': responses, 'x-getman': data})
            return result
        if native:
            return {'format': 'getman-collection-v1', 'kind': collection['kind'], 'name': collection['name'],
                    'description': collection['description'], 'variables': json.loads(collection['variables']), 'items': branch()}
        return {'info': {'name': collection['name'], 'description': collection['description'],
                         'schema': 'https://schema.getpostman.com/json/collection/v2.1.0/collection.json'},
                'variable': [{'key': k, 'value': v} for k, v in json.loads(collection['variables']).items()], 'item': branch()}

    def export_documents(self, collection_ids):
        ids=list(dict.fromkeys(collection_ids))
        if not ids:raise ValueError('Select at least one collection to export.')
        return {'format':'reqon-collections-v1','collections':[self.export_document(cid) for cid in ids]}

    def import_documents(self, source, default_name='Imported collection'):
        """Import a selection export atomically, or one existing collection file."""
        if not isinstance(source,dict):raise ValueError('Not a collection JSON object.')
        if source.get('format')!='reqon-collections-v1':return [self.import_document(source,default_name)]
        documents=source.get('collections')
        if not isinstance(documents,list) or not documents:raise ValueError('The collections file must contain a non-empty collections list.')
        if not all(isinstance(document,dict) for document in documents):raise ValueError('Invalid collection in the collections file.')
        with self.atomic():return [self.import_document(document,default_name) for document in documents]

    def import_document(self, source, default_name='Imported collection'):
        if not isinstance(source,dict):raise ValueError('Not a collection JSON object.')
        native = source.get('format') == 'getman-collection-v1'
        if not native and not ('info' in source and 'item' in source): raise ValueError('Not a Postman v2.1 or Reqon collection.')
        kind = source.get('kind', 'http') if native else 'http'
        if kind not in ('http', 'grpc', 'mixed'): raise ValueError('Unsupported collection type.')
        def restore(data):
            data = dict(data)
            encoded = data.pop('descriptor_base64', None)
            if encoded:
                content = base64.b64decode(encoded, validate=True)
                folder = self.path.parent / 'descriptors'; folder.mkdir(exist_ok=True)
                target = folder / (hashlib.sha256(content).hexdigest() + '.protoset')
                target.write_bytes(content); data['descriptor_file'] = str(target)
            return data
        def http_data(req, name):
            if isinstance(req, str): req = {'url': req}
            url = req.get('url', '')
            def rows(fields): return [[f.get('key', ''), str(f.get('value', '')), f.get('description', ''), not f.get('disabled', False)] for f in fields]
            headers = rows(req.get('header', [])); params = rows(url.get('query', [])) if isinstance(url, dict) else []
            body = req.get('body', {}); form = rows(body.get('urlencoded', []))
            data = {'name': name, 'method': req.get('method', 'GET'), 'url': url.get('raw', '') if isinstance(url, dict) else url,
                    'headers': [r[:3] for r in headers if r[3]], 'header_rows': headers,
                    'params': [r[:3] for r in params if r[3]], 'param_rows': params,
                    'form': [r[:3] for r in form if r[3]], 'form_rows': form,
                    'body': body.get('raw', ''), 'body_type': {'raw': 'raw', 'urlencoded': 'x-www-form-urlencoded'}.get(body.get('mode'), 'none'),
                    'auth_type': 'No Auth', 'auth_value': ''}
            auth = req.get('auth') or {}
            if auth.get('type') == 'bearer':
                data.update(auth_type='Bearer Token', auth_value=next((r.get('value', '') for r in auth.get('bearer', []) if r.get('key') == 'token'), ''))
            return data
        with self.atomic():
            info = source if native else source['info']
            variables = source.get('variables', {}) if native else {v['key']: v.get('value', '') for v in source.get('variable', []) if not v.get('disabled')}
            description = info.get('description', '') or ''
            if isinstance(description, dict): description = description.get('content', '')
            cid = self.insert('collections', {'name': info.get('name', default_name), 'kind': kind,
                             'description': description, 'variables': json.dumps(variables)})
            def branch(items, fid=None):
                for item in items:
                    if item.get('type') == 'folder' if native else 'item' in item:
                        folder = self.insert('folders', {'collection_id': cid, 'parent_id': fid, 'name': item.get('name', 'Folder')})
                        branch(item.get('items' if native else 'item', []), folder)
                    else:
                        data = restore(item['data'] if native else item.get('x-getman') or http_data(item['request'], item.get('name', 'Request')))
                        if data.get('kind', 'http') not in ('http', 'grpc'): raise ValueError('Unsupported request protocol.')
                        if native and kind != 'mixed' and data.get('kind', 'http') != kind: raise ValueError('Request protocol differs from collection protocol.')
                        rid = self.save_request(data, collection_id=cid, folder_id=fid)
                        for example in item.get('examples' if native else 'response', []):
                            snapshot = example['data'] if native else example.get('x-getman') or {
                                'request': http_data(example.get('originalRequest', item['request']), data['name']),
                                'response': {'status': example.get('code', 200), 'reason': example.get('status', 'OK'),
                                             'body': example.get('body', ''), 'headers': [(h['key'], h.get('value', '')) for h in example.get('header', [])],
                                             'duration': 0, 'size': len(example.get('body', '').encode())}}
                            snapshot = dict(snapshot); snapshot['request'] = restore(snapshot.get('request', data))
                            self.insert('examples', {'request_id': rid, 'name': example.get('name', 'Example'), 'data': json.dumps(snapshot)})
            branch(source.get('items' if native else 'item', []))
            return cid
