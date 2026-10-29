use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let builder = CxxQtBuilder::new_qml_module(QmlModule::new("se.cptjanst.babel").qml_files([
        "qml/Main.qml",
        "qml/ProjectsPage.qml",
        "qml/AccountDialog.qml",
        "qml/NativePage.qml",
        "qml/LanguageDialog.qml",
        "qml/VersionsDialog.qml",
        "qml/SettingsDialog.qml",
        "qml/HelpDialog.qml",
        "qml/PairDialog.qml",
    ]))
    .files(["src/bridge.rs"])
    .qt_module("Gui")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .include_dir("include")
    .cpp_file("src/cpp/init.cpp");
    builder.build();
}
