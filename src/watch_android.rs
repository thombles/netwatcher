use crate::{list, Error, List, Update};
use jni::objects::{Global, JClass, JObject, JString};
use jni::{jni_sig, jni_str, Env, EnvUnowned, NativeMethod};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

// Include the DEX file built by build.rs
const NETWATCHER_DEX_BYTES: &[u8] = include_bytes!(env!("NETWATCHER_DEX_PATH"));

static STATE: OnceLock<Arc<Mutex<State>>> = OnceLock::new();

struct State {
    watchers: HashMap<WatcherId, Box<dyn FnMut(Update) + Send + 'static>>,
    current_interfaces: List,
    next_watcher_id: usize,
    java_support: Option<Global<JObject<'static>>>,
}

type WatcherId = usize;

pub(crate) struct WatchHandle {
    id: WatcherId,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        if let Some(state_ref) = STATE.get() {
            let mut state = state_ref.lock().unwrap();
            state.watchers.remove(&self.id);

            if state.watchers.is_empty() {
                if let Some(ref support_object) = state.java_support {
                    let _ = stop_java_watching(support_object);
                }
                state.java_support = None;
            }
        }
    }
}

pub(crate) fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    mut callback: F,
) -> Result<WatchHandle, Error> {
    let state_ref = STATE.get_or_init(|| {
        Arc::new(Mutex::new(State {
            watchers: HashMap::new(),
            current_interfaces: List::default(),
            next_watcher_id: 1,
            java_support: None,
        }))
    });

    let current_list = list::list_interfaces()?;

    let initial_update = Update {
        interfaces: current_list.0.clone(),
        diff: current_list.diff_from(&List::default()),
    };
    callback(initial_update);

    let mut state = state_ref.lock().unwrap();
    let id = state.next_watcher_id;
    state.next_watcher_id += 1;
    state.current_interfaces = current_list;
    let is_first_watcher = state.watchers.is_empty();
    state.watchers.insert(id, Box::new(callback));
    if is_first_watcher {
        start_java_watching(&mut state)?;
    }
    Ok(WatchHandle { id })
}

fn start_java_watching(state: &mut State) -> Result<(), Error> {
    let support_object = crate::android::with_android_ctx(|jvm, context_obj| {
        jvm.attach_current_thread(|env| {
            let support_class = inject_dex_class(env, context_obj)?;
            let support_object = env.new_object(
                &support_class,
                jni_sig!("(Landroid/content/Context;)V"),
                &[context_obj.as_ref().into()],
            )?;
            let global_ref = env.new_global_ref(support_object)?;
            env.call_method(
                global_ref.as_ref(),
                jni_str!("startInterfaceWatch"),
                jni_sig!("()V"),
                &[],
            )?;
            Ok(global_ref)
        })
    })?;

    state.java_support = Some(support_object);
    Ok(())
}

fn inject_dex_class<'a>(
    env: &mut Env<'a>,
    context_obj: &Global<JObject<'static>>,
) -> Result<JClass<'a>, Error> {
    // to enable backwards compat to API level 21, write to disk instead of loading in-memory
    let cache_dir = env.call_method(
        context_obj.as_ref(),
        jni_str!("getCodeCacheDir"),
        jni_sig!("()Ljava/io/File;"),
        &[],
    )?;
    let cache_dir_path = env.call_method(
        &cache_dir.l()?,
        jni_str!("getAbsolutePath"),
        jni_sig!("()Ljava/lang/String;"),
        &[],
    )?;
    let cache_dir_jstring = JString::cast_local(env, cache_dir_path.l()?)?;
    let cache_dir_rust = cache_dir_jstring.try_to_string(env)?;
    let temp_dex_path = PathBuf::from(cache_dir_rust.clone()).join("netwatcher.dex");
    fs::write(&temp_dex_path, NETWATCHER_DEX_BYTES)?;

    // dex file must not be writable or it won't be loaded
    let mut perms = fs::metadata(&temp_dex_path)?.permissions();
    perms.set_readonly(true);
    fs::set_permissions(&temp_dex_path, perms)?;

    let dex_class_loader_class = env.find_class(jni_str!("dalvik/system/DexClassLoader"))?;
    let parent_loader = env.call_method(
        context_obj.as_ref(),
        jni_str!("getClassLoader"),
        jni_sig!("()Ljava/lang/ClassLoader;"),
        &[],
    )?;

    let temp_dex_path_str = temp_dex_path.to_string_lossy().to_string();
    let temp_dex_path_jstring = env.new_string(&temp_dex_path_str)?;
    let cache_dir_jstring = env.new_string(&cache_dir_rust)?;
    let dex_loader = env.new_object(
        &dex_class_loader_class,
        jni_sig!(
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/ClassLoader;)V"
        ),
        &[
            (&temp_dex_path_jstring).into(),
            (&cache_dir_jstring).into(),
            (&JObject::null()).into(),
            (&parent_loader.l()?).into(),
        ],
    )?;

    let class_name_str = env.new_string("net.octet_stream.netwatcher.NetwatcherSupportAndroid")?;
    let support_class_obj = env.call_method(
        &dex_loader,
        jni_str!("loadClass"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
        &[(&class_name_str).into()],
    )?;
    let support_class = JClass::cast_local(env, support_class_obj.l()?)?;
    let _ = fs::remove_file(&temp_dex_path);

    let native_methods = [unsafe {
        NativeMethod::from_raw_parts(
            jni_str!("netwatcherInterfacesDidChange"),
            jni_str!("()V"),
            Java_net_octet_1stream_netwatcher_NetwatcherSupportAndroid_netwatcherInterfacesDidChange
                as *mut _,
        )
    }];
    unsafe {
        env.register_native_methods(&support_class, &native_methods)?;
    }

    Ok(support_class)
}

fn stop_java_watching(support_object: &Global<JObject<'static>>) -> Result<(), Error> {
    crate::android::with_android_ctx(|jvm, _| {
        jvm.attach_current_thread(|env| {
            env.call_method(
                support_object.as_ref(),
                jni_str!("stopInterfaceWatch"),
                jni_sig!("()V"),
                &[],
            )?;
            Ok(())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_net_octet_1stream_netwatcher_NetwatcherSupportAndroid_netwatcherInterfacesDidChange(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    let Some(state_ref) = STATE.get() else {
        return;
    };
    let Ok(new_list) = list::list_interfaces() else {
        return;
    };
    let mut state = state_ref.lock().unwrap();
    if new_list == state.current_interfaces {
        return;
    }
    let diff = new_list.diff_from(&state.current_interfaces);
    let update = Update {
        interfaces: new_list.0.clone(),
        diff,
    };
    state.current_interfaces = new_list;
    for callback in state.watchers.values_mut() {
        callback(update.clone());
    }
}
