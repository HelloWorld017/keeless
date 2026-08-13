use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, vec2};

pub const DIALOG_PADDING: i8 = 16;
pub const CONTROL_HEIGHT: f32 = 32.0;
pub const PRIMARY: Color32 = Color32::from_rgb(0, 115, 230);

const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
const FOREGROUND: Color32 = Color32::from_rgb(37, 37, 37);
const MUTED: Color32 = Color32::from_rgb(247, 247, 247);
const MUTED_FOREGROUND: Color32 = Color32::from_rgb(102, 102, 102);
const BORDER: Color32 = Color32::from_rgb(232, 232, 232);
const PRIMARY_FOREGROUND: Color32 = Color32::from_rgb(248, 251, 255);
const ERROR: Color32 = Color32::from_rgb(220, 38, 38);
const WARNING: Color32 = Color32::from_rgb(180, 83, 9);
const RADIUS: u8 = 7;

pub fn configure(context: &egui::Context) {
    context.set_theme(egui::Theme::Light);
    context.style_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(16.0));

        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.window_margin = Margin::same(DIALOG_PADDING);
        style.spacing.button_padding = vec2(10.0, 4.0);
        style.spacing.interact_size = vec2(0.0, CONTROL_HEIGHT);
        style.spacing.icon_width = 16.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.icon_spacing = 6.0;
        style.spacing.scroll = egui::style::ScrollStyle::thin();

        let visuals = &mut style.visuals;
        visuals.dark_mode = false;
        visuals.override_text_color = Some(FOREGROUND);
        visuals.faint_bg_color = MUTED;
        visuals.extreme_bg_color = SURFACE;
        visuals.code_bg_color = MUTED;
        visuals.warn_fg_color = WARNING;
        visuals.error_fg_color = ERROR;
        visuals.window_corner_radius = CornerRadius::same(RADIUS);
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.window_fill = SURFACE;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.menu_corner_radius = CornerRadius::same(RADIUS);
        visuals.panel_fill = SURFACE;
        visuals.popup_shadow = egui::Shadow::NONE;
        visuals.text_cursor.stroke = Stroke::new(1.0, PRIMARY);
        visuals.button_frame = true;
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

        let noninteractive = &mut visuals.widgets.noninteractive;
        noninteractive.bg_fill = SURFACE;
        noninteractive.weak_bg_fill = SURFACE;
        noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        noninteractive.corner_radius = CornerRadius::same(RADIUS);
        noninteractive.fg_stroke = Stroke::new(1.0, FOREGROUND);
        noninteractive.expansion = 0.0;

        let inactive = &mut visuals.widgets.inactive;
        inactive.bg_fill = SURFACE;
        inactive.weak_bg_fill = SURFACE;
        inactive.bg_stroke = Stroke::new(1.0, BORDER);
        inactive.corner_radius = CornerRadius::same(RADIUS);
        inactive.fg_stroke = Stroke::new(1.0, FOREGROUND);
        inactive.expansion = 0.0;

        let hovered = &mut visuals.widgets.hovered;
        hovered.bg_fill = MUTED;
        hovered.weak_bg_fill = MUTED;
        hovered.bg_stroke = Stroke::new(1.0, BORDER);
        hovered.corner_radius = CornerRadius::same(RADIUS);
        hovered.fg_stroke = Stroke::new(1.0, FOREGROUND);
        hovered.expansion = 0.0;

        let active = &mut visuals.widgets.active;
        active.bg_fill = Color32::from_rgb(235, 244, 255);
        active.weak_bg_fill = Color32::from_rgb(235, 244, 255);
        active.bg_stroke = Stroke::new(1.0, PRIMARY);
        active.corner_radius = CornerRadius::same(RADIUS);
        active.fg_stroke = Stroke::new(1.0, FOREGROUND);
        active.expansion = 0.0;

        visuals.selection.bg_fill = Color32::from_rgb(219, 234, 254);
        visuals.selection.stroke = Stroke::new(1.0, PRIMARY);
    });
}

pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .inner_margin(Margin::same(DIALOG_PADDING))
}

pub fn title(ui: &mut egui::Ui, value: &str) {
    ui.label(
        RichText::new(value)
            .text_style(egui::TextStyle::Heading)
            .strong(),
    );
}

pub fn description(ui: &mut egui::Ui, value: impl Into<egui::WidgetText>) {
    ui.add(
        egui::Label::new(value.into().color(MUTED_FOREGROUND))
            .wrap()
            .sense(egui::Sense::hover()),
    );
}

pub fn code(ui: &mut egui::Ui, value: &str) {
    egui::Frame::new()
        .fill(MUTED)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 0.0],
                egui::Label::new(RichText::new(value).monospace()).truncate(),
            )
            .on_hover_text(value);
        });
}

pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(PRIMARY_FOREGROUND))
            .fill(PRIMARY)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(RADIUS))
            .min_size(vec2(0.0, CONTROL_HEIGHT)),
    )
}

pub fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(label)
            .corner_radius(CornerRadius::same(RADIUS))
            .min_size(vec2(0.0, CONTROL_HEIGHT)),
    )
}

pub fn actions(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(12.0);
    ui.with_layout(
        egui::Layout::right_to_left(egui::Align::Center),
        add_contents,
    );
}
