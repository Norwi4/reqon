"""Named desktop themes and the shared semantic color roles."""
from dataclasses import dataclass
from functools import lru_cache
import re
from ui_brand import COLORS,METHOD_COLORS,PROTOCOL_COLORS


def rgb(value):
    value=value.lstrip('#')
    if len(value)==3:value=''.join(c*2 for c in value)
    return tuple(int(value[i:i+2],16) for i in (0,2,4))


def mix(a,b,weight):return '#'+''.join(f'{round(x*(1-weight)+y*weight):02x}' for x,y in zip(rgb(a),rgb(b)))


def luminance(value):
    values=[v/255 for v in rgb(value)];values=[v/12.92 if v<=.04045 else ((v+.055)/1.055)**2.4 for v in values]
    return sum(v*w for v,w in zip(values,(.2126,.7152,.0722)))


def contrast(a,b):
    high,low=sorted((luminance(a),luminance(b)),reverse=True);return (high+.05)/(low+.05)


@dataclass(frozen=True)
class Theme:
    id:str
    name:str
    dark:bool
    colors:dict


def palette(id,name,dark,canvas,panel,text,muted,border,accent,action):
    colors=dict(COLORS,canvas=canvas,panel=panel,text=text,muted=muted,border=border,accent=accent,action=action,
        hover=mix(canvas,text,.08),selected=mix(canvas,action,.18),subtle=muted,line=mix(canvas,text,.04),
        action_hover=mix(action,'#ffffff' if dark else '#000000',.12),focus=action,
        error='#f29a97' if dark else '#b4232c',success='#89d6ae' if dark else '#146b3a',warning='#e7bf74' if dark else '#795800',
        string='#ce9178' if dark else '#a24112',key='#9cdcfe' if dark else '#005ea2',number='#b5cea8' if dark else '#176b4b')
    for role in ('error','success','warning'):colors[role+'_bg']=mix(canvas,colors[role],.14)
    colors['match']=mix(canvas,accent,.25);colors['action_text']='#ffffff' if contrast(action,'#ffffff')>=contrast(action,'#111111') else '#111111'
    return Theme(id,name,dark,colors)


THEMES={theme.id:theme for theme in [
    palette('light','Light',False,'#ffffff','#f5f5f5','#202020','#626262','#d4d4d4','#d94f17','#0875d1'),
    Theme('dark','Dark',True,dict(COLORS,action_text='#ffffff')),
    palette('high-contrast-light','High Contrast Light',False,'#ffffff','#ffffff','#000000','#3d3d3d','#595959','#b83c00','#b83c00'),
    palette('high-contrast-dark','High Contrast Dark',True,'#090909','#090909','#ffffff','#dedede','#a0a0a0','#ff8c45','#ff8c45'),
    palette('ayu-light','Ayu Light',False,'#fafafa','#f3f4f5','#454b55','#626b77','#c4c8ce','#9b6500','#e6a200'),
    palette('ayu-dark','Ayu Dark',True,'#0b0e14','#11151c','#d6d9df','#a0a8b5','#3b4558','#ffb454','#ffb454'),
    palette('dracula','Dracula',True,'#282a36','#21222c','#f8f8f2','#b6b2ce','#62627d','#ff79c6','#ff79c6'),
    palette('monokai','Monokai',True,'#272822','#20211c','#f8f8f2','#b7b8a9','#57594b','#a6e22e','#66d9ef'),
    palette('night-owl-light','Night Owl Light',False,'#fbfbfb','#f0f3f4','#253744','#536b7b','#b9c9d0','#087c7c','#087c7c'),
    palette('night-owl-dark','Night Owl Dark',True,'#011627','#00111e','#d6deeb','#9bb3c8','#32556e','#c792ea','#c792ea'),
    palette('solarized-light','Solarized Light',False,'#fdf6e3','#eee8d5','#435761','#58696e','#b8b29e','#866600','#9a7800'),
    palette('solarized-dark','Solarized Dark',True,'#002b36','#073642','#cedcdb','#a3b9ba','#49727b','#2aa198','#2aa198'),
]}
_active='dark'


def current_theme():return THEMES[_active]


def activate_theme(id):
    global _active
    if id not in THEMES:raise ValueError('Unknown theme: '+str(id))
    _active=id


ROLES={value:key for key,value in COLORS.items()}
for role,values in {
    'canvas':['#212121','#202020'], 'panel':['#262626','#242424','#272727'],
    'hover':['#292929','#303030','#353535','#383838','#2d2d2d','#383e47','#292d33'],
    'selected':['#3a3a3a','#393939','#3b3b3b','#30485b','#284c70','#344d68'],
    'border':['#333333','#333','#444','#444444','#414141','#424242','#3d3d3d','#3e3e3e','#555','#555555','#686868'],
    'text':['#ffffff','#fff','#e6e6e6','#efefef','#eeeeee','#eee','#f2f2f2','#f5f5f5','#cccccc','#c0c0c0','#f0dfd8'],
    'muted':['#ababab','#a6a6a6','#aaa','#aaaaaa','#9b9b9b','#999','#999999','#969696','#8e8e8e','#bfbfbf'],
    'subtle':['#777777','#777','#707070','#808080','#888','#888888','#858585','#858575'],
    'action':['#0668d2','#008cff','#0983ef','#3b82f6'], 'action_hover':['#157ce6'],
    'focus':['#6a6a6a','#6e9ecf','#89c6ff','#8dbcf0'], 'accent':['#ff6c37','#ff824f','#ffca80'],
    'error':['#ef9a9a','#ed968d','#d97b6c','#e79591','#ffb39d'], 'error_bg':['#5a1d08','#67220e','#5c1c06'],
    'warning':['#d3b47b','#edc584'],'warning_bg':['#49381e'],'success':['#74d99c','#9bdfb1'],'success_bg':['#183e2d'],
    'string':['#e6dc8e'], 'key':['#569cd6'],
}.items():
    for value in values:ROLES[value]=role


def theme_color(value,usage='',literals=()):
    if not isinstance(value,str):return value
    return _theme_color(value,usage,tuple(literals),current_theme().id)


@lru_cache(maxsize=4096)
def _theme_color(value,usage,literals,theme_id):
    source=value.lower();theme=THEMES[theme_id];colors=theme.colors
    if source in literals:return value
    if source=='white':source='#ffffff'
    elif source=='black':source='#000000'
    if not re.fullmatch(r'#[0-9a-f]{3}(?:[0-9a-f]{3})?',source):return value
    # Preserve the established dark appearance and HTTP method hues exactly.
    if theme.id=='dark':return value
    semantic={**dict.fromkeys([METHOD_COLORS['GET'],METHOD_COLORS['HEAD']],'#146b3a'),METHOD_COLORS['POST']:'#835700',
        METHOD_COLORS['PUT']:'#1e61b6',METHOD_COLORS['PATCH']:'#7541a0',METHOD_COLORS['DELETE']:'#b23132',
        METHOD_COLORS['OPTIONS']:'#a82975',PROTOCOL_COLORS['http']:'#08777c',PROTOCOL_COLORS['grpc']:'#3c65a6'}
    if source in semantic:return source if theme.dark else semantic[source]
    role=ROLES.get(source)
    channels=rgb(source);neutral=max(channels)-min(channels)<18
    if neutral and ('border' in usage or usage in ('gridline-color','outline')):role='border'
    elif neutral and 'background' in usage and max(channels)>210:role='panel'
    elif neutral and role is None:
        role='canvas' if max(channels)<40 else 'hover' if max(channels)<80 else 'muted' if max(channels)<190 else 'text'
    return colors.get(role,value)


def render_style(source,literals=(),theme_id=None):
    """Translate canonical widget styles without rewriting geometry or selectors."""
    return _render_style(source,tuple(value.lower() for value in literals),theme_id or current_theme().id)


@lru_cache(maxsize=512)
def _render_style(source,literals,theme_id):
    if theme_id=='dark':return source
    def block(match):
        text=match.group();background=re.search(r'background(?:-color)?\s*:\s*(#[0-9a-fA-F]{3,6})',text)
        solid=background and ROLES.get(background[1].lower()) in ('action','action_hover','accent')
        def declaration(item):
            prop,value=item[1],item[2]
            def replace_color(color):
                original=color.group()
                if prop=='color' and solid and original.lower() in ('white','#ffffff','#fff','#f5f7fa'):
                    bg=_theme_color(background[1],'',(),theme_id);return '#ffffff' if contrast(bg,'#ffffff')>=contrast(bg,'#111111') else '#111111'
                return _theme_color(original,prop,literals,theme_id)
            return prop+':'+re.sub(r'#[0-9a-fA-F]{3,8}\b|\bwhite\b|\bblack\b',replace_color,value)
        return re.sub(r'([\w-]+)\s*:([^;{}]+)',declaration,text)
    return re.sub(r'[^{}]+',block,source)
