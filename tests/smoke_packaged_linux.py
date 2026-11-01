"""Check the installed executable under X11, with ordinary window-manager close."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time


def check_resizing(window):
    # The WM can list the client before Qt finishes mapping its child widgets.
    # Activate it and let the first expose finish on the emulated build machine.
    subprocess.run(['xdotool','windowactivate','--sync',window],check=True)
    time.sleep(3)
    def geometry():
        output=subprocess.check_output(['xdotool','getwindowgeometry','--shell',window],text=True)
        return {key:int(value) for key,value in (line.split('=',1) for line in output.splitlines())}
    def wait_size(width,height):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            value=geometry()
            if value['WIDTH']==width and value['HEIGHT']==height:return value
            time.sleep(.1)
        raise AssertionError((width,height,geometry()))
    positions=[('left',5,400,50,0),('right',1195,400,-50,0),('top',600,5,0,30),('bottom',600,795,0,-30),
        ('top-left',5,5,50,30),('top-right',1195,5,-50,30),('bottom-left',5,795,50,-30),('bottom-right',1195,795,-50,-30)]
    for name,x,y,dx,dy in positions:
        subprocess.run(['wmctrl','-i','-r',window,'-e','0,120,100,1200,800'],check=True)
        wait_size(1200,800)
        subprocess.run(['xdotool','mousemove','--sync','--window',window,str(x),str(y),'sleep','.3','mousedown','1','sleep','.2',
            'mousemove','--window',window,str(x+dx),str(y+dy),'sleep','.3','mouseup','1'],check=True)
        wait_size(1200-abs(dx),800-abs(dy))
    before=geometry()
    subprocess.run(['wmctrl','-i','-r',window,'-b','add,maximized_vert,maximized_horz'],check=True)
    wait_size(1600,1000)
    subprocess.run(['wmctrl','-i','-r',window,'-b','remove,maximized_vert,maximized_horz'],check=True)
    wait_size(before['WIDTH'],before['HEIGHT'])
    print('Installed Linux package: real mouse resize passed for all 8 sides/corners; maximize/restore passed.')

with tempfile.TemporaryDirectory(prefix='reqon-package-') as directory:
    for attempt in range(2):
        log=Path(directory)/f'run-{attempt}.log'
        with log.open('w') as output:
            process=subprocess.Popen(['/usr/bin/reqon','--data-dir',directory],stdout=output,stderr=subprocess.STDOUT)
            try:
                deadline=time.monotonic()+90; window=None
                while time.monotonic()<deadline and process.poll() is None:
                    # Openbox may still be starting when the application process is launched.
                    listing=subprocess.run(['wmctrl','-l'],text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
                    lines=listing.stdout.splitlines() if listing.returncode==0 else []
                    window=next((line.split()[0] for line in lines if 'Reqon' in line or 'Getman' in line),None)
                    if window:break
                    time.sleep(.2)
                assert window,log.read_text()
                if attempt==0 and os.environ.get('REQON_TEST_RESIZE')=='1':check_resizing(window)
                subprocess.run(['wmctrl','-i','-c',window],check=True)
                assert process.wait(timeout=30)==0,log.read_text()
            finally:
                if process.poll() is None:process.kill();process.wait()
        db=sqlite3.connect(Path(directory)/'reqon.sqlite3')
        assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
        session=json.loads(db.execute("SELECT data FROM workspace_state WHERE key='session'").fetchone()[0])
        assert len(session['tabs'])==1,session
        request=json.loads(db.execute('SELECT data FROM local_drafts ORDER BY rowid DESC LIMIT 1').fetchone()[0])
        assert request['tls_verify'] is False,request
        assert request['grpc_max_response_mb']==10 and request['grpc_timeout_ms']==0,request
        db.close()
    print('Installed Linux package: native X11 startup, close and tab restoration passed twice.')
