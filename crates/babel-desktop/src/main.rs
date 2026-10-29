//! Babel for the desktop: a Kirigami window around the translation editor,
//! working on this computer and syncing with a Babel server when signed in.

mod app;
mod bridge;
mod native;
mod proof;
mod pages;
mod sync;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();
    app::App::start()?;

    bridge::babel_init_webengine();
    let mut gui = QGuiApplication::new();
    if let Some(mut g) = gui.as_mut() {
        g.as_mut().set_application_name(&QString::from("babel"));
        g.as_mut().set_application_display_name(&QString::from("Babel"));
        g.as_mut().set_organization_domain(&QString::from("cptjanst.se"));
        g.as_mut().set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
        QGuiApplication::set_desktop_file_name(&QString::from("se.cptjanst.babel"));
    }
    bridge::babel_init_style();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        bridge::babel_exit_on_load_failure(engine.as_mut());
        engine.load(&QUrl::from("qrc:/qt/qml/se/cptjanst/babel/qml/Main.qml"));
    }
    let code = gui.as_mut().map(|g| g.exec()).unwrap_or(1);
    std::process::exit(code);
}
