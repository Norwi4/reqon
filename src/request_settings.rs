use crate::{
    model::Request,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, FontId, Sense, TextEdit, Ui, pos2, vec2};

pub fn draw(ui: &mut Ui, request: &mut Request, t: Theme) -> bool {
    let mut changed = false;
    egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui| {
        ui.spacing_mut().item_spacing.y=0.;
        let row=ui.allocate_space(vec2(ui.available_width(),32.)).1;
        w::label(ui,pos2(row.left(),row.top()+8.),"Configure request settings for this item.",12.,t.muted);
        let row=ui.allocate_space(vec2(ui.available_width(),32.)).1;
        w::icon(ui,pos2(row.left()+8.,row.top()+8.),Icon::Tag,t.text,16.);
        w::label(ui,pos2(row.left()+20.,row.top()+8.),"Tags",12.,t.text);
        let mut tags=request.tags.join(", ");
        if ui.add_sized(vec2(ui.available_width(),32.),TextEdit::singleline(&mut tags).hint_text("e.g., smoke, regression").font(FontId::proportional(13.)).margin(vec2(12.,8.))).changed(){request.tags=tags.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect();changed=true;}
        ui.add_space(24.);
        if request.protocol != "grpc" {
        changed|=toggle(ui,"URL Encoding","Automatically encode query parameters in the URL",&mut request.encode_url,t);
        changed|=toggle(ui,"Automatically Follow Redirects","Follow HTTP redirects automatically",&mut request.follow_redirects,t);
        changed|=toggle(ui,"Forward Authorization on Redirect","Send Authorization and Proxy-Authorization headers when a redirect points to a different origin",&mut request.forward_auth,t);
        let row=setting(ui,"Max Redirects","Set a limit for the number of redirects to follow",t);
        let mut value=request.max_redirects.to_string();
        if ui.put(rect(row.right()-96.,row.top()+6.,96.,30.),TextEdit::singleline(&mut value).font(FontId::proportional(12.)).margin(vec2(8.,7.))).changed(){if let Ok(value)=value.parse(){request.max_redirects=value;changed=true;}}
        }
        let row=setting(ui,"Timeout (ms)","Set maximum time to wait before aborting the request",t);
        let label=if request.timeout_inherit{"Inherit".into()}else{request.timeout.to_string()};
        let select=w::dropdown(ui,rect(row.right()-160.,row.top()+6.,160.,30.),&label,false,true,t);
        egui::Popup::menu(&select).show(|ui| {
            if ui.selectable_label(request.timeout_inherit,"Inherit").clicked(){request.timeout_inherit=true;changed=true;ui.close();}
            if ui.selectable_label(!request.timeout_inherit,"Custom").clicked(){request.timeout_inherit=false;changed=true;ui.close();}
            if !request.timeout_inherit {ui.horizontal(|ui|{ui.label("ms");changed|=ui.add(egui::DragValue::new(&mut request.timeout).range(0..=86_400_000)).changed();});}
        });
        ui.add_space(24.);
        changed|=toggle(ui,"Validate TLS Certificates","Verify the server certificate and hostname",&mut request.verify_tls,t);
        ui.add_space(12.);
        for (name,value,certificate) in [("Proxy URL",&mut request.proxy,false),("Client Certificate (PEM)",&mut request.client_certificate,true),("Client Private Key (PEM)",&mut request.client_key,true),("CA Certificate (PEM)",&mut request.ca_certificate,true)] {
            let row=ui.allocate_space(vec2(ui.available_width(),24.)).1;
            w::label(ui,pos2(row.left(),row.center().y),name,12.,t.muted);
            let row=ui.allocate_space(vec2(ui.available_width(),30.)).1;
            changed|=ui.put(rect(row.left(),row.top(),row.width()-if certificate{82.}else{0.},30.),TextEdit::singleline(value).font(FontId::proportional(13.)).margin(vec2(8.,7.))).changed();
            if certificate&&ui.push_id(("certificate-browse",name),|ui|w::button(ui,rect(row.right()-74.,row.top(),74.,30.),"Browse",false,t)).inner.clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Certificates",&["pem","crt","key","cer"]).pick_file(){*value=path.to_string_lossy().into_owned();changed=true;}}
            ui.add_space(16.);
        }
    });
    changed
}
fn setting(ui: &mut Ui, label: &str, description: &str, t: Theme) -> egui::Rect {
    let row = ui.allocate_space(vec2(ui.available_width(), 48.)).1;
    w::medium(ui, pos2(row.left(), row.top() + 12.), label, 12., t.text);
    let child = w::child(
        ui,
        rect(
            row.left(),
            row.top() + 20.,
            (row.width() - 170.).max(120.),
            20.,
        ),
        label,
    );
    w::label(
        &child,
        pos2(row.left(), row.top() + 28.),
        description,
        12.,
        t.muted,
    );
    row
}
fn toggle(ui: &mut Ui, label: &str, description: &str, value: &mut bool, t: Theme) -> bool {
    let row = setting(ui, label, description, t);
    let r = rect(row.right() - 36., row.top() + 12., 36., 20.);
    let response = ui.interact(r, ui.scope_id().with(label), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    ui.painter().rect_filled(
        r,
        10,
        if *value {
            t.accent
        } else {
            crate::theme::hex(0x666666)
        },
    );
    ui.painter().circle_filled(
        pos2(
            if *value {
                r.right() - 10.
            } else {
                r.left() + 10.
            },
            r.center().y,
        ),
        6.5,
        egui::Color32::WHITE,
    );
    response.clicked()
}
