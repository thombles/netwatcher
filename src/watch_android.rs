use crate::{list, Error, List, Update};
use jni::objects::{JClass, JObject};
use jni::sys::{jint, JNINativeMethod};
use jni::{JNIEnv, JavaVM};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

// Include the DEX file built by build.rs
const NETWATCHER_DEX_BYTES: &[u8] = include_bytes!(env!("NETWATCHER_DEX_PATH"));

static STATE: OnceLock<Arc<Mutex<State>>> = OnceLock::new();

struct State {
    watchers: HashMap<WatcherId, Box<dyn FnMut(Update) + Send + 'static>>,
    current_interfaces: List,
    next_watcher_id: usize,
    java_support: Option<JavaSupport>,
}

struct JavaSupport {
    jvm: JavaVM,
    support_object: jni::objects::GlobalRef,
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
                if let Some(ref support) = state.java_support {
                    let _ = stop_java_watching(support);
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
    let (vm_ptr, context_ptr) = crate::android::android_ctx().ok_or(Error::NoAndroidContext)?;
    let jvm = unsafe { JavaVM::from_raw(vm_ptr as *mut jni::sys::JavaVM)? };

    let support_object = {
        let mut env = jvm.attach_current_thread()?;
        let support_class = inject_dex_class(&mut env)?;
        let context_obj = unsafe { JObject::from_raw(context_ptr as jni::sys::jobject) };
        let support_object = env.new_object(
            &support_class,
            "(Landroid/content/Context;)V",
            &[(&context_obj).into()],
        )?;
        let global_ref = env.new_global_ref(support_object)?;
        env.call_method(&global_ref, "startInterfaceWatch", "()V", &[])?;
        global_ref
    };

    let java_support = JavaSupport {
        jvm,
        support_object,
    };
    state.java_support = Some(java_support);
    Ok(())
}

fn inject_dex_class<'a>(env: &mut JNIEnv<'a>) -> Result<JClass<'a>, Error> {
    let (_, context_ptr) = crate::android::android_ctx().ok_or(Error::NoAndroidContext)?;
    let context_obj = unsafe { JObject::from_raw(context_ptr as jni::sys::jobject) };
    let byte_buffer = unsafe {
        env.new_direct_byte_buffer(
            NETWATCHER_DEX_BYTES.as_ptr() as *mut u8,
            NETWATCHER_DEX_BYTES.len(),
        )?
    };

    // API 26+
    let in_memory_class = env.find_class("dalvik/system/InMemoryDexClassLoader")?;
    let parent_loader = env.call_method(
        &context_obj,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        &[],
    )?;
    let dex_loader = env.new_object(
        &in_memory_class,
        "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        &[(&byte_buffer).into(), (&parent_loader.l()?).into()],
    )?;

    // Load the support class and register native methods
    let class_name_str = env.new_string("net.octet_stream.netwatcher.NetwatcherSupportAndroid")?;
    let support_class_obj = env.call_method(
        &dex_loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[(&class_name_str).into()],
    )?;
    let support_class: JClass = support_class_obj.l()?.into();

    let native_methods = [JNINativeMethod {
        name: b"netwatcherInterfacesDidChange\0".as_ptr() as *mut std::os::raw::c_char,
        signature: b"()V\0".as_ptr() as *mut std::os::raw::c_char,
        fnPtr:
            Java_net_octet_1stream_netwatcher_NetwatcherSupportAndroid_netwatcherInterfacesDidChange
                as *mut std::ffi::c_void,
    }];

    let result = unsafe {
        (**env.get_raw()).RegisterNatives.unwrap()(
            env.get_raw(),
            support_class.as_raw(),
            native_methods.as_ptr(),
            native_methods.len() as jint,
        )
    };
    if result != 0 {
        return Err(Error::Jni("Failed to register native methods".to_string()));
    }

    Ok(support_class)
}

fn stop_java_watching(java_support: &JavaSupport) -> Result<(), Error> {
    let mut env = java_support.jvm.attach_current_thread()?;
    env.call_method(
        &java_support.support_object,
        "stopInterfaceWatch",
        "()V",
        &[],
    )?;
    Ok(())
}

#[no_mangle]
pub extern "C" fn Java_net_octet_1stream_netwatcher_NetwatcherSupportAndroid_netwatcherInterfacesDidChange(
    _env: JNIEnv,
    _class: JClass,
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
