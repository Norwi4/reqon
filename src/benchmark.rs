use crate::{
    app::{App, PreparedReply},
    model::*,
};
use eframe::egui::{self, pos2, vec2};
use std::{path::Path, time::Instant};
fn stats(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    serde_json::json!({"p50_ms":samples[n/2],"p95_ms":samples[(n*95/100).min(n-1)],"max_ms":samples[n-1]})
}
fn frame(app: &mut App, ctx: &egui::Context) -> f64 {
    let start = Instant::now();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0., 0.), vec2(1510., 944.))),
            ..Default::default()
        },
        |ui| {
            app.draw(ui);
        },
    );
    // Exercise tessellation too; no GPU upload or presentation is included.
    let _ = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
    output.textures_delta.clear();
    start.elapsed().as_secs_f64() * 1000.
}
pub fn run(root: &Path) -> anyhow::Result<serde_json::Value> {
    let ctx = egui::Context::default();
    let mut app = App::from_context(&ctx, root.join(format!("bench-{}", id())))?;
    app.disk_sync = None;
    app.ws.requests.clear();
    app.ws.collections.clear();
    app.ws.open_tabs.clear();
    app.saved.clear();
    app.ws.collections.push(Collection {
        id: "bench".into(),
        name: "Performance Collection".into(),
        path: root.join("benchmark-fixture"),
        environments: vec![],
        vars: vec![],
        folders: vec![],
    });
    let body = "{\"payload\":\"".to_owned() + &"x".repeat(8192) + "\"}";
    let request_body_bytes = body.len();
    for i in 0..10_000 {
        app.ws.requests.push(Request {
            id: format!("request-{i}"),
            name: format!("Request {i:05}"),
            collection: "bench".into(),
            folder: format!("Group {:03}", i / 100),
            url: "http://localhost:9473/users".into(),
            body: body.clone(),
            body_kind: BodyKind::Json,
            ..Default::default()
        });
    }
    app.ws.open_tabs = (0..20).map(|i| format!("request-{i}")).collect();
    app.ws.active = Some("request-0".into());
    let start = Instant::now();
    let rows = app.tree.rows(&app.ws, "", &app.collapsed);
    let tree_ms = start.elapsed().as_secs_f64() * 1000.;
    let row_count = rows.len();
    for _ in 0..8 {
        frame(&mut app, &ctx);
    }
    let idle = (0..120).map(|_| frame(&mut app, &ctx)).collect();
    let tabs = (0..120)
        .map(|i| {
            app.open(format!("request-{}", i % 20));
            frame(&mut app, &ctx)
        })
        .collect();
    let body = serde_json::to_string(
        &(0..100_000)
            .map(|i| serde_json::json!({"id":i,"name":format!("Record {i}"),"active":true}))
            .collect::<Vec<_>>(),
    )?;
    let size = body.len();
    let start = Instant::now();
    let prepared = PreparedReply::new(Reply {
        status: 200,
        reason: "OK".into(),
        size,
        body,
        ..Default::default()
    });
    let preparation = start.elapsed().as_secs_f64() * 1000.;
    app.tabs
        .entry(app.ws.active.clone().unwrap())
        .or_default()
        .apply_reply(prepared);
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    let response = (0..120).map(|_| frame(&mut app, &ctx)).collect();
    let start = Instant::now();
    app.flush();
    let initial_snapshot_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut saves = vec![];
    for _ in 0..10 {
        if let Some(id) = app.ws.active.clone() {
            app.modified_requests.insert(id);
        }
        let start = Instant::now();
        app.flush();
        saves.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let mut result = serde_json::json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"scope":"CPU layout + tessellation, headless 1510x944, excludes GPU presentation", "requests":10_000,"request_body_bytes":request_body_bytes,"tree_rows":row_count,"tree_build_ms":tree_ms,"steady_frames":stats(idle),"tab_switch_frames":stats(tabs),"response_bytes":size,"response_prepare_ms":preparation,"response_frames":stats(response),"initial_snapshot_ms":initial_snapshot_ms,"incremental_snapshot":stats(saves)});
    app.git.insert(
        "bench".into(),
        crate::git::State {
            root: Some(root.join("benchmark-fixture")),
            branch: "main".into(),
            updated: Some(Instant::now()),
            files: (0..10_000)
                .map(|i| crate::git::File {
                    path: format!("Group {:03}/Request {i:05}.req", i / 100),
                    index: '?',
                    work: '?',
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
    app.open("__git__bench".into());
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    result["git_changed_files"] = 10_000.into();
    result["git_frames"] = stats((0..120).map(|_| frame(&mut app, &ctx)).collect());
    app.ws.requests.clear();
    app.ws.collections.clear();
    app.ws.open_tabs.clear();
    app.ws.active = None;
    app.tabs.clear();
    Ok(result)
}
