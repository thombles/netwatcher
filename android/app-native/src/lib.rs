use jni::{
    jni_sig, jni_str,
    objects::{Global, JClass, JObject},
    EnvUnowned, Outcome,
};
use netwatcher::{Interface, WatchHandle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use tokio::sync::oneshot;

static GUI_CALLBACK: OnceLock<Arc<Mutex<Option<GuiCallback>>>> = OnceLock::new();
static WATCHER_HANDLE: OnceLock<Arc<Mutex<Option<WatchHandle>>>> = OnceLock::new();
static ASYNC_WATCHER: OnceLock<Arc<Mutex<Option<AsyncWatcher>>>> = OnceLock::new();

struct GuiCallback {
    jvm: jni::JavaVM,
    callback_object: Global<JObject<'static>>,
}

struct AsyncWatcher {
    stop_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl AsyncWatcher {
    fn stop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

impl Drop for AsyncWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn init_android_logging() {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug),
    );
}

// Helper for CI testing that logs IPs in a particular format
fn log_ips(prefix: &str, interfaces: &HashMap<u32, Interface>) {
    let mut ips: Vec<String> = interfaces
        .values()
        .flat_map(|iface| {
            iface
                .ips
                .iter()
                .map(|record| format!("{}:{}", iface.name, record.ip))
        })
        .collect();
    ips.sort();
    let joined = ips.join(",");
    log::info!("{prefix}:{joined}");
}

/// JNI entrypoint invoked from Java to provide the Android `Context` to Rust.
///
/// The exported function itself is safe to call from JNI because it receives
/// typed wrapper values. The raw-pointer unsafety is contained inside
/// `netwatcher::set_android_context`.
#[no_mangle]
pub extern "system" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_setAndroidContext(
    env: EnvUnowned<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
) {
    init_android_logging();
    log::info!("set_android_context in Rust");
    match unsafe { netwatcher::set_android_context(env.as_raw(), context.as_raw()) } {
        Ok(_) => {
            log::debug!("Successfully set Android context via netwatcher");
            // For CI testing, list interfaces at startup
            match netwatcher::list_interfaces() {
                Ok(ifs) => log_ips("LIST_IPS", &ifs),
                Err(e) => log::error!("Failed to list interfaces after setting context: {e}"),
            }
        }
        Err(e) => {
            log::error!("Failed to set Android context: {e}");
        }
    }
}

/// JNI entrypoint invoked from Java to register the GUI callback and start
/// both sync and async interface watchers.
#[no_mangle]
pub extern "system" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_startWatching(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    callback: JObject<'_>,
) {
    init_android_logging();

    log::info!("starting network interface watching from Rust");
    match env
        .with_env(|env| -> jni::errors::Result<()> {
            let gui_callback = GuiCallback {
                jvm: env.get_java_vm()?,
                callback_object: env.new_global_ref(&callback)?,
            };

            let callback_storage = GUI_CALLBACK.get_or_init(|| Arc::new(Mutex::new(None)));
            *callback_storage.lock().unwrap() = Some(gui_callback);

            Ok(())
        })
        .into_outcome()
    {
        Outcome::Ok(()) => start_interface_watching(),
        Outcome::Err(e) => log::error!("failed to initialise java callback: {e:?}"),
        Outcome::Panic(_) => log::error!("panic while initialising java callback"),
    }
}

#[no_mangle]
pub extern "system" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_stopWatching(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    log::info!("stopping network interface watching from Rust");
    stop_interface_watching();
}

fn start_interface_watching() {
    let handle = netwatcher::watch_interfaces_with_callback(|update| {
        log::info!(
            "interface update received: {} interfaces",
            update.interfaces.len()
        );
        // For CI testing, emit WATCH_IPS
        log_ips("WATCH_IPS", &update.interfaces);
        notify_java_gui(format_interfaces(&update.interfaces));
    });

    match handle {
        Ok(handle) => {
            let handle_storage = WATCHER_HANDLE.get_or_init(|| Arc::new(Mutex::new(None)));
            *handle_storage.lock().unwrap() = Some(handle);
        }
        Err(e) => {
            log::error!("failed to start network interface watching: {e:?}");
        }
    }

    let (stop_tx, mut stop_rx) = oneshot::channel();
    let join_handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for async watcher");
        runtime.block_on(async move {
            let Ok(mut watch) =
                netwatcher::watch_interfaces_async::<netwatcher::async_adapter::Tokio>()
            else {
                log::error!("failed to start async network interface watcher");
                return;
            };

            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    update = watch.changed() => {
                        log_ips("ASYNC_WATCH_IPS", &update.interfaces);
                    }
                }
            }
        });
    });
    let async_storage = ASYNC_WATCHER.get_or_init(|| Arc::new(Mutex::new(None)));
    *async_storage.lock().unwrap() = Some(AsyncWatcher {
        stop_tx: Some(stop_tx),
        join_handle: Some(join_handle),
    });

    std::thread::spawn(move || {
        let Ok(mut watch) = netwatcher::watch_interfaces_blocking() else {
            log::error!("failed to start blocking network interface watcher");
            return;
        };

        loop {
            let update = watch.updated();
            log_ips("BLOCKING_WATCH_IPS", &update.interfaces);
        }
    });
}

fn stop_interface_watching() {
    if let Some(handle_storage) = WATCHER_HANDLE.get() {
        *handle_storage.lock().unwrap() = None;
        log::info!("network interface watching stopped");
    }

    if let Some(async_storage) = ASYNC_WATCHER.get() {
        if let Some(mut async_watcher) = async_storage.lock().unwrap().take() {
            async_watcher.stop();
        }
    }

    if let Some(callback_storage) = GUI_CALLBACK.get() {
        *callback_storage.lock().unwrap() = None;
    }
}

fn format_interfaces(interfaces: &HashMap<u32, Interface>) -> String {
    let mut result = String::new();

    if interfaces.is_empty() {
        result.push_str("No network interfaces found");
        return result;
    }

    for interface in interfaces.values() {
        result.push_str(&format!("{}:\n", interface.name));

        if interface.ips.is_empty() {
            result.push_str("  No IP addresses\n");
        } else {
            for ip in &interface.ips {
                result.push_str(&format!("  {}/{}\n", ip.ip, ip.prefix_len));
            }
        }
        result.push('\n');
    }

    result
}

fn notify_java_gui(interface_data: String) {
    if let Some(callback_storage) = GUI_CALLBACK.get() {
        let guard = callback_storage.lock().unwrap();
        if let Some(ref callback) = *guard {
            if let Err(e) = callback
                .jvm
                .attach_current_thread(|env| -> jni::errors::Result<()> {
                    let java_string = env.new_string(&interface_data)?;
                    env.call_method(
                        callback.callback_object.as_ref(),
                        jni_str!("onInterfacesChanged"),
                        jni_sig!("(Ljava/lang/String;)V"),
                        &[(&java_string).into()],
                    )?;
                    Ok(())
                })
            {
                log::error!("failed to notify java callback: {e:?}");
            }
        } else {
            log::warn!("GUI callback unset");
        }
    } else {
        log::warn!("GUI_CALLBACK uninitialised");
    }
}
