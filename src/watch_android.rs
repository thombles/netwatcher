use crate::{list, Error, List, Update};
use jni::JavaVM;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

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
        let class_name = "net/octet_stream/netwatcher/netwatcher_android/NetwatcherAndroidSupport";
        let support_class = env.find_class(class_name)?;
        let constructor_sig = "(Landroid/content/Context;)V";
        let context_obj =
            unsafe { jni::objects::JObject::from_raw(context_ptr as jni::sys::jobject) };
        let support_object =
            env.new_object(&support_class, constructor_sig, &[(&context_obj).into()])?;
        let global_ref = env.new_global_ref(support_object)?;
        let callback_ptr = netwatcher_interfaces_did_change as *const () as jni::sys::jlong;
        env.call_method(
            &global_ref,
            "startInterfaceWatch",
            "(J)V",
            &[jni::objects::JValue::Long(callback_ptr)],
        )?;
        global_ref
    };
    let java_support = JavaSupport {
        jvm,
        support_object,
    };
    state.java_support = Some(java_support);
    Ok(())
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
pub extern "C" fn netwatcher_interfaces_did_change() {
    let Some(state_ref) = STATE.get() else {
        return;
    };
    let Ok(new_list) = list::list_interfaces() else {
        return;
    };
    let mut state = state_ref.lock().unwrap();
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
