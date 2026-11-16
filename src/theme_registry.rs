use crate::{model::Preferences, theme::Theme};
use eframe::egui::{self, Color32};
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub mode: String,
    theme: Value,
}
pub fn presets() -> &'static [Preset] {
    static THEMES: OnceLock<Vec<Preset>> = OnceLock::new();
    THEMES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/themes/bruno.json"))
            .expect("embedded theme palettes")
    })
}
impl Preset {
    fn color(&self, path: &str, fallback: Color32) -> Color32 {
        self.theme
            .pointer(path)
            .and_then(Value::as_array)
            .filter(|a| a.len() == 4)
            .map(|a| {
                Color32::from_rgba_unmultiplied(
                    a[0].as_u64().unwrap_or(0) as u8,
                    a[1].as_u64().unwrap_or(0) as u8,
                    a[2].as_u64().unwrap_or(0) as u8,
                    a[3].as_u64().unwrap_or(255) as u8,
                )
            })
            .unwrap_or(fallback)
    }
    pub fn palette(&self) -> Theme {
        let mut t = Theme::new(self.mode == "dark");
        t.bg = self.color("/background/base", t.bg);
        t.sidebar = self.color("/sidebar/bg", t.sidebar);
        t.surface = self.color("/tabs/secondary/active/bg", t.surface);
        t.hover = self.color("/sidebar/collection/item/hoverBg", t.hover);
        t.border = self.color("/border/border1", t.border);
        t.strong_border = self.color("/border/border2", t.strong_border);
        t.text = self.color("/text", t.text);
        t.muted = self.color("/colors/text/muted", t.muted);
        t.faint = self.color("/colors/text/subtext0", t.faint);
        t.accent = self.color("/brand", t.accent);
        t.green = self.color("/request/methods/get", t.green);
        t.red = self.color("/request/methods/delete", t.red);
        t.blue = self.color("/textLink", t.blue);
        t.orange = self.color("/request/methods/put", t.orange);
        t.pink = self.color("/request/gql", t.pink);
        for (index, name) in [
            "property", "string", "number", "comment", "keyword", "operator",
        ]
        .iter()
        .enumerate()
        {
            t.syntax[index] = self.color(&format!("/codemirror/tokens/{name}"), t.syntax[index]);
        }
        t
    }
}
pub fn current(p: &Preferences, legacy_dark: bool, ctx: &egui::Context) -> Theme {
    let dark = match p.appearance.mode.as_str() {
        "dark" => true,
        "light" => false,
        "system" => ctx
            .system_theme()
            .map(|x| x == egui::Theme::Dark)
            .unwrap_or(legacy_dark),
        _ => legacy_dark,
    };
    let id = if dark {
        &p.appearance.dark
    } else {
        &p.appearance.light
    };
    presets()
        .iter()
        .find(|x| &x.id == id && (x.mode == "dark") == dark)
        .map(Preset::palette)
        .unwrap_or_else(|| Theme::new(dark))
}

pub fn font_bytes(family: &str) -> Option<(Vec<u8>, u32)> {
    if family.trim().is_empty() || family.eq_ignore_ascii_case("default") {
        return None;
    }
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    let db = DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    });
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    })?;
    db.with_face_data(id, |data, index| (data.to_vec(), index))
}
