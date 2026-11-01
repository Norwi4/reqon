"""Local application preferences and request defaults. No process-wide network state."""
import json
from dataclasses import dataclass,asdict,replace
from pathlib import Path


@dataclass(frozen=True)
class GeneralConfig:
    http_version:str='HTTP/1.x'
    timeout_ms:int=0
    max_response_mb:int=50
    verify_ssl:bool=False
    tls_keylog:bool=False
    disable_cookies:bool=False
    request_validation:bool=True
    response_format:str='Auto'
    working_directory:str=''
    read_outside:bool=False
    no_cache:bool=True
    postman_token:bool=True
    retain_headers:bool=False
    follow_redirects:bool=True
    new_tab:bool=False
    ask_unsaved:bool=False
    tab_icons:bool=True
    two_pane:bool=False
    autocomplete:bool=True
    documentation_editor:str='Reqon editor'
    font_family:str='JetBrains Mono'
    font_size:int=13
    indent_count:int=4
    indent_type:str='Space'
    close_brackets:bool=True
    close_quotes:bool=True
    language:str='English'
    autosave:bool=True

    def validate(self):
        defaults=GeneralConfig()
        for key,value in asdict(self).items():
            if type(value) is not type(getattr(defaults,key)):raise ValueError('Invalid setting: '+key)
        for key,choices in [('http_version',('Auto','HTTP/1.x','HTTP/2')),('response_format',('Auto','JSON')),
                ('indent_type',('Space','Tab')),('language',('English','Русский')),('documentation_editor',('Reqon editor','Markdown editor'))]:
            if getattr(self,key) not in choices:raise ValueError('Invalid setting: '+key)
        for key,low,high in [('timeout_ms',0,2147483647),('max_response_mb',0,2047),('font_size',8,40),('indent_count',1,16)]:
            if not low<=getattr(self,key)<=high:raise ValueError('Invalid setting: '+key)
        if not self.font_family.strip():raise ValueError('Enter an editor font family.')

    @property
    def indent(self):return '\t' if self.indent_type=='Tab' else ' '*self.indent_count


class GeneralPreferences:
    def __init__(self,store):
        self.store=store;self.config=GeneralConfig(working_directory=str(store.path.parent/'files'));self.error=''
        rows=store.all("SELECT data FROM workspace_state WHERE key='general'")
        if rows:
            try:
                values=json.loads(rows[0][0]);config=replace(self.config,**{k:v for k,v in values.items() if k in GeneralConfig.__dataclass_fields__})
                config.validate();self.config=config
            except (ValueError,TypeError,AttributeError):self.error='Saved General settings could not be read. Defaults are active.'
    def save(self,config):
        config.validate()
        with self.store.atomic():self.store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('general',?)",(json.dumps(asdict(config),ensure_ascii=False),))
        self.config=config;self.error=''


def resolve_file(name,config):
    root=Path(config.working_directory).expanduser().resolve()
    path=Path(name).expanduser();path=(path if path.is_absolute() else root/path).resolve()
    if not config.read_outside and not path.is_relative_to(root):
        raise ValueError('File is outside the working directory. Select a file inside it or enable Read files outside working directory in General.')
    if not path.is_file():raise ValueError('Request file does not exist: '+str(path))
    return path


def effective_request(data,config,storage):
    result=dict(data,_general=config)
    if not data.get('general_overrides',False):
        result.update(http_timeout=config.timeout_ms/1000,http_verify=config.verify_ssl,http_follow_redirects=config.follow_redirects,http_disable_cookies=config.disable_cookies,
            http_version=config.http_version,tls_verify=config.verify_ssl,grpc_timeout_ms=config.timeout_ms,grpc_max_response_mb=config.max_response_mb)
    else:result.setdefault('http_version',config.http_version)
    if config.tls_keylog:
        directory=Path(storage)/'logs';directory.mkdir(parents=True,exist_ok=True);result['_tls_keylog']=str(directory/'tls-keys.log')
    return result
