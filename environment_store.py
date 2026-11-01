"""Ordered local environment rows; the legacy variables dictionary stays usable."""
import json
import re
import sqlite3
import uuid
from contextlib import closing


def clean_rows(rows):
    if not isinstance(rows,list):raise ValueError('Environment values must be a list.')
    result=[]
    for row in rows:
        if not isinstance(row,dict):raise ValueError('Each variable must contain a key and value.')
        value=row.get('value','')
        result.append({'key':str(row.get('key','')),'value':value if isinstance(value,str) else json.dumps(value,ensure_ascii=False),
            'enabled':bool(row.get('enabled',True))})
    return result


def active_values(rows):
    result={}
    for row in rows:
        key=row['key'].strip()
        if key.startswith('{{') and key.endswith('}}'):key=key[2:-2].strip()
        if key and row.get('enabled',True):result[key]=row['value']
    return result


def valid_color(value):
    value=str(value or '').lower()
    if value and not re.fullmatch(r'#[0-9a-f]{6}',value):raise ValueError('Choose a valid environment color.')
    return value


class EnvironmentStore:
    def __init__(self,store):
        self.store=store
        existing=store.all("SELECT name FROM sqlite_master WHERE name='environment_details'")
        if not existing and store.all('SELECT id FROM environments LIMIT 1'):
            backup=store.path.with_name(store.path.stem+'.before-0.13.1.sqlite3')
            if not backup.exists():
                with closing(sqlite3.connect(backup)) as target:store.db.backup(target)
        with store.atomic():
            store.db.execute('''CREATE TABLE IF NOT EXISTS environment_details (
                environment_id INTEGER PRIMARY KEY REFERENCES environments(id) ON DELETE CASCADE,
                uid TEXT NOT NULL UNIQUE,color TEXT NOT NULL DEFAULT '',rows TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0)''')
        self.sync()
    def sync(self):
        with self.store.atomic():
            position=self.store.all('SELECT COALESCE(MAX(sort_order),-1)+1 FROM environment_details')[0][0]
            for id,name,variables,details in self.store.all('''SELECT e.id,e.name,e.variables,d.rows FROM environments e
                LEFT JOIN environment_details d ON d.environment_id=e.id ORDER BY e.name COLLATE NOCASE,e.id'''):
                values=json.loads(variables)
                if details is None:
                    rows=clean_rows([{'key':key,'value':value} for key,value in values.items()])
                    self.store.db.execute('INSERT INTO environment_details(environment_id,uid,rows,sort_order) VALUES(?,?,?,?)',
                        (id,uuid.uuid4().hex,json.dumps(rows),position));position+=1
                    self.store.db.execute('UPDATE environments SET variables=? WHERE id=?',(json.dumps(active_values(rows)),id))
                elif active_values(json.loads(details))!=values:
                    rows=clean_rows([{'key':key,'value':value} for key,value in values.items()])
                    self.store.db.execute('UPDATE environment_details SET rows=? WHERE environment_id=?',(json.dumps(rows),id))
                    self.store.db.execute('UPDATE environments SET variables=? WHERE id=?',(json.dumps(active_values(rows)),id))
    def all(self):
        self.sync()
        return [dict(zip(('id','name','uid','color','sort_order'),row)) for row in self.store.all('''
            SELECT e.id,e.name,d.uid,d.color,d.sort_order FROM environments e JOIN environment_details d ON d.environment_id=e.id
            ORDER BY d.sort_order,e.id''')]
    def get(self,id):
        if id is None:
            records=self.store.all("SELECT data FROM workspace_state WHERE key='environment_globals'")
            return {'id':None,'uid':'globals','name':'Globals','color':'','rows':json.loads(records[0][0]) if records else []}
        self.sync()
        records=self.store.all('''SELECT e.id,e.name,d.uid,d.color,d.rows FROM environments e
            JOIN environment_details d ON d.environment_id=e.id WHERE e.id=?''',(id,))
        if not records:raise ValueError('This environment no longer exists.')
        data=dict(zip(('id','name','uid','color','rows'),records[0]));data['rows']=json.loads(data['rows']);return data
    def from_uid(self,uid):
        if uid=='globals':return None
        rows=self.store.all('SELECT environment_id FROM environment_details WHERE uid=?',(uid,))
        return rows[0][0] if rows else False
    def create(self,name='New Environment',rows=None,color=''):
        rows=clean_rows(rows or []);color=valid_color(color)
        with self.store.atomic():
            id=self.store.insert('environments',{'name':name.strip() or 'New Environment','variables':json.dumps(active_values(rows))})
            position=self.store.all('SELECT COALESCE(MAX(sort_order),-1)+1 FROM environment_details')[0][0]
            self.store.db.execute('INSERT INTO environment_details(environment_id,uid,color,rows,sort_order) VALUES(?,?,?,?,?)',
                (id,uuid.uuid4().hex,color,json.dumps(rows),position))
        return id
    def save(self,id,name,rows,color):
        rows=clean_rows(rows);color=valid_color(color)
        with self.store.atomic():
            if id is None:self.store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('environment_globals',?)",(json.dumps(rows),))
            else:
                if not self.store.all('SELECT id FROM environments WHERE id=?',(id,)):raise ValueError('This environment no longer exists.')
                self.store.db.execute('UPDATE environments SET name=?,variables=? WHERE id=?',(name.strip() or 'New Environment',json.dumps(active_values(rows)),id))
                self.store.db.execute('UPDATE environment_details SET color=?,rows=? WHERE environment_id=?',(color,json.dumps(rows),id))
    def move(self,id,index):
        ids=[item['id'] for item in self.all()];ids.remove(id);ids.insert(max(0,min(index,len(ids))),id)
        with self.store.atomic():
            for position,key in enumerate(ids):self.store.db.execute('UPDATE environment_details SET sort_order=? WHERE environment_id=?',(position,key))
    def duplicate(self,id):
        data=self.get(id);return self.create(data['name']+' Copy',data['rows'],data['color'])
    def delete(self,id):
        with self.store.atomic():self.store.db.execute('DELETE FROM environments WHERE id=?',(id,))
    def export_document(self,id):
        data=self.get(id)
        return {'id':data['uid'],'name':data['name'],'values':[dict(row,type='default') for row in data['rows']],
            '_postman_variable_scope':'globals' if id is None else 'environment','_reqon':{'version':1,'color':data['color']}}
    def import_document(self,document):
        if not isinstance(document,dict) or not isinstance(document.get('values'),list):raise ValueError('Select a Postman or Reqon environment JSON file.')
        rows=clean_rows(document['values']);metadata=document.get('_reqon',{})
        color=valid_color(metadata.get('color',''))
        return self.create(str(document.get('name') or 'Imported environment'),rows,color)
