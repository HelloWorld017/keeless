//! State and operations shared by COM authenticator instances.

use std::sync::{Arc, Condvar, Mutex, PoisonError};

use keeless_host_desktop_shared::DesktopLauncher;
use keeless_schema::{Operation, OperationSuccess};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    api::Api,
    cancellation::{ActiveCeremony, CeremonyGuard},
    error::{HResult, NTE_USER_CANCELLED, session_request_error},
    sdk_bindings::{Long, PLUGIN_LOCKED, PLUGIN_UNLOCKED},
    session::Session,
};

pub(crate) struct Provider {
    pub(crate) api: Arc<Api>,
    runtime: Runtime,
    session: AsyncMutex<Session>,
    pub(crate) active: ActiveCeremony,
    lifetime: (Mutex<ServerLifetime>, Condvar),
}

#[derive(Default)]
struct ServerLifetime {
    activated: bool,
    objects: u32,
    locks: u32,
    factory_references: u32,
}

impl Provider {
    pub(crate) fn new(launcher: Option<DesktopLauncher>) -> Result<Self, String> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start async runtime: {error}"))?;
        let session = runtime
            .block_on(Session::load_with_launcher(launcher))
            .map_err(|error| error.to_string())?;
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: provider session initialized (paired={})",
            session.is_paired()
        );
        let api = Api::load().map_err(|error| error.to_string())?;
        crate::diagnostics::diagnostic!("keeless-passkey-windows: Windows WebAuthn API loaded");
        Ok(Self {
            api: Arc::new(api),
            runtime,
            session: AsyncMutex::new(session),
            active: ActiveCeremony::default(),
            lifetime: (Mutex::new(ServerLifetime::default()), Condvar::new()),
        })
    }

    pub(crate) fn request(
        &self,
        guard: &CeremonyGuard<'_>,
        operation: Operation,
    ) -> Result<OperationSuccess, HResult> {
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: provider waiting for session lock"
        );
        let result = self.runtime.block_on(async {
            let mut session = tokio::select! {
                _ = guard.cancelled() => return Err(NTE_USER_CANCELLED),
                session = self.session.lock() => session,
            };
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: provider acquired session lock"
            );
            tokio::select! {
                _ = guard.cancelled() => Err(NTE_USER_CANCELLED),
                result = session.request_with_sync(operation, &self.api) => result.map_err(|error| session_request_error(&error)),
            }
        });
        match &result {
            Ok(_) => crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: provider desktop request completed"
            ),
            Err(error) => crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: provider desktop request failed with {error:#010x}"
            ),
        }
        result
    }

    pub(crate) fn sync_credentials(&self, guard: &CeremonyGuard<'_>) -> Result<(), HResult> {
        let result = self.runtime.block_on(async {
            let mut session = tokio::select! {
                _ = guard.cancelled() => return Err(NTE_USER_CANCELLED),
                session = self.session.lock() => session,
            };
            tokio::select! {
                _ = guard.cancelled() => Err(NTE_USER_CANCELLED),
                result = session.sync_credentials(&self.api) => result.map_err(|error| session_request_error(&error)),
            }
        });
        if let Err(error) = &result {
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: post-registration credential sync failed with {error:#010x}"
            );
        }
        result
    }

    pub(crate) fn sync_on_activation(&self) {
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: activation credential sync started"
        );

        let result = self.runtime.block_on(async {
            let mut session = self.session.lock().await;
            session.sync_credentials(&self.api).await
        });

        match result {
            Ok(()) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: activation credential sync completed"
                );
            }
            Err(error) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: activation credential sync skipped/failed ({})",
                    error
                );
            }
        }
    }

    pub(crate) fn lock_status(&self) -> Long {
        if self.active.is_active() {
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: lock status is locked because a ceremony is active"
            );
            return PLUGIN_LOCKED;
        }
        PLUGIN_UNLOCKED
    }

    pub(crate) fn object_created(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.activated = true;
        state.objects += 1;
    }

    pub(crate) fn object_released(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.objects = state.objects.saturating_sub(1);
        self.lifetime.1.notify_all();
    }

    pub(crate) fn lock_server(&self, locked: bool) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if locked {
            state.locks += 1;
        } else {
            state.locks = state.locks.saturating_sub(1);
        }
        self.lifetime.1.notify_all();
    }

    pub(crate) fn factory_created(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references += 1;
    }

    pub(crate) fn factory_referenced(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references += 1;
    }

    pub(crate) fn factory_released(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references = state.factory_references.saturating_sub(1);
        self.lifetime.1.notify_all();
    }

    pub(crate) fn wait_for_shutdown(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while !state.activated || state.objects != 0 || state.locks != 0 {
            state = self
                .lifetime
                .1
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    pub(crate) fn wait_for_factory_release(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while state.objects != 0 || state.locks != 0 || state.factory_references != 0 {
            state = self
                .lifetime
                .1
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}
