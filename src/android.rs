use crate::Error;
use jni::{objects::JObject, JNIEnv, JavaVM};
use std::sync::{Mutex, OnceLock};

struct AndroidContext {
    vm: JavaVM,
    context: jni::objects::GlobalRef,
}

unsafe impl Send for AndroidContext {}
unsafe impl Sync for AndroidContext {}

static ANDROID_CONTEXT: OnceLock<Mutex<Option<AndroidContext>>> = OnceLock::new();

/// Sets the Android context for the netwatcher library.
///
/// # Safety
///
/// This function is unsafe because it accepts raw pointers from the JNI layer.
/// The caller must ensure that:
/// - `env` is a valid JNIEnv pointer from the current JNI call
/// - `context` is a valid jobject representing an Android Context
/// - The pointers remain valid for the duration of this function call
pub unsafe fn set_android_context(
    env: *mut jni::sys::JNIEnv,
    context: jni::sys::jobject,
) -> Result<(), Error> {
    let env = JNIEnv::from_raw(env)?;
    let context_obj = JObject::from_raw(context);

    let jvm = env.get_java_vm()?;
    let global_context = env.new_global_ref(&context_obj)?;

    let android_ctx = AndroidContext {
        vm: jvm,
        context: global_context,
    };

    let context_storage = ANDROID_CONTEXT.get_or_init(|| Mutex::new(None));
    *context_storage.lock().unwrap() = Some(android_ctx);

    Ok(())
}

pub(crate) fn android_ctx() -> Option<(*mut std::ffi::c_void, *mut std::ffi::c_void)> {
    if let Some(context_storage) = ANDROID_CONTEXT.get() {
        let ctx = context_storage.lock().unwrap();
        if let Some(ref android_ctx) = *ctx {
            let vm_ptr = android_ctx.vm.get_java_vm_pointer() as *mut std::ffi::c_void;
            let context_ptr = android_ctx.context.as_obj().as_raw() as *mut std::ffi::c_void;
            return Some((vm_ptr, context_ptr));
        }
    }

    // Fallback to ndk_context if no explicit context was set
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return None;
    }
    Some((ctx.vm(), ctx.context()))
}
