//! The few Android framework calls the app needs, made through JNI on the activity that
//! `android-activity` registered with `ndk-context`: whether the app may read shared storage,
//! asking for that access, and the system language.
//!
//! Every call returns a `Result`; a Java exception is cleared and reported as an error, never
//! left pending (a pending exception aborts the next JNI call).

use jni::JNIEnv;
use jni::objects::{JObject, JValue};

type JResult<T> = Result<T, String>;

/// Run `f` with a JNI environment attached to this thread and the activity object.
fn with_activity<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> JResult<T> {
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err("no Android context".into());
    }
    // SAFETY: `android-activity` initialises `ndk-context` with the process's `JavaVM` and a
    // global reference to the running activity before `android_main` starts, and both stay valid
    // for the life of the process. Neither pointer is freed here.
    #[allow(unsafe_code)]
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    // SAFETY: see above; the object is the activity's global reference, which we only borrow.
    #[allow(unsafe_code)]
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let result = f(&mut *env, &activity);
    let exception = if env.exception_check().unwrap_or(false) { Some(take_exception(&mut *env)) } else { None };
    result.map_err(|e| match exception {
        Some(text) => format!("{e}: {text}"),
        None => e.to_string(),
    })
}

fn sdk_int(env: &mut JNIEnv) -> jni::errors::Result<i32> {
    env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()
}

/// Whether the app can read and write shared storage (Download, Documents, ...): all-files access
/// on Android 11 and later, the storage permission before that.
pub fn has_storage_access() -> bool {
    with_activity(|env, activity| {
        if sdk_int(env)? >= 30 {
            env.call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])?.z()
        } else {
            let name = env.new_string("android.permission.WRITE_EXTERNAL_STORAGE")?;
            let granted = env.call_method(activity, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&name)])?.i()?;
            Ok(granted == 0)
        }
    })
    .unwrap_or_else(|e| {
        log::warn!("storage access check: {e}");
        false
    })
}

/// The pending Java exception as text (`android.content.ActivityNotFoundException: …`), cleared.
fn take_exception(env: &mut JNIEnv) -> String {
    let Ok(throwable) = env.exception_occurred() else { return "Java exception".into() };
    let _ = env.exception_clear();
    let text = env
        .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
        .and_then(|v| v.l())
        .and_then(|o| env.get_string(&o.into()).map(String::from));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    text.unwrap_or_else(|_| "Java exception".into())
}

/// Start the settings screen `action` (for this app's package when `for_package`). `Err` with the
/// Java exception's text (cleared) when the system refuses it.
fn try_settings(env: &mut JNIEnv, activity: &JObject, action: &str, for_package: bool) -> jni::errors::Result<Result<(), String>> {
    let attempt = |env: &mut JNIEnv| -> jni::errors::Result<()> {
        let action = env.new_string(action)?;
        let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
        if for_package {
            let package = env.call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
            let package: String = env.get_string(&package.into())?.into();
            let uri_text = env.new_string(format!("package:{package}"))?;
            let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&uri_text)])?.l()?;
            env.call_method(&intent, "setData", "(Landroid/net/Uri;)Landroid/content/Intent;", &[JValue::Object(&uri)])?;
        }
        // FLAG_ACTIVITY_NEW_TASK: some vendor builds refuse settings screens started into the
        // NativeActivity's own task.
        env.call_method(&intent, "addFlags", "(I)Landroid/content/Intent;", &[JValue::Int(0x1000_0000)])?;
        env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
        Ok(())
    };
    match attempt(env) {
        Ok(()) => Ok(Ok(())),
        Err(jni::errors::Error::JavaException) => {
            let text = take_exception(env);
            log::warn!("settings screen {action}: {text}");
            Ok(Err(text))
        }
        Err(e) => Err(e),
    }
}

/// Ask for shared-storage access: on Android 11 and later the system's "All files access" page
/// for this app, else the list of apps with that access, else the app's own settings page (some
/// vendor builds lack the first two); the permission dialog before Android 11.
pub fn request_storage_access() -> JResult<()> {
    let outcome = with_activity(|env, activity| {
        if sdk_int(env)? >= 30 {
            let mut first_error = None;
            for (action, for_package) in [
                ("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION", true),
                ("android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION", false),
                ("android.settings.APPLICATION_DETAILS_SETTINGS", true),
            ] {
                match try_settings(env, activity, action, for_package)? {
                    Ok(()) => return Ok(Ok(())),
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                }
            }
            Ok(Err(first_error.unwrap_or_default()))
        } else {
            let names = env.new_object_array(2, "java/lang/String", JObject::null())?;
            let read = env.new_string("android.permission.READ_EXTERNAL_STORAGE")?;
            let write = env.new_string("android.permission.WRITE_EXTERNAL_STORAGE")?;
            env.set_object_array_element(&names, 0, read)?;
            env.set_object_array_element(&names, 1, write)?;
            env.call_method(activity, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&names), JValue::Int(1)])?;
            Ok(Ok(()))
        }
    })?;
    outcome
}

/// The system language as a BCP 47 tag (`ru-RU`), for the interface's Auto language.
pub fn system_language() -> Option<String> {
    with_activity(|env, _| {
        let locale = env.call_static_method("java/util/Locale", "getDefault", "()Ljava/util/Locale;", &[])?.l()?;
        let tag = env.call_method(&locale, "toLanguageTag", "()Ljava/lang/String;", &[])?.l()?;
        let tag: String = env.get_string(&tag.into())?.into();
        Ok(tag)
    })
    .map_err(|e| log::warn!("system language: {e}"))
    .ok()
}
