"""Theme preferences are local workspace settings, independent of requests."""
import json
from dataclasses import dataclass,asdict,replace
from ui_palettes import THEMES


@dataclass(frozen=True)
class ThemeConfig:
    mode:str='manual'
    manual:str='dark'
    day:str='light'
    night:str='dark'
    def validate(self):
        if self.mode not in ('manual','system'):raise ValueError('Choose Manual or Sync with system.')
        for value in (self.manual,self.day,self.night):
            if not isinstance(value,str) or value not in THEMES:raise ValueError('Unknown theme selection.')
    def effective(self,system_dark):return self.manual if self.mode=='manual' else self.night if system_dark else self.day


class ThemePreferences:
    def __init__(self,store):
        self.store=store;self.config=ThemeConfig();self.error=''
        rows=store.all("SELECT data FROM workspace_state WHERE key='theme'")
        if rows:
            try:
                values=json.loads(rows[0][0]);config=replace(self.config,**{key:value for key,value in values.items() if key in ThemeConfig.__dataclass_fields__});config.validate();self.config=config
            except (ValueError,TypeError,AttributeError):self.error='Saved theme settings could not be read. Dark theme is active.'
    def save(self,config):
        config.validate()
        with self.store.atomic():self.store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('theme',?)",(json.dumps(asdict(config)),))
        self.config=config;self.error=''
