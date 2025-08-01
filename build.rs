use android_build::{DebugInfo, Dexer, JavaBuild};
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=java/");
    if !env::var("TARGET").unwrap().contains("android") {
        return;
    }

    let android_jar = android_build::android_jar(None).expect("Unable to locate android.jar path");
    let is_release_build = env::var("PROFILE") == Ok("release".to_owned());
    let java_path = "java/net/octet_stream/netwatcher/NetwatcherSupportAndroid.java";
    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let classes_out_dir = out_dir.join("java/net/octet_stream/netwatcher");
    let _ = std::fs::remove_dir_all(&classes_out_dir);
    std::fs::create_dir_all(&classes_out_dir).unwrap();
    JavaBuild::new()
        .file(java_path)
        .class_path(&android_jar)
        .classes_out_dir(&classes_out_dir)
        .java_source_version(8)
        .java_target_version(8)
        .debug_info(debug_info(is_release_build))
        .compile()
        .expect("java build failed");

    let dex_output_dir = out_dir.join("dex");
    std::fs::create_dir_all(&dex_output_dir).unwrap();
    let java_classes_root = out_dir.join("java");

    Dexer::new()
        .android_jar(&android_jar)
        .class_path(&java_classes_root)
        .collect_classes(&java_classes_root)
        .unwrap()
        .release(is_release_build)
        .android_min_api(21)
        .out_dir(&dex_output_dir)
        .run()
        .expect("dexing failed");

    let dex_path = dex_output_dir.join("classes.dex");
    if dex_path.exists() {
        println!("cargo:rustc-env=NETWATCHER_DEX_PATH={}", dex_path.display());
    } else {
        panic!(
            "DEX file was not created at expected location: {}",
            dex_path.display()
        );
    }
}

fn debug_info(is_release_build: bool) -> DebugInfo {
    DebugInfo {
        line_numbers: !is_release_build,
        source_files: !is_release_build,
        variables: !is_release_build,
    }
}
