use crate::{
    app::{Action, App, Dialog},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Response, Sense, Ui};

pub const WIDTH: f32 = 44.;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Panel {
    #[default]
    Collections,
    Environments,
}

fn icon_button(
    ui: &mut Ui,
    bounds: Rect,
    icon: Icon,
    tooltip: &str,
    active: bool,
    interactive: bool,
    theme: Theme,
) -> Response {
    let response = ui.interact(
        bounds,
        ui.scope_id().with(tooltip),
        if interactive {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, interactive, active, tooltip)
    });
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            bounds,
            6,
            if active {
                theme.accent.gamma_multiply(0.10)
            } else {
                theme.hover
            },
        );
    }
    if active {
        ui.painter().rect_filled(
            rect(bounds.left() - 6., bounds.center().y - 10., 2., 20.),
            1,
            theme.accent,
        );
    }
    w::icon(
        ui,
        bounds.center(),
        icon,
        if active {
            theme.accent
        } else if response.hovered() {
            theme.text
        } else {
            theme.muted
        },
        20.,
    );
    response.on_hover_text(tooltip)
}

impl App {
    pub(crate) fn toggle_sidebar_panel(&mut self, panel: Panel) {
        self.sidebar_visible = self.sidebar_panel != panel || !self.sidebar_visible;
        self.sidebar_panel = panel;
    }

    pub(crate) fn navigation_rail(&mut self, ui: &mut Ui, bounds: Rect) {
        let t = self.theme;
        let mut rail = w::child(ui, bounds, "navigation-rail");
        rail.painter().rect_filled(bounds, 0, t.sidebar);
        // Keep the full separator inside the rail's clip rectangle.
        rail.painter().rect_filled(
            rect(bounds.right() - 1., bounds.top(), 1., bounds.height()),
            0,
            t.border,
        );
        let button = |top| rect(bounds.left() + 6., top, 32., 32.);
        if icon_button(
            &mut rail,
            button(bounds.top() + 6.),
            Icon::Box,
            "Collections",
            self.sidebar_visible && self.sidebar_panel == Panel::Collections,
            true,
            t,
        )
        .clicked()
        {
            self.toggle_sidebar_panel(Panel::Collections);
        }
        if icon_button(
            &mut rail,
            button(bounds.top() + 50.),
            Icon::Key,
            "Manage environments",
            self.sidebar_visible && self.sidebar_panel == Panel::Environments,
            true,
            t,
        )
        .clicked()
        {
            self.toggle_sidebar_panel(Panel::Environments);
        }
        // A visual placeholder only: no action, navigation or stored state.
        icon_button(
            &mut rail,
            button(bounds.top() + 94.),
            Icon::Flask,
            "Preview",
            false,
            false,
            t,
        );
        if icon_button(
            &mut rail,
            button(bounds.bottom() - 38.),
            Icon::Settings,
            "Preferences",
            self.ws.active.as_deref() == Some(crate::preferences::TAB_ID),
            true,
            t,
        )
        .clicked()
        {
            self.actions.push(Action::Dialog(Dialog::Settings));
        }
    }
}
