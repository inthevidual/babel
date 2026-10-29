#pragma once

void babel_init_webengine();
void babel_init_style();

class QQmlApplicationEngine;
class QString;

void babel_clipboard_set(const QString &text);
QString babel_clipboard_text();
/// Quit with an error if the main QML file fails to load, instead of
/// running on without a window.
void babel_exit_on_load_failure(QQmlApplicationEngine &engine);
