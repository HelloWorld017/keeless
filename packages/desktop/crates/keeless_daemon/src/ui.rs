use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use eframe::egui;
use keeless_core::{
    ClientApprovalProvider, CoreError, HostFuture, PasswordInputMode, PasswordInputProvider,
};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use zeroize::{Zeroize, Zeroizing};

enum UiRequest {
    Approval {
        bundle: String,
        response: oneshot::Sender<bool>,
    },
    Password {
        mode: PasswordInputMode,
        response: oneshot::Sender<Option<Zeroizing<Vec<u8>>>>,
    },
}

pub struct UiBridge {
    sender: mpsc::Sender<UiRequest>,
}

impl UiBridge {
    pub fn channel(shutdown: Arc<AtomicBool>) -> (Arc<Self>, DaemonApp) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(Self { sender }),
            DaemonApp {
                receiver,
                prompt: None,
                visible: false,
                shutdown,
            },
        )
    }
}

impl ClientApprovalProvider for UiBridge {
    fn approve(&self, public_key_bundle: &str) -> HostFuture<'_, keeless_core::Result<bool>> {
        let bundle = public_key_bundle.to_owned();
        Box::pin(async move {
            let (sender, receiver) = oneshot::channel();
            self.sender
                .send(UiRequest::Approval {
                    bundle,
                    response: sender,
                })
                .map_err(|_| CoreError::Host("desktop UI is unavailable".into()))?;
            receiver
                .await
                .map_err(|_| CoreError::Host("desktop approval was cancelled".into()))
        })
    }
}

impl PasswordInputProvider for UiBridge {
    fn request_password(
        &self,
        mode: PasswordInputMode,
    ) -> HostFuture<'_, keeless_core::Result<Option<Zeroizing<Vec<u8>>>>> {
        Box::pin(async move {
            let (sender, receiver) = oneshot::channel();
            self.sender
                .send(UiRequest::Password {
                    mode,
                    response: sender,
                })
                .map_err(|_| CoreError::Host("desktop UI is unavailable".into()))?;
            receiver
                .await
                .map_err(|_| CoreError::Host("desktop password input was cancelled".into()))
        })
    }
}

enum Prompt {
    Approval {
        bundle: String,
        response: oneshot::Sender<bool>,
    },
    Password {
        mode: PasswordInputMode,
        password: String,
        confirmation: String,
        mismatch: bool,
        response: oneshot::Sender<Option<Zeroizing<Vec<u8>>>>,
    },
}

pub struct DaemonApp {
    receiver: mpsc::Receiver<UiRequest>,
    prompt: Option<Prompt>,
    visible: bool,
    shutdown: Arc<AtomicBool>,
}

impl eframe::App for DaemonApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        if self.shutdown.load(Ordering::Relaxed) {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.prompt.is_none() {
            if let Ok(request) = self.receiver.try_recv() {
                self.prompt = Some(match request {
                    UiRequest::Approval { bundle, response } => {
                        Prompt::Approval { bundle, response }
                    }
                    UiRequest::Password { mode, response } => Prompt::Password {
                        mode,
                        password: String::new(),
                        confirmation: String::new(),
                        mismatch: false,
                        response,
                    },
                });
                self.visible = true;
                context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                context.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        egui::CentralPanel::default().show(context, |ui| match self.prompt.take() {
            None => {}
            Some(Prompt::Approval { bundle, response }) => {
                ui.heading("Approve a new client?");
                ui.label("An unknown local client requested access.");
                ui.monospace(approval_fingerprint(&bundle));
                ui.horizontal(|ui| {
                    if ui.button("Deny").clicked() {
                        let _ = response.send(false);
                    } else if ui.button("Approve").clicked() {
                        let _ = response.send(true);
                    } else {
                        self.prompt = Some(Prompt::Approval { bundle, response });
                    }
                });
            }
            Some(Prompt::Password {
                mode,
                mut password,
                mut confirmation,
                mut mismatch,
                response,
            }) => {
                ui.heading(password_title(mode));
                let password_field = egui::TextEdit::singleline(&mut password)
                    .password(true)
                    .hint_text("Password");
                ui.add(password_field);
                if mode == PasswordInputMode::Create {
                    ui.add(
                        egui::TextEdit::singleline(&mut confirmation)
                            .password(true)
                            .hint_text("Confirm password"),
                    );
                }
                if mismatch {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        if mode == PasswordInputMode::Create {
                            "Enter matching non-empty passwords."
                        } else {
                            "Enter a password."
                        },
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        password.zeroize();
                        confirmation.zeroize();
                        let _ = response.send(None);
                    } else if ui.button("Continue").clicked() {
                        if password.is_empty()
                            || (mode == PasswordInputMode::Create && password != confirmation)
                        {
                            mismatch = true;
                            self.prompt = Some(Prompt::Password {
                                mode,
                                password,
                                confirmation,
                                mismatch,
                                response,
                            });
                        } else {
                            confirmation.zeroize();
                            let bytes = Zeroizing::new(std::mem::take(&mut password).into_bytes());
                            let _ = response.send(Some(bytes));
                        }
                    } else {
                        self.prompt = Some(Prompt::Password {
                            mode,
                            password,
                            confirmation,
                            mismatch,
                            response,
                        });
                    }
                });
            }
        });
        if self.visible && self.prompt.is_none() {
            self.visible = false;
            context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        context.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl Drop for DaemonApp {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn password_title(mode: PasswordInputMode) -> &'static str {
    match mode {
        PasswordInputMode::Create => "Create database password",
        PasswordInputMode::Unlock => "Unlock database",
        PasswordInputMode::Reveal => "Confirm to reveal password",
        PasswordInputMode::Save => "Confirm to save database",
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
