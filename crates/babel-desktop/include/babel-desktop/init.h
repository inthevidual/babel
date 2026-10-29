#pragma once

void babel_init_webengine();
void babel_init_style();

class QQmlApplicationEngine;
/// Quit with an error if the main QML file fails to load, instead of
/// running on without a window.
void babel_exit_on_load_failure(QQmlApplicationEngine &engine);
