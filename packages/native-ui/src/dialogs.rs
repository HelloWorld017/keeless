use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use eframe::egui;
use sha2::{Digest, Sha256};

use crate::{
    protocol::{ConnectionRequest, FileMode, FileRequest, PasswordMode, PasswordRequest},
    secure_text_edit::{SecureTextBuffer, SecureTextEditState, secure_text_edit},
};

type DialogResult<T> = Result<Option<T>, String>;

pub fn prompt_password(request: PasswordRequest) -> DialogResult<SecureTextBuffer> {
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
    run_dialog(password_title(request.mode), [460.0, 220.0], app)?;
    take_result(&result)
}

pub fn prompt_connection(request: ConnectionRequest) -> DialogResult<bool> {
    let result = Arc::new(Mutex::new(None));
    let title = "Keeless connection request";
    let app = ConnectionApp {
        fingerprint: approval_fingerprint(&request.public_key),
        name: request.name,
        result: result.clone(),
    };
    run_dialog(title, [500.0, 240.0], app)?;
    take_result(&result)
}

pub fn pick_file(request: &FileRequest) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = &request.title {
        dialog = dialog.set_title(title);
    }
    if let Some(directory) = &request.directory {
        dialog = dialog.set_directory(directory);
    }
    if let Some(file_name) = &request.file_name {
        dialog = dialog.set_file_name(file_name);
    }
    for filter in &request.filters {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    match request.mode {
        FileMode::Open => dialog.pick_file(),
        FileMode::Save => dialog.save_file(),
    }
}

fn run_dialog(title: &str, size: [f32; 2], app: impl eframe::App + 'static) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(|_| Ok(Box::new(app))))
        .map_err(|error| error.to_string())
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

        egui::CentralPanel::default().show(context, |ui| {
            ui.heading(password_title(self.mode));
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
            } else {
                ui.add_space(20.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Continue").clicked() {
                    submitted = true;
                }
                if ui.button("Cancel").clicked() {
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
    name: Option<String>,
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
        egui::CentralPanel::default().show(context, |ui| {
            ui.heading("Allow a new connection?");
            ui.add_space(8.0);
            ui.label("An unknown local client requested access to Keeless.");
            if let Some(name) = &self.name {
                ui.label(format!("Claimed client name: {name}"));
            }
            ui.add_space(8.0);
            ui.label("Public-key fingerprint:");
            ui.monospace(&self.fingerprint);
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Allow").clicked() {
                    self.finish(context, Ok(Some(true)));
                }
                if ui.button("Deny").clicked() {
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
}
