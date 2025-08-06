use jni::objects::{JClass, JObject};
use jni::sys::jobject;
use jni::JNIEnv;
use netwatcher::{Interface, WatchHandle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

static GUI_CALLBACK: OnceLock<Arc<Mutex<Option<GuiCallback>>>> = OnceLock::new();
static WATCHER_HANDLE: OnceLock<Arc<Mutex<Option<WatchHandle>>>> = OnceLock::new();

struct GuiCallback {
    jvm: jni::JavaVM,
    callback_object: jni::objects::GlobalRef,
}

#[no_mangle]
pub unsafe extern "C" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_setAndroidContext(
    env: JNIEnv,
    _class: JClass,
    context: jobject,
) {
    log::info!("set_android_context in Rust");
    let env_ptr = env.get_raw();
    match netwatcher::set_android_context(env_ptr, context) {
        Ok(_) => {
            log::debug!("Successfully set Android context via netwatcher");
        }
        Err(e) => {
            log::error!("Failed to set Android context: {}", e);
        }
    }
}

#[no_mangle]
pub extern "C" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_startWatching(
    env: JNIEnv,
    _class: JClass,
    callback: jobject,
) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug),
    );

    log::info!("starting network interface watching from Rust");
    if let Ok(jvm) = env.get_java_vm() {
        let callback_obj = unsafe { JObject::from_raw(callback) };
        if let Ok(global_ref) = env.new_global_ref(&callback_obj) {
            let gui_callback = GuiCallback {
                jvm,
                callback_object: global_ref,
            };

            let callback_storage = GUI_CALLBACK.get_or_init(|| Arc::new(Mutex::new(None)));
            *callback_storage.lock().unwrap() = Some(gui_callback);

            start_interface_watching();
        }
    }
}

#[no_mangle]
pub extern "C" fn Java_net_octet_1stream_netwatcher_netwatchertestapp_MainActivity_stopWatching(
    _env: JNIEnv,
    _class: JClass,
) {
    log::info!("stopping network interface watching from Rust");
    stop_interface_watching();
}

fn start_interface_watching() {
    let handle = netwatcher::watch_interfaces(|update| {
        log::info!(
            "interface update received: {} interfaces",
            update.interfaces.len()
        );
        notify_java_gui(format_interfaces(&update.interfaces));
    });

    match handle {
        Ok(handle) => {
            let handle_storage = WATCHER_HANDLE.get_or_init(|| Arc::new(Mutex::new(None)));
            *handle_storage.lock().unwrap() = Some(handle);
        }
        Err(e) => {
            log::error!("failed to start network interface watching: {:?}", e);
        }
    }
}

fn stop_interface_watching() {
    if let Some(handle_storage) = WATCHER_HANDLE.get() {
        *handle_storage.lock().unwrap() = None;
        log::info!("network interface watching stopped");
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

    for (_index, interface) in interfaces {
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
            match callback.jvm.attach_current_thread() {
                Ok(mut env) => match env.new_string(&interface_data) {
                    Ok(java_string) => {
                        if let Err(e) = env.call_method(
                            &callback.callback_object,
                            "onInterfacesChanged",
                            "(Ljava/lang/String;)V",
                            &[(&java_string).into()],
                        ) {
                            log::error!("failed to call java callback: {:?}", e);
                        }
                    }
                    Err(e) => {
                        log::error!("failed to create java string: {:?}", e);
                    }
                },
                Err(e) => {
                    log::error!("failed to attach to java thread: {:?}", e);
                }
            }
        } else {
            log::warn!("GUI callback unset");
        }
    } else {
        log::warn!("GUI_CALLBACK uninitialised");
    }
}
