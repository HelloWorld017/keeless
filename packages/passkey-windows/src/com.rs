//! Classic out-of-process COM local server and provider state.

use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use keeless_host_desktop_shared::DesktopLauncher;
use keeless_schema::{DatabaseStatus, Operation, OperationSuccess};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    api::Api,
    authenticator,
    cancellation::{ActiveCeremony, CeremonyGuard},
    error::{
        CLASS_E_NOAGGREGATION, E_FAIL, E_NOINTERFACE, E_POINTER, HResult, NTE_USER_CANCELLED,
        client_error,
    },
    registration,
    sdk_bindings::*,
    session::Session,
};

const COINIT_MULTITHREADED: u32 = 0;
const CLSCTX_LOCAL_SERVER: u32 = 0x4;
const REGCLS_MULTIPLEUSE: u32 = 1;
const REGCLS_SUSPENDED: u32 = 4;
const RPC_E_TOO_LATE: HResult = 0x8001_0119_u32 as i32;
const LOCK_STATUS_TIMEOUT: Duration = Duration::from_secs(1);

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *const c_void, coinit: u32) -> HResult;
    fn CoInitializeSecurity(
        security_descriptor: *const c_void,
        auth_service_count: i32,
        auth_services: *const c_void,
        reserved1: *const c_void,
        authentication_level: u32,
        impersonation_level: u32,
        authentication_list: *const c_void,
        capabilities: u32,
        reserved3: *const c_void,
    ) -> HResult;
    fn CoRegisterClassObject(
        clsid: *const Guid,
        unknown: *mut c_void,
        class_context: u32,
        flags: u32,
        registration: *mut u32,
    ) -> HResult;
    fn CoResumeClassObjects() -> HResult;
    fn CoRevokeClassObject(registration: u32) -> HResult;
    fn CoUninitialize();
}

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
    fn new(launcher: Option<DesktopLauncher>) -> Result<Self, String> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start async runtime: {error}"))?;
        let session = runtime
            .block_on(Session::load_with_launcher(launcher))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            api: Arc::new(Api::load().map_err(|error| error.to_string())?),
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
        self.runtime.block_on(async {
            let mut session = tokio::select! {
                _ = guard.cancelled() => return Err(NTE_USER_CANCELLED),
                session = self.session.lock() => session,
            };
            tokio::select! {
                _ = guard.cancelled() => Err(NTE_USER_CANCELLED),
                result = session.request(operation) => result.map_err(|error| client_error(&error)),
            }
        })
    }

    pub(crate) fn lock_status(&self) -> Long {
        if self.active.is_active() {
            return PLUGIN_LOCKED;
        }
        let result = self.runtime.block_on(async {
            let mut session = tokio::time::timeout(LOCK_STATUS_TIMEOUT, self.session.lock())
                .await
                .map_err(|_| ())?;
            tokio::time::timeout(LOCK_STATUS_TIMEOUT, session.lock_status())
                .await
                .map_err(|_| ())?
                .map_err(|_| ())
        });
        match result {
            Ok(DatabaseStatus::Unlocked) => PLUGIN_UNLOCKED,
            Ok(DatabaseStatus::Locked | DatabaseStatus::NotExist) | Err(_) => PLUGIN_LOCKED,
        }
    }

    fn object_created(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.activated = true;
        state.objects += 1;
    }

    fn object_released(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.objects = state.objects.saturating_sub(1);
        self.lifetime.1.notify_all();
    }

    fn lock_server(&self, locked: bool) {
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

    fn factory_created(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references += 1;
    }

    fn factory_referenced(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references += 1;
    }

    fn factory_released(&self) {
        let mut state = self
            .lifetime
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.factory_references = state.factory_references.saturating_sub(1);
        self.lifetime.1.notify_all();
    }

    fn wait_for_shutdown(&self) {
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

    fn wait_for_factory_release(&self) {
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

#[repr(C)]
pub(crate) struct ComAuthenticator {
    vtable: *const IPluginAuthenticatorVtbl,
    references: AtomicU32,
    pub(crate) provider: Arc<Provider>,
}

#[repr(C)]
struct ComClassFactory {
    vtable: *const IClassFactoryVtbl,
    references: AtomicU32,
    provider: Arc<Provider>,
}

static AUTHENTICATOR_VTABLE: IPluginAuthenticatorVtbl = IPluginAuthenticatorVtbl {
    query_interface: authenticator_query_interface,
    add_ref: authenticator_add_ref,
    release: authenticator_release,
    make_credential: authenticator::make_credential,
    get_assertion: authenticator::get_assertion,
    cancel_operation: authenticator::cancel_operation,
    get_lock_status: authenticator::get_lock_status,
};

static CLASS_FACTORY_VTABLE: IClassFactoryVtbl = IClassFactoryVtbl {
    query_interface: class_factory_query_interface,
    add_ref: class_factory_add_ref,
    release: class_factory_release,
    create_instance: class_factory_create_instance,
    lock_server: class_factory_lock_server,
};

/// Entry point used by the small binary wrapper. Arguments exclude argv[0].
pub fn main(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let mut activated = false;
    let mut command = None;
    let mut desktop = None;
    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().as_ref() {
            "-PluginActivated" | "-Embedding" => activated = true,
            "--enable" | "--disable" | "doctor" | "reset-pairing" => {
                if command.replace(argument).is_some() {
                    return Err("only one command may be supplied".into());
                }
            }
            "--desktop" => {
                let path = arguments
                    .next()
                    .ok_or("--desktop requires an absolute executable path")?;
                desktop = Some(
                    DesktopLauncher::new(PathBuf::from(path)).map_err(|error| error.to_string())?,
                );
            }
            _ => {
                return Err(format!(
                    "unsupported argument: {}",
                    argument.to_string_lossy()
                ));
            }
        }
    }

    if let Some(command) = command {
        if activated || desktop.is_some() {
            return Err("the selected command cannot be combined with activation options".into());
        }
        return run_command(command.to_string_lossy().as_ref());
    }
    if !activated {
        return Err(
            "expected -PluginActivated, --enable, --disable, doctor, or reset-pairing".into(),
        );
    }
    if desktop.is_some() && !cfg!(debug_assertions) {
        return Err("--desktop is only available in development builds".into());
    }
    let launcher = desktop.or_else(installed_desktop_launcher);
    serve(Arc::new(Provider::new(launcher)?))
}

fn run_command(command: &str) -> Result<(), String> {
    match command {
        "--enable" => registration::enable(&Api::load().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string()),
        "--disable" => registration::disable(&Api::load().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string()),
        "doctor" => {
            let api = Api::load().map_err(|error| error.to_string())?;
            let enabled = registration::is_enabled(&api).map_err(|error| error.to_string())?;
            println!("Windows WebAuthn plugin APIs: available");
            println!(
                "Keeless provider: {}",
                if enabled { "enabled" } else { "disabled" }
            );
            Ok(())
        }
        "reset-pairing" => {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime
                .block_on(async {
                    let mut session = Session::load().await?;
                    session.reset_pairing().await
                })
                .map_err(|error: crate::session::SessionError| error.to_string())
        }
        _ => Err("unsupported command".into()),
    }
}

fn installed_desktop_launcher() -> Option<DesktopLauncher> {
    let executable = std::env::current_exe().ok()?;
    let install_root = executable.parent()?.parent()?.parent()?;
    DesktopLauncher::new(install_root.join("keeless.exe")).ok()
}

fn serve(provider: Arc<Provider>) -> Result<(), String> {
    let initialized = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED) };
    if initialized < S_OK {
        return Err(format!("CoInitializeEx failed with {initialized:#x}"));
    }
    let result = (|| {
        let security = unsafe {
            CoInitializeSecurity(
                ptr::null(),
                -1,
                ptr::null(),
                ptr::null(),
                0,
                3,
                ptr::null(),
                0,
                ptr::null(),
            )
        };
        if security < S_OK && security != RPC_E_TOO_LATE {
            return Err(format!("CoInitializeSecurity failed with {security:#x}"));
        }
        let factory = Box::into_raw(Box::new(ComClassFactory {
            vtable: &CLASS_FACTORY_VTABLE,
            references: AtomicU32::new(1),
            provider: provider.clone(),
        }));
        provider.factory_created();
        let mut registration = 0;
        let registered = unsafe {
            CoRegisterClassObject(
                &CLSID_KEELESS_PASSKEY_WINDOWS,
                factory.cast(),
                CLSCTX_LOCAL_SERVER,
                REGCLS_MULTIPLEUSE | REGCLS_SUSPENDED,
                &mut registration,
            )
        };
        if registered < S_OK {
            unsafe { class_factory_release(factory.cast()) };
            return Err(format!("CoRegisterClassObject failed with {registered:#x}"));
        }
        let resumed = unsafe { CoResumeClassObjects() };
        if resumed < S_OK {
            unsafe {
                CoRevokeClassObject(registration);
                class_factory_release(factory.cast());
            }
            return Err(format!("CoResumeClassObjects failed with {resumed:#x}"));
        }
        provider.wait_for_shutdown();
        unsafe {
            CoRevokeClassObject(registration);
            class_factory_release(factory.cast());
        }
        provider.wait_for_factory_release();
        Ok(())
    })();
    unsafe { CoUninitialize() };
    result
}

unsafe extern "system" fn authenticator_query_interface(
    this: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    hresult_boundary(|| unsafe { authenticator_query_interface_inner(this, iid, result) })
}

unsafe fn authenticator_query_interface_inner(
    this: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    if result.is_null() {
        return E_POINTER;
    }
    unsafe { *result = ptr::null_mut() };
    let Some(authenticator) = (this as *mut ComAuthenticator).as_ref() else {
        return E_POINTER;
    };
    let Some(iid) = iid.as_ref() else {
        return E_POINTER;
    };
    if *iid != IID_IUNKNOWN && *iid != IID_IPLUGIN_AUTHENTICATOR {
        return E_NOINTERFACE;
    }
    unsafe { *result = this };
    authenticator_add_ref(this);
    S_OK
}

unsafe extern "system" fn authenticator_add_ref(this: *mut c_void) -> u32 {
    reference_boundary(|| unsafe { authenticator_add_ref_inner(this) })
}

unsafe fn authenticator_add_ref_inner(this: *mut c_void) -> u32 {
    let Some(authenticator) = (this as *mut ComAuthenticator).as_ref() else {
        return 0;
    };
    authenticator.references.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn authenticator_release(this: *mut c_void) -> u32 {
    reference_boundary(|| unsafe { authenticator_release_inner(this) })
}

unsafe fn authenticator_release_inner(this: *mut c_void) -> u32 {
    let Some(authenticator) = (this as *mut ComAuthenticator).as_ref() else {
        return 0;
    };
    let previous = authenticator
        .references
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |references| {
            references.checked_sub(1)
        })
        .unwrap_or(0);
    if previous == 0 {
        return 0;
    }
    let references = previous - 1;
    if references == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        let provider = authenticator.provider.clone();
        unsafe { drop(Box::from_raw(this as *mut ComAuthenticator)) };
        provider.object_released();
    }
    references
}

unsafe extern "system" fn class_factory_query_interface(
    this: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    hresult_boundary(|| unsafe { class_factory_query_interface_inner(this, iid, result) })
}

unsafe fn class_factory_query_interface_inner(
    this: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    if result.is_null() {
        return E_POINTER;
    }
    unsafe { *result = ptr::null_mut() };
    let Some(factory) = (this as *mut ComClassFactory).as_ref() else {
        return E_POINTER;
    };
    let Some(iid) = iid.as_ref() else {
        return E_POINTER;
    };
    if *iid != IID_IUNKNOWN && *iid != IID_ICLASS_FACTORY {
        return E_NOINTERFACE;
    }
    unsafe { *result = this };
    class_factory_add_ref(this);
    let _ = factory;
    S_OK
}

unsafe extern "system" fn class_factory_add_ref(this: *mut c_void) -> u32 {
    reference_boundary(|| unsafe { class_factory_add_ref_inner(this) })
}

unsafe fn class_factory_add_ref_inner(this: *mut c_void) -> u32 {
    let Some(factory) = (this as *mut ComClassFactory).as_ref() else {
        return 0;
    };
    factory.provider.factory_referenced();
    factory.references.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn class_factory_release(this: *mut c_void) -> u32 {
    reference_boundary(|| unsafe { class_factory_release_inner(this) })
}

unsafe fn class_factory_release_inner(this: *mut c_void) -> u32 {
    let Some(factory) = (this as *mut ComClassFactory).as_ref() else {
        return 0;
    };
    let previous = factory
        .references
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |references| {
            references.checked_sub(1)
        })
        .unwrap_or(0);
    if previous == 0 {
        return 0;
    }
    let references = previous - 1;
    let provider = factory.provider.clone();
    provider.factory_released();
    if references == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        unsafe { drop(Box::from_raw(this as *mut ComClassFactory)) };
    }
    references
}

unsafe extern "system" fn class_factory_create_instance(
    this: *mut c_void,
    outer: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    hresult_boundary(|| unsafe { class_factory_create_instance_inner(this, outer, iid, result) })
}

unsafe fn class_factory_create_instance_inner(
    this: *mut c_void,
    outer: *mut c_void,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> HResult {
    if result.is_null() {
        return E_POINTER;
    }
    unsafe { *result = ptr::null_mut() };
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    let Some(factory) = (this as *mut ComClassFactory).as_ref() else {
        return E_POINTER;
    };
    let Some(iid) = iid.as_ref() else {
        return E_POINTER;
    };
    if *iid != IID_IUNKNOWN && *iid != IID_IPLUGIN_AUTHENTICATOR {
        return E_NOINTERFACE;
    }
    let object = Box::new(ComAuthenticator {
        vtable: &AUTHENTICATOR_VTABLE,
        references: AtomicU32::new(1),
        provider: factory.provider.clone(),
    });
    factory.provider.object_created();
    unsafe { *result = Box::into_raw(object).cast() };
    S_OK
}

unsafe extern "system" fn class_factory_lock_server(this: *mut c_void, locked: Bool) -> HResult {
    hresult_boundary(|| unsafe { class_factory_lock_server_inner(this, locked) })
}

unsafe fn class_factory_lock_server_inner(this: *mut c_void, locked: Bool) -> HResult {
    let Some(factory) = (this as *mut ComClassFactory).as_ref() else {
        return E_POINTER;
    };
    factory.provider.lock_server(locked != 0);
    S_OK
}

fn hresult_boundary(callback: impl FnOnce() -> HResult) -> HResult {
    catch_unwind(AssertUnwindSafe(callback)).unwrap_or(E_FAIL)
}

fn reference_boundary(callback: impl FnOnce() -> u32) -> u32 {
    catch_unwind(AssertUnwindSafe(callback)).unwrap_or(0)
}
