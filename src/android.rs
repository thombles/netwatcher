use crate::Error;
use jni::{
    objects::{Global, JObject},
    EnvUnowned, JavaVM, Outcome,
};
use std::sync::{Mutex, OnceLock};

struct AndroidContext {
    vm: JavaVM,
    context: Global<JObject<'static>>,
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
    let mut env = unsafe { EnvUnowned::from_raw(env) };
    match env
        .with_env(|env| {
            let context_obj = unsafe { JObject::from_raw(env, context) };
            let android_ctx = AndroidContext {
                vm: env.get_java_vm()?,
                context: env.new_global_ref(&context_obj)?,
            };

            let context_storage = ANDROID_CONTEXT.get_or_init(|| Mutex::new(None));
            *context_storage.lock().unwrap() = Some(android_ctx);

            Ok(())
        })
        .into_outcome()
    {
        Outcome::Ok(()) => Ok(()),
        Outcome::Err(err) => Err(err),
        Outcome::Panic(_) => Err(Error::Jni("panic while setting Android context".into())),
    }
}

pub(crate) fn with_android_ctx<T>(
    f: impl FnOnce(&JavaVM, &Global<JObject<'static>>) -> Result<T, Error>,
) -> Result<T, Error> {
    if let Some(context_storage) = ANDROID_CONTEXT.get() {
        let ctx = context_storage.lock().unwrap();
        if let Some(ref android_ctx) = *ctx {
            return f(&android_ctx.vm, &android_ctx.context);
        }
    }

    // Fallback to ndk_context if no explicit context was set
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err(Error::NoAndroidContext);
    }

    unsafe {
        let vm = JavaVM::from_raw(ctx.vm() as *mut jni::sys::JavaVM);
        vm.attach_current_thread(|env| {
            let context_obj = JObject::from_raw(env, ctx.context() as jni::sys::jobject);
            let global_context = env.new_global_ref(&context_obj)?;

            f(&vm, &global_context)
        })
    }
}
