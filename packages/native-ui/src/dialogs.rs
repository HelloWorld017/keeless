use std::sync::{Arc, Mutex};

use eframe::egui;
use sha2::{Digest, Sha256};

use crate::{
    protocol::{
        ConnectionRequest, DialogAnchor, PasskeyAccount, PasskeyMode, PasskeyRequest, PasswordMode,
        PasswordRequest,
    },
    secure_text_edit::{SecureTextBuffer, SecureTextEditState, secure_text_edit},
    styles,
};

type DialogResult<T> = Result<Option<T>, String>;

pub fn prompt_password(
    request: PasswordRequest,
    anchor: Option<DialogAnchor>,
) -> DialogResult<SecureTextBuffer> {
    let result = Arc::new(Mutex::new(None));
    let app = PasswordApp {
        mode: request.mode,
        password: Some(SecureTextBuffer::new().map_err(|error| error.to_string())?),
        confirmation: Some(SecureTextBuffer::new().map_err(|error| error.to_string())?),
        password_state: SecureTextEditState::default(),
        confirmation_state: SecureTextEditState::default(),
        mismatch: false,
        input_error: None,
        focus_pending: true,
        result: result.clone(),
    };
    run_dialog(
        password_title(request.mode),
        password_dialog_size(request.mode),
        anchor,
        app,
    )?;
    take_result(&result)
}

pub fn prompt_connection(
    request: ConnectionRequest,
    anchor: Option<DialogAnchor>,
) -> DialogResult<bool> {
    let result = Arc::new(Mutex::new(None));
    let title = "Keeless connection request";
    let app = ConnectionApp {
        fingerprint: approval_fingerprint(&request.public_key),
        recipient_fingerprint: approval_fingerprint(&request.recipient),
        sender_scope: request.sender_scope,
        recipient_scope: request.recipient_scope,
        kind: request.kind,
        result: result.clone(),
    };
    run_dialog(title, CONNECTION_DIALOG_SIZE, anchor, app)?;
    take_result(&result)
}

pub fn prompt_passkey(
    request: PasskeyRequest,
    anchor: Option<DialogAnchor>,
) -> DialogResult<String> {
    let result = Arc::new(Mutex::new(None));
    let title = match request.mode {
        PasskeyMode::Register => "Keeless passkey creation",
        PasskeyMode::Assert => "Keeless passkey sign-in",
    };
    let size = passkey_dialog_size(request.accounts.len());
    let app = PasskeyApp {
        mode: request.mode,
        rp_id: request.rp_id,
        accounts: request.accounts,
        selected: 0,
        result: result.clone(),
    };
    run_dialog(title, size, anchor, app)?;
    take_result(&result)
}

fn run_dialog(
    title: &str,
    size: [f32; 2],
    anchor: Option<DialogAnchor>,
    app: impl eframe::App + 'static,
) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_visible(anchor.is_none()),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(|creation_context| {
            styles::configure(&creation_context.egui_ctx);
            Ok(Box::new(PositionedApp { app, anchor, size }))
        }),
    )
    .map_err(|error| error.to_string())
}

struct PositionedApp<T> {
    app: T,
    anchor: Option<DialogAnchor>,
    size: [f32; 2],
}

impl<T: eframe::App> eframe::App for PositionedApp<T> {
    fn update(&mut self, context: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(anchor) = self.anchor.take() {
            if let Some(position) =
                position_for_anchor(anchor, self.size, context.pixels_per_point())
            {
                context.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
            }
            context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        self.app.update(context, frame);
    }
}

fn position_for_anchor(
    anchor: DialogAnchor,
    size: [f32; 2],
    pixels_per_point: f32,
) -> Option<egui::Pos2> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    Some(egui::pos2(
        anchor.x as f32 / pixels_per_point - size[0] / 2.0,
        anchor.y as f32 / pixels_per_point - size[1] / 2.0,
    ))
}

const CONNECTION_DIALOG_SIZE: [f32; 2] = [440.0, 292.0];

fn password_dialog_size(mode: PasswordMode) -> [f32; 2] {
    match mode {
        PasswordMode::Create => [400.0, 224.0],
        PasswordMode::Unlock | PasswordMode::Reveal | PasswordMode::Save => [400.0, 192.0],
    }
}

fn passkey_dialog_size(account_count: usize) -> [f32; 2] {
    let additional_accounts = account_count.saturating_sub(1) as f32;
    [420.0, (208.0 + 28.0 * additional_accounts).min(440.0)]
}

fn take_result<T>(result: &Arc<Mutex<Option<DialogResult<T>>>>) -> DialogResult<T> {
    result
        .lock()
        .map_err(|_| "dialog result lock was poisoned".to_owned())?
        .take()
        .unwrap_or(Ok(None))
}

struct PasswordApp {
    mode: PasswordMode,
    password: Option<SecureTextBuffer>,
    confirmation: Option<SecureTextBuffer>,
    password_state: SecureTextEditState,
    confirmation_state: SecureTextEditState,
    mismatch: bool,
    input_error: Option<&'static str>,
    focus_pending: bool,
    result: Arc<Mutex<Option<DialogResult<SecureTextBuffer>>>>,
}

impl PasswordApp {
    fn finish(&mut self, context: &egui::Context, result: DialogResult<SecureTextBuffer>) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(result);
        }
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn submit(&mut self, context: &egui::Context) {
        let Some(password) = &self.password else {
            return;
        };
        let matches = if self.mode == PasswordMode::Create {
            self.confirmation
                .as_ref()
                .map(|confirmation| password.equals(confirmation))
                .transpose()
                .map(|value| value.unwrap_or(false))
        } else {
            Ok(true)
        };
        match matches {
            Ok(true) if !password.is_empty() => {
                let password = self.password.take().expect("password exists");
                self.finish(context, Ok(Some(password)));
            }
            Ok(_) => self.mismatch = true,
            Err(error) => self.finish(context, Err(error.to_string())),
        }
    }
}

impl eframe::App for PasswordApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        if self.result.lock().is_ok_and(|slot| slot.is_some()) {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if context.input(|input| input.viewport().close_requested()) {
            self.finish(context, Ok(None));
            return;
        }
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.finish(context, Ok(None));
            return;
        }

        egui::CentralPanel::default()
            .frame(styles::panel())
            .show(context, |ui| {
                styles::title(ui, password_title(self.mode));
                ui.add_space(8.0);
                let output = secure_text_edit(
                    ui,
                    "password",
                    self.password.as_mut().expect("password exists"),
                    &mut self.password_state,
                    "Password",
                );
                if self.focus_pending {
                    output.response.request_focus();
                    self.focus_pending = false;
                }
                if output.response.changed() {
                    self.input_error = None;
                    self.mismatch = false;
                }
                let mut submitted = output.submitted;
                if let Some(error) = output.error {
                    match error {
                        crate::secure_text_edit::SecureTextEditError::Capacity => {
                            self.input_error = Some("Password is limited to 4096 UTF-8 bytes.");
                        }
                        crate::secure_text_edit::SecureTextEditError::Memory(error) => {
                            self.finish(context, Err(error.to_string()));
                            return;
                        }
                    }
                }
                if self.mode == PasswordMode::Create {
                    ui.add_space(8.0);
                    let output = secure_text_edit(
                        ui,
                        "confirmation",
                        self.confirmation.as_mut().expect("confirmation exists"),
                        &mut self.confirmation_state,
                        "Confirm password",
                    );
                    if output.response.changed() {
                        self.input_error = None;
                        self.mismatch = false;
                    }
                    submitted |= output.submitted;
                    if let Some(error) = output.error {
                        match error {
                            crate::secure_text_edit::SecureTextEditError::Capacity => {
                                self.input_error = Some("Password is limited to 4096 UTF-8 bytes.");
                            }
                            crate::secure_text_edit::SecureTextEditError::Memory(error) => {
                                self.finish(context, Err(error.to_string()));
                                return;
                            }
                        }
                    }
                }
                if let Some(message) = self.input_error {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                } else if self.mismatch {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        if self.mode == PasswordMode::Create {
                            "Enter matching non-empty passwords."
                        } else {
                            "Enter a password."
                        },
                    );
                }
                styles::actions(ui, |ui| {
                    if styles::primary_button(ui, "Continue").clicked() {
                        submitted = true;
                    }
                    if styles::secondary_button(ui, "Cancel").clicked() {
                        self.finish(context, Ok(None));
                    }
                });
                if submitted {
                    self.input_error = None;
                    self.submit(context);
                }
            });
    }
}

impl Drop for PasswordApp {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(Ok(None));
        }
    }
}

struct ConnectionApp {
    fingerprint: String,
    recipient_fingerprint: String,
    sender_scope: String,
    recipient_scope: String,
    kind: String,
    result: Arc<Mutex<Option<DialogResult<bool>>>>,
}

impl ConnectionApp {
    fn finish(&self, context: &egui::Context, result: DialogResult<bool>) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(result);
        }
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for ConnectionApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        if context.input(|input| input.viewport().close_requested())
            || context.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.finish(context, Ok(None));
            return;
        }
        egui::CentralPanel::default()
            .frame(styles::panel())
            .show(context, |ui| {
                styles::title(
                    ui,
                    if self.kind == "upgrade" {
                        "Allow database access?"
                    } else {
                        "Allow a limited connection?"
                    },
                );
                ui.add_space(8.0);
                styles::description(ui, format!("Client scope: {}", self.sender_scope));
                styles::description(ui, format!("Server scope: {}", self.recipient_scope));
                if self.sender_scope == "app" {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "This app can access every database that you approve.",
                    );
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Client fingerprint").small());
                styles::code(ui, &self.fingerprint);
                ui.label(egui::RichText::new("Server fingerprint").small());
                styles::code(ui, &self.recipient_fingerprint);
                styles::actions(ui, |ui| {
                    if styles::primary_button(ui, "Allow").clicked() {
                        self.finish(context, Ok(Some(true)));
                    }
                    if styles::secondary_button(ui, "Deny").clicked() {
                        self.finish(context, Ok(Some(false)));
                    }
                });
            });
    }
}

impl Drop for ConnectionApp {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(Ok(None));
        }
    }
}

struct PasskeyApp {
    mode: PasskeyMode,
    rp_id: String,
    accounts: Vec<PasskeyAccount>,
    selected: usize,
    result: Arc<Mutex<Option<DialogResult<String>>>>,
}

impl PasskeyApp {
    fn finish(&self, context: &egui::Context, result: DialogResult<String>) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(result);
        }
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn approve(&self, context: &egui::Context) {
        let Some(account) = self.accounts.get(self.selected) else {
            self.finish(context, Ok(None));
            return;
        };
        self.finish(context, Ok(Some(account.id.clone())));
    }
}

impl eframe::App for PasskeyApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        if context.input(|input| input.viewport().close_requested())
            || context.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.finish(context, Ok(None));
            return;
        }
        egui::CentralPanel::default()
            .frame(styles::panel())
            .show(context, |ui| {
                match self.mode {
                    PasskeyMode::Register => {
                        styles::title(ui, "Create a passkey?");
                        ui.add_space(8.0);
                        styles::description(ui, "A site asked Keeless to create a passkey for:");
                    }
                    PasskeyMode::Assert => {
                        styles::title(ui, "Sign in with a passkey?");
                        ui.add_space(8.0);
                        styles::description(ui, "A site asked Keeless to sign in to:");
                    }
                }
                styles::code(ui, &self.rp_id);
                ui.add_space(12.0);

                if self.accounts.len() == 1 {
                    styles::description(ui, format!("Account: {}", self.accounts[0].username));
                } else {
                    styles::description(ui, "Choose an account:");
                    egui::ScrollArea::vertical()
                        .max_height(196.0)
                        .show(ui, |ui| {
                            for (index, account) in self.accounts.iter().enumerate() {
                                ui.radio_value(&mut self.selected, index, &account.username);
                            }
                        });
                }

                styles::actions(ui, |ui| {
                    if styles::primary_button(ui, "Approve").clicked() {
                        self.approve(context);
                    }
                    if styles::secondary_button(ui, "Deny").clicked() {
                        self.finish(context, Ok(None));
                    }
                });
            });
    }
}

impl Drop for PasskeyApp {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.result.lock()
            && slot.is_none()
        {
            *slot = Some(Ok(None));
        }
    }
}

fn password_title(mode: PasswordMode) -> &'static str {
    match mode {
        PasswordMode::Create => "Create database password",
        PasswordMode::Unlock => "Unlock database",
        PasswordMode::Reveal => "Confirm to reveal password",
        PasswordMode::Save => "Confirm to save database",
    }
}

fn approval_fingerprint(bundle: &str) -> String {
    let digest = Sha256::digest(bundle.as_bytes());
    digest[..16]
        .chunks(2)
        .map(hex::encode)
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_compact() {
        assert_eq!(
            approval_fingerprint("client"),
            "948f:e603:f61d:c036:b5c5:96dc:09fe:3ce3"
        );
    }

    #[test]
    fn dialog_sizes_fit_their_content() {
        assert_eq!(password_dialog_size(PasswordMode::Unlock), [400.0, 192.0]);
        assert_eq!(password_dialog_size(PasswordMode::Create), [400.0, 224.0]);
        assert_eq!(CONNECTION_DIALOG_SIZE, [440.0, 292.0]);
        assert_eq!(passkey_dialog_size(1), [420.0, 208.0]);
        assert_eq!(passkey_dialog_size(2), [420.0, 236.0]);
        assert_eq!(passkey_dialog_size(32), [420.0, 440.0]);
    }

    #[test]
    fn positions_the_dialog_around_the_physical_anchor() {
        assert_eq!(
            position_for_anchor(DialogAnchor { x: 800, y: 600 }, [400.0, 200.0], 2.0),
            Some(egui::pos2(200.0, 200.0))
        );
        assert_eq!(
            position_for_anchor(DialogAnchor { x: 0, y: 0 }, [400.0, 200.0], 0.0),
            None
        );
    }
}
