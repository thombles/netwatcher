use crate::{list, Error, List, Update};
use jni::objects::{Global, JClass, JObject, JString};
use jni::{jni_sig, jni_str, Env, EnvUnowned, NativeMethod};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::async_callback::{
    next_async_list, push_async_list, shared_async_callback_queue, wait_next_list,
    SharedAsyncCallbackQueue,
};

const NETWATCHER_DEX_BYTES: &[u8] = include_bytes!(env!("NETWATCHER_DEX_PATH"));

static STATE: OnceLock<Arc<Mutex<State>>> = OnceLock::new();

type WatcherId = usize;

struct State {
    callback_watchers: HashMap<WatcherId, Box<dyn FnMut(Update) + Send + 'static>>,
    queued_watchers: HashMap<WatcherId, SharedAsyncCallbackQueue>,
    current_interfaces: List,
    next_watcher_id: WatcherId,
    java_support: Option<Global<JObject<'static>>>,
}

pub(crate) struct WatchHandle {
    id: WatcherId,
}

pub(crate) struct AsyncWatch {
    id: WatcherId,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
}

pub(crate) struct BlockingWatch {
    id: WatcherId,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
}

impl Drop for AsyncWatch {
    fn drop(&mut self) {
        unregister_watcher(self.id);
    }
}

impl AsyncWatch {
    pub(crate) async fn changed(&mut self) -> Update {
        loop {
            let new_list = next_async_list(&self.queue).await;
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

impl Drop for BlockingWatch {
    fn drop(&mut self) {
        unregister_watcher(self.id);
    }
}

impl BlockingWatch {
    pub(crate) fn updated(&mut self) -> Update {
        loop {
            let new_list = wait_next_list(&self.queue);
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        unregister_watcher(self.id);
    }
}

pub(crate) fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let id = register_callback_watcher(Box::new(callback))?;
    Ok(WatchHandle { id })
}

#[allow(clippy::extra_unused_type_parameters)]
pub(crate) fn watch_interfaces_async<A: crate::AsyncFdAdapter>() -> Result<AsyncWatch, Error> {
    let queue = shared_async_callback_queue();
    let id = register_queued_watcher(queue.clone())?;
    Ok(AsyncWatch {
        id,
        queue,
        cursor: crate::UpdateCursor::default(),
    })
}

pub(crate) fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    let queue = shared_async_callback_queue();
    let id = register_queued_watcher(queue.clone())?;
    Ok(BlockingWatch {
        id,
        queue,
        cursor: crate::UpdateCursor::default(),
    })
}

fn register_callback_watcher(
    mut callback: Box<dyn FnMut(Update) + Send + 'static>,
) -> Result<WatcherId, Error> {
    let state_ref = STATE.get_or_init(init_state).clone();

    let current_list = list::list_interfaces()?;
    callback(current_list.initial_update());

    let mut state = state_ref.lock().unwrap();
    let id = state.next_watcher_id;
    let is_first_watcher = state.callback_watchers.is_empty() && state.queued_watchers.is_empty();
    if is_first_watcher {
        start_java_watching(&mut state)?;
    }
    state.next_watcher_id += 1;
    state.current_interfaces = current_list;
    state.callback_watchers.insert(id, callback);
    Ok(id)
}

fn register_queued_watcher(queue: SharedAsyncCallbackQueue) -> Result<WatcherId, Error> {
    let state_ref = STATE.get_or_init(init_state).clone();
    let current_list = list::list_interfaces()?;
    push_async_list(&queue, current_list.clone());

    let mut state = state_ref.lock().unwrap();
    let id = state.next_watcher_id;
    let is_first_watcher = state.callback_watchers.is_empty() && state.queued_watchers.is_empty();
    if is_first_watcher {
        start_java_watching(&mut state)?;
    }
    state.next_watcher_id += 1;
    state.current_interfaces = current_list;
    state.queued_watchers.insert(id, queue);
    Ok(id)
}

fn unregister_watcher(id: WatcherId) {
    let Some(state_ref) = STATE.get() else {
        return;
    };

    let mut state = state_ref.lock().unwrap();
    state.callback_watchers.remove(&id);
    state.queued_watchers.remove(&id);

    if state.callback_watchers.is_empty() && state.queued_watchers.is_empty() {
        if let Some(ref support_object) = state.java_support {
            let _ = stop_java_watching(support_object);
        }
        state.java_support = None;
    }
}

fn init_state() -> Arc<Mutex<State>> {
    Arc::new(Mutex::new(State {
        callback_watchers: HashMap::new(),
        queued_watchers: HashMap::new(),
        current_interfaces: List::default(),
        next_watcher_id: 1,
        java_support: None,
    }))
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

    let update = new_list.update_from(&state.current_interfaces);
    state.current_interfaces = new_list;

    for callback in state.callback_watchers.values_mut() {
        callback(update.clone());
    }
    for queue in state.queued_watchers.values() {
        push_async_list(queue, state.current_interfaces.clone());
    }
}
