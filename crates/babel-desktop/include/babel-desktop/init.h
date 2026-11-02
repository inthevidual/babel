#pragma once

void babel_init_style();

class QQmlApplicationEngine;
class QString;

void babel_clipboard_set(const QString &text);
QString babel_clipboard_text();
/// Quit with an error if the main QML file fails to load, instead of
/// running on without a window.
void babel_exit_on_load_failure(QQmlApplicationEngine &engine);

/// UI test driver: real events into the app's window, through Qt's normal delivery.
/// kind: 0 move, 1 press, 2 release, 3 double-click (after a press).
void babel_test_mouse(int kind, double x, double y, int modifiers);
void babel_test_key(int key, const QString &text, int modifiers);
bool babel_test_grab(const QString &path);
