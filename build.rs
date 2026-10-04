use std::{env, path::PathBuf, process::Command};
fn main() {
    let qt = pkg_config::Config::new()
        .atleast_version("6.5")
        .probe("Qt6QuickControls2")
        .expect("Qt 6 development packages are required; see README.linux.md");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let tools = env::var_os("QT_HOST_PATH")
        .map(PathBuf::from)
        .map(|p| p.join("libexec"))
        .unwrap_or_else(|| {
            let p = Command::new("pkg-config")
                .args(["--variable=libexecdir", "Qt6Core"])
                .output()
                .unwrap();
            PathBuf::from(String::from_utf8(p.stdout).unwrap().trim())
        });
    let tool = |name: &str| {
        let path = tools.join(name);
        if path.exists() {
            path
        } else {
            PathBuf::from(format!("/usr/lib/qt6/{name}"))
        }
    };
    for (name, args) in [
        (
            "moc",
            vec![
                "native/bridge.h".into(),
                "-o".into(),
                output.join("moc_bridge.cpp").into_os_string(),
            ],
        ),
        (
            "rcc",
            vec![
                "qml/resources.qrc".into(),
                "-name".into(),
                "resources".into(),
                "-o".into(),
                output.join("qml_resources.cpp").into_os_string(),
            ],
        ),
    ] {
        let status = Command::new(tool(name))
            .args(args)
            .status()
            .expect("Cannot run Qt build tool");
        assert!(status.success(), "Qt {name} failed");
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-fPIC")
        .include("native")
        .file("native/bridge.cpp")
        .file(output.join("moc_bridge.cpp"))
        .file(output.join("qml_resources.cpp"));
    for include in qt.include_paths {
        build.include(include);
    }
    build.compile("chess_qt_bridge");
    for path in [
        "native",
        "qml",
        "assets/fonts",
        "Resources/Icons/Chess_128x128.png",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
