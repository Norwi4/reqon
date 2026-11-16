//! Persistent native PTY sessions. On Windows portable-pty uses ConPTY.
use crate::{
    app::App,
    theme::{self, Theme},
    widgets::{self as w, Icon, rect},
};
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use portable_pty::{CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

enum Input {
    Bytes(Vec<u8>),
    Resize(u16, u16),
    Close,
}
struct Replies(mpsc::Sender<Input>);
impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        _: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let p = params.first().and_then(|v| v.first()).copied().unwrap_or(0);
        let answer = match (c, p) {
            ('n', 6) => {
                let (r, c) = screen.cursor_position();
                format!("\x1b[{};{}R", r + 1, c + 1)
            }
            ('n', 5) => "\x1b[0n".into(),
            ('c', _) => "\x1b[?1;2c".into(),
            _ => return,
        };
        let _ = self.0.send(Input::Bytes(answer.into_bytes()));
    }
}
type Parser = vt100::Parser<Replies>;
pub struct Session {
    pub id: String,
    pub cwd: PathBuf,
    parser: Arc<Mutex<Parser>>,
    input: mpsc::Sender<Input>,
    exited: Arc<AtomicBool>,
    size: (u16, u16),
    selection: Option<((u16, u16), (u16, u16))>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.input.send(Input::Close);
    }
}
impl Session {
    fn spawn(cwd: PathBuf, ctx: egui::Context) -> Result<Self> {
        let pair = portable_pty::native_pty_system().openpty(PtySize::default())?;
        #[cfg(windows)]
        let shell = std::env::var_os("PWSH").unwrap_or_else(|| "powershell.exe".into());
        #[cfg(not(windows))]
        let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/bash".into());
        let mut cmd = CommandBuilder::new(shell);
        #[cfg(windows)]
        cmd.args(["-NoLogo", "-NoProfile"]);
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .context("Cannot start terminal shell")?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (tx, rx) = mpsc::channel();
        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            24.try_into().unwrap(),
            80.try_into().unwrap(),
            10_000,
            Replies(tx.clone()),
        )));
        let exited = Arc::new(AtomicBool::new(false));
        let read_parser = parser.clone();
        let read_ctx = ctx.clone();
        std::thread::Builder::new()
            .name("terminal-output".into())
            .spawn(move || {
                let mut buffer = [0u8; 16_384];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut parser) = read_parser.lock() {
                                parser.process(&buffer[..n]);
                            }
                            read_ctx.request_repaint();
                        }
                    }
                }
            })?;
        let control_parser = parser.clone();
        let control_exited = exited.clone();
        std::thread::Builder::new()
            .name("terminal-control".into())
            .spawn(move || {
                loop {
                    match rx.recv_timeout(Duration::from_millis(30)) {
                        Ok(Input::Bytes(bytes)) => {
                            if writer
                                .write_all(&bytes)
                                .and_then(|_| writer.flush())
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(Input::Resize(rows, cols)) => {
                            if let Ok(mut p) = control_parser.lock() {
                                p.set_size(rows.try_into().unwrap(), cols.try_into().unwrap());
                            }
                            let _ = pair.master.resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                        }
                        Ok(Input::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let _ = child.kill();
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if let Ok(Some(status)) = child.try_wait() {
                        if let Ok(mut p) = control_parser.lock() {
                            p.process(
                                format!(
                                    "\r\n[Process exited with code {}]\r\n",
                                    status.exit_code()
                                )
                                .as_bytes(),
                            );
                        }
                        break;
                    }
                }
                let _ = child.kill();
                control_exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
                drop(writer);
                drop(pair.master);
            })?;
        Ok(Self {
            id: crate::model::id(),
            cwd,
            parser,
            input: tx,
            exited,
            size: (24, 80),
            selection: None,
        })
    }
    fn send(&self, bytes: impl Into<Vec<u8>>) {
        let _ = self.input.send(Input::Bytes(bytes.into()));
    }
    fn resize(&mut self, rows: u16, cols: u16) {
        let size = (rows.max(2), cols.max(10));
        if size != self.size {
            self.size = size;
            self.selection = None;
            let _ = self.input.send(Input::Resize(size.0, size.1));
        }
    }
}
pub struct Manager {
    sessions: Vec<Session>,
    active: Option<String>,
    tx: mpsc::Sender<Result<Session, String>>,
    rx: mpsc::Receiver<Result<Session, String>>,
    starting: usize,
    initialized: bool,
    error: String,
    focus_next: bool,
}
impl Default for Manager {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            sessions: vec![],
            active: None,
            tx,
            rx,
            starting: 0,
            initialized: false,
            error: String::new(),
            focus_next: false,
        }
    }
}
impl Manager {
    pub fn open(&mut self, cwd: PathBuf, ctx: egui::Context) {
        self.initialized = true;
        self.starting += 1;
        self.error.clear();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = Session::spawn(cwd, ctx.clone()).map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.starting = self.starting.saturating_sub(1);
            match result {
                Ok(session) => {
                    self.active = Some(session.id.clone());
                    self.sessions.push(session);
                    self.focus_next = true;
                }
                Err(e) => self.error = e,
            }
        }
    }
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.active.as_ref().is_some_and(|id| {
            ctx.memory(|m| m.has_focus(egui::Id::unique(("terminal-session", id))))
        })
    }
}
impl App {
    pub fn open_terminal(&mut self, cwd: PathBuf) {
        self.terminal_cwd = Some(cwd.clone());
        self.devtools_open = true;
        self.devtools_tab = 3;
        self.terminal.open(cwd, self.context.clone());
    }
    pub fn terminal_panel(&mut self, ui: &mut Ui, r: Rect) {
        self.terminal.poll();
        let t = self.theme;
        let cwd = self.terminal_cwd.clone().unwrap_or_else(|| {
            self.ws
                .collections
                .iter()
                .find(|c| c.id == self.active_collection_id())
                .map(|c| c.path.clone())
                .unwrap_or_else(|| self.root.clone())
        });
        if !self.terminal.initialized {
            self.terminal.open(cwd.clone(), ui.ctx().clone());
        }
        let sidebar = rect(r.left(), r.top(), 200., r.height());
        w::line(ui, sidebar.right_top(), sidebar.right_bottom(), t.border);
        w::semibold(
            ui,
            pos2(r.left() + 8., r.top() + 15.),
            "Sessions",
            13.,
            t.text,
        );
        if w::icon_button(
            ui,
            rect(sidebar.right() - 28., r.top() + 2., 24., 24.),
            Icon::Plus,
            "New terminal",
            t,
        )
        .clicked()
        {
            self.terminal.open(cwd.clone(), ui.ctx().clone());
        }
        w::line(
            ui,
            pos2(r.left(), r.top() + 30.),
            pos2(sidebar.right(), r.top() + 30.),
            t.border,
        );
        let mut list = w::child(
            ui,
            rect(r.left(), r.top() + 30., 200., r.height() - 30.),
            "terminal-sessions",
        );
        let mut close = None;
        egui::ScrollArea::vertical().show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            for session in &self.terminal.sessions {
                let row = ui.allocate_space(vec2(200., 25.)).1;
                let response = ui.interact(row, ui.scope_id().with(&session.id), Sense::click());
                let active = self.terminal.active.as_ref() == Some(&session.id);
                if active || response.hovered() {
                    ui.painter()
                        .rect_filled(row, 0, if active { t.surface } else { t.hover });
                }
                if active {
                    ui.painter().rect_filled(
                        rect(row.left(), row.top(), 2., row.height()),
                        0,
                        t.accent,
                    );
                }
                w::icon(
                    ui,
                    pos2(row.left() + 13., row.center().y),
                    Icon::Terminal,
                    t.muted,
                    14.,
                );
                let name = session
                    .cwd
                    .file_name()
                    .unwrap_or(session.cwd.as_os_str())
                    .to_string_lossy();
                let label = if session.exited.load(Ordering::Relaxed) {
                    format!("{name} (exited)")
                } else {
                    name.into_owned()
                };
                w::elided_medium(
                    ui,
                    rect(row.left() + 25., row.top(), 147., row.height()),
                    &label,
                    13.,
                    t.text,
                );
                if response.clicked() {
                    self.terminal.active = Some(session.id.clone());
                    self.terminal.focus_next = true;
                }
                if response.hovered()
                    && w::icon_button(
                        ui,
                        rect(row.right() - 25., row.top(), 24., 24.),
                        Icon::TabClose,
                        "Close terminal session",
                        t,
                    )
                    .clicked()
                {
                    close = Some(session.id.clone());
                }
                response.on_hover_text(session.cwd.to_string_lossy());
            }
        });
        if let Some(id) = close {
            self.terminal.sessions.retain(|s| s.id != id);
            if self.terminal.active.as_ref() == Some(&id) {
                self.terminal.active = self.terminal.sessions.last().map(|s| s.id.clone());
                self.terminal.focus_next = true;
            }
        }
        let terminal_rect = rect(
            sidebar.right() + 8.,
            r.top() + 8.,
            r.width() - 216.,
            r.height() - 16.,
        );
        if let Some(session) = self
            .terminal
            .sessions
            .iter_mut()
            .find(|s| Some(&s.id) == self.terminal.active.as_ref())
        {
            draw_session(
                ui,
                terminal_rect,
                session,
                t,
                std::mem::take(&mut self.terminal.focus_next),
            );
        } else {
            let mut empty = w::child(ui, terminal_rect, "terminal-empty");
            if self.terminal.starting > 0 {
                empty.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Starting terminal…");
                });
            } else {
                empty.label("No active terminal sessions");
                if empty.button("New Terminal").clicked() {
                    self.terminal.open(cwd, ui.ctx().clone());
                }
            }
        }
        if !self.terminal.error.is_empty() {
            w::label(
                ui,
                pos2(terminal_rect.left() + 8., terminal_rect.bottom() - 15.),
                &self.terminal.error,
                12.,
                t.red,
            );
        }
    }
}

fn color(c: vt100::Color, default: Color32, t: Theme) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => [
            t.bg,
            t.red,
            t.green,
            t.accent,
            t.blue,
            t.pink,
            theme::hex(0x54c7c7),
            t.text,
            t.faint,
            t.red,
            t.green,
            t.accent,
            t.blue,
            t.pink,
            theme::hex(0x7ddff2),
            Color32::WHITE,
        ][i as usize],
        vt100::Color::Idx(i) if i >= 232 => {
            let n = 8 + (i - 232) * 10;
            Color32::from_rgb(n, n, n)
        }
        vt100::Color::Idx(i) => {
            let n = i - 16;
            let channel = |v| if v == 0 { 0 } else { 55 + 40 * v };
            Color32::from_rgb(channel(n / 36), channel(n / 6 % 6), channel(n % 6))
        }
    }
}
fn draw_session(ui: &mut Ui, r: Rect, s: &mut Session, t: Theme, focus: bool) {
    let font = FontId::new(14., FontFamily::Name("Terminal".into()));
    let width = ui.fonts_mut(|f| f.layout_no_wrap("M".into(), font.clone(), t.text).size().x);
    let height = 16.;
    s.resize((r.height() / height) as u16, (r.width() / width) as u16);
    let id = egui::Id::unique(("terminal-session", &s.id));
    let response = ui.interact(r, id, Sense::click_and_drag());
    if focus || response.clicked() || response.drag_started() {
        response.request_focus();
    }
    let mut parser = s.parser.lock().unwrap();
    let screen = parser.screen_mut();
    let (rows, cols) = screen.size();
    let (rows, cols) = (rows.get(), cols.get());
    let cell_at = |p: egui::Pos2| {
        (
            ((p.y - r.top()) / height)
                .floor()
                .clamp(0., (rows - 1) as f32) as u16,
            ((p.x - r.left()) / width).floor().clamp(0., cols as f32) as u16,
        )
    };
    if response.drag_started() {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            let c = cell_at(p);
            s.selection = Some((c, c));
        }
    }
    if response.dragged() {
        if let (Some((a, _)), Some(p)) = (s.selection, response.interact_pointer_pos()) {
            s.selection = Some((a, cell_at(p)));
        }
    }
    if response.clicked() {
        s.selection = None;
    }
    let selected = s
        .selection
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) })
        .filter(|(a, b)| a != b);
    if response.hovered() {
        let scroll = ui.input_mut(|i| {
            let y = i.smooth_scroll_delta.y;
            i.smooth_scroll_delta.y = 0.;
            y
        });
        if scroll.abs() > 0. {
            screen.set_scrollback((screen.scrollback() as f32 + scroll / height).max(0.) as usize);
        }
    }
    let clipboard = selected.map(|(a, b)| screen.contents_between(a.0, a.1, b.0, b.1));
    response.context_menu(|ui| {
        if ui
            .add_enabled(clipboard.is_some(), egui::Button::new("Copy"))
            .clicked()
        {
            ui.ctx().copy_text(clipboard.clone().unwrap());
            ui.close();
        }
        if ui.button("Paste").clicked() {
            ui.ctx().memory_mut(|m| m.request_focus(id));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
        if ui.button("Select all").clicked() {
            s.selection = Some(((0, 0), (rows - 1, cols)));
            ui.close();
        }
    });
    let mut bytes = vec![];
    if response.has_focus() {
        let events = ui.input_mut(|i| {
            let mut events = vec![];
            i.events.retain(|e| {
                if matches!(
                    e,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                ) {
                    events.push(e.clone());
                    false
                } else {
                    true
                }
            });
            events
        });
        for event in events {
            match event {
                event @ (egui::Event::Copy | egui::Event::Cut) => {
                    if let Some(text) = &clipboard {
                        ui.ctx().copy_text(text.clone());
                    } else {
                        bytes.push(if matches!(event, egui::Event::Cut) {
                            24
                        } else {
                            3
                        });
                    }
                }
                egui::Event::Text(text) => bytes.extend(text.as_bytes()),
                egui::Event::Paste(text) => {
                    if screen.bracketed_paste() {
                        bytes.extend(b"\x1b[200~");
                    }
                    bytes.extend(text.replace("\r\n", "\n").replace('\n', "\r").as_bytes());
                    if screen.bracketed_paste() {
                        bytes.extend(b"\x1b[201~");
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if key == egui::Key::C && modifiers.ctrl && clipboard.is_some() {
                        ui.ctx().copy_text(clipboard.clone().unwrap());
                        continue;
                    }
                    if modifiers.shift && matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                        let n = screen.scrollback();
                        screen.set_scrollback(if key == egui::Key::PageUp {
                            n + rows as usize
                        } else {
                            n.saturating_sub(rows as usize)
                        });
                        continue;
                    }
                    bytes.extend(key_bytes(key, modifiers, screen.application_cursor()));
                }
                _ => {}
            }
        }
    }
    if !bytes.is_empty() {
        screen.set_scrollback(0);
        s.selection = None;
        s.send(bytes);
    }
    let painter = ui.painter().with_clip_rect(r);
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let at = pos2(r.left() + col as f32 * width, r.top() + row as f32 * height);
            let mut fg = color(cell.fgcolor(), t.text, t);
            let mut bg = color(cell.bgcolor(), t.bg, t);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if selected.is_some_and(|(a, b)| (row, col) >= a && (row, col) < b) {
                bg = ui.visuals().selection.bg_fill;
            }
            let cw = width * if cell.is_wide() { 2. } else { 1. };
            if bg != t.bg {
                painter.rect_filled(rect(at.x, at.y, cw, height), 0, bg);
            }
            if cell.has_contents() {
                painter.text(
                    at,
                    egui::Align2::LEFT_TOP,
                    cell.contents(),
                    font.clone(),
                    fg,
                );
            }
            if cell.underline() {
                painter.line_segment(
                    [
                        pos2(at.x, at.y + height - 1.),
                        pos2(at.x + cw, at.y + height - 1.),
                    ],
                    Stroke::new(1., fg),
                );
            }
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 && !s.exited.load(Ordering::Relaxed) {
        let (row, col) = screen.cursor_position();
        let cursor = rect(
            r.left() + col as f32 * width,
            r.top() + row as f32 * height,
            width,
            height,
        );
        if response.has_focus() {
            painter.rect_filled(
                rect(cursor.left(), cursor.top(), 1.5, cursor.height()),
                0,
                t.text,
            );
        } else {
            painter.rect_stroke(
                cursor,
                0,
                Stroke::new(1., t.muted),
                egui::StrokeKind::Inside,
            );
        }
    }
}
fn key_bytes(key: egui::Key, m: egui::Modifiers, application: bool) -> Vec<u8> {
    use egui::Key::*;
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    if modifier > 1
        && matches!(
            key,
            ArrowUp | ArrowDown | ArrowRight | ArrowLeft | Home | End
        )
    {
        let suffix = match key {
            ArrowUp => 'A',
            ArrowDown => 'B',
            ArrowRight => 'C',
            ArrowLeft => 'D',
            Home => 'H',
            _ => 'F',
        };
        return format!("\x1b[1;{modifier}{suffix}").into_bytes();
    }
    if m.ctrl && key == Space {
        return vec![0];
    }
    if m.ctrl && key == Backspace {
        return vec![23];
    }
    if m.alt && key == Backspace {
        return vec![27, 127];
    }
    if m.ctrl {
        let letter = match key {
            A => 1,
            B => 2,
            C => 3,
            D => 4,
            E => 5,
            F => 6,
            G => 7,
            H => 8,
            I => 9,
            J => 10,
            K => 11,
            L => 12,
            M => 13,
            N => 14,
            O => 15,
            P => 16,
            Q => 17,
            R => 18,
            S => 19,
            T => 20,
            U => 21,
            V => return vec![],
            W => 23,
            X => 24,
            Y => 25,
            Z => 26,
            _ => 0,
        };
        if letter > 0 {
            return vec![letter];
        }
    }
    let prefix = if application { "\x1bO" } else { "\x1b[" };
    let s = match key {
        Enter => "\r".into(),
        Backspace => "\x7f".into(),
        Tab => if m.shift { "\x1b[Z" } else { "\t" }.into(),
        Escape => "\x1b".into(),
        ArrowUp => format!("{prefix}A"),
        ArrowDown => format!("{prefix}B"),
        ArrowRight => format!("{prefix}C"),
        ArrowLeft => format!("{prefix}D"),
        Home => "\x1b[H".into(),
        End => "\x1b[F".into(),
        Delete => "\x1b[3~".into(),
        Insert => "\x1b[2~".into(),
        PageUp => "\x1b[5~".into(),
        PageDown => "\x1b[6~".into(),
        F1 => "\x1bOP".into(),
        F2 => "\x1bOQ".into(),
        F3 => "\x1bOR".into(),
        F4 => "\x1bOS".into(),
        F5 => "\x1b[15~".into(),
        F6 => "\x1b[17~".into(),
        F7 => "\x1b[18~".into(),
        F8 => "\x1b[19~".into(),
        F9 => "\x1b[20~".into(),
        F10 => "\x1b[21~".into(),
        F11 => "\x1b[23~".into(),
        F12 => "\x1b[24~".into(),
        _ => String::new(),
    };
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn conpty_preserves_shell_state_handles_unicode_resize_and_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::spawn(dir.path().into(), egui::Context::default()).unwrap();
        session.resize(30, 120);
        fn wait(s: &Session, needle: &str) {
            let until = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                let text = s.parser.lock().unwrap().screen().contents();
                if text.contains(needle) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < until,
                    "Missing {needle}: {text}"
                );
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        wait(&session, "PS ");
        session.send(b"$env:REQON_PTY_TEST='persistent'; Write-Output ('READY_'+'ONE')\r".to_vec());
        wait(&session, "READY_ONE");
        session.send(
            "Write-Output ('VALUE_'+$env:REQON_PTY_TEST); Write-Output ('Привет'+' мир')\r"
                .as_bytes()
                .to_vec(),
        );
        wait(&session, "VALUE_persistent");
        wait(&session, "Привет мир");
        assert_eq!(
            session.parser.lock().unwrap().screen().size(),
            (30.try_into().unwrap(), 120.try_into().unwrap())
        );
        session.send(b"exit\r".to_vec());
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while !session.exited.load(Ordering::Relaxed) {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    #[test]
    fn ansi_and_key_modes_preserve_screen_and_selection() {
        let mut p = vt100::Parser::new(4.try_into().unwrap(), 30.try_into().unwrap(), 100);
        p.process(b"old\r\x1b[2K\x1b[31mred\x1b[0m\r\nnext");
        assert!(p.screen().contents().starts_with("red\nnext"));
        assert_eq!(
            p.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        assert_eq!(key_bytes(egui::Key::C, egui::Modifiers::CTRL, false), [3]);
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE, true),
            b"\x1bOA"
        );
    }
}
