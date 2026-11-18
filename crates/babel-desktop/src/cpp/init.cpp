#include "babel-desktop/init.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtGui/QIcon>
#include <QtGui/QImage>
#include <QtGui/QKeyEvent>
#include <QtGui/QMouseEvent>
#include <QtQuick/QQuickWindow>
#include <QtGui/QClipboard>
#include <QtGui/QGuiApplication>
#include <QtCore/QtGlobal>
#include <QtQml/QQmlApplicationEngine>
#include <QtQuickControls2/QQuickStyle>

// The controls' style, and on macOS the icons. On Linux, KDE's desktop style
// and the system's icon theme. On macOS (packaging/macos/build-macos.sh) there
// is neither: Qt's Fusion style, and the Breeze icons the app bundle carries
// in Contents/Resources/icons. BABEL_ICONS names such a folder anywhere.
void babel_init_style()
{
    QString icons = qEnvironmentVariable("BABEL_ICONS");
#ifdef Q_OS_MACOS
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
        QQuickStyle::setStyle(QStringLiteral("Fusion"));
    if (icons.isEmpty())
        icons = QCoreApplication::applicationDirPath() + QStringLiteral("/../Resources/icons");
#else
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
        QQuickStyle::setStyle(QStringLiteral("org.kde.desktop"));
#endif
    if (!icons.isEmpty() && QDir(icons + QStringLiteral("/breeze")).exists()) {
        QIcon::setThemeSearchPaths(QStringList{icons} + QIcon::themeSearchPaths());
        QIcon::setThemeName(QStringLiteral("breeze"));
    }
}

void babel_exit_on_load_failure(QQmlApplicationEngine &engine)
{
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, QCoreApplication::instance(),
        [] { QCoreApplication::exit(1); }, Qt::QueuedConnection);
}

void babel_clipboard_set(const QString &text)
{
    QGuiApplication::clipboard()->setText(text);
}

QString babel_clipboard_text()
{
    return QGuiApplication::clipboard()->text();
}

static QQuickWindow *mainWindow()
{
    for (QWindow *w : QGuiApplication::topLevelWindows())
        if (auto *q = qobject_cast<QQuickWindow *>(w); q && q->isVisible())
            return q;
    return nullptr;
}

void babel_test_mouse(int kind, double x, double y, int modifiers)
{
    QQuickWindow *w = mainWindow();
    if (!w)
        return;
    const QPointF p(x, y);
    const auto mods = Qt::KeyboardModifiers(modifiers);
    // 4, 5: right button press, release.
    const bool right = kind == 4 || kind == 5;
    if (right)
        kind -= 3;
    const auto type = kind == 0 ? QEvent::MouseMove : kind == 1 ? QEvent::MouseButtonPress : kind == 2 ? QEvent::MouseButtonRelease : QEvent::MouseButtonDblClick;
    static bool down = false;
    if (kind == 1 || kind == 3)
        down = true;
    if (kind == 2)
        down = false;
    const Qt::MouseButton button = kind == 0 ? Qt::NoButton : right ? Qt::RightButton : Qt::LeftButton;
    const Qt::MouseButtons buttons = (kind == 1) ? Qt::MouseButtons(button) : (down && !right ? Qt::LeftButton : Qt::NoButton);
    QMouseEvent e(type, p, p, w->mapToGlobal(p), button, buttons, mods);
    QCoreApplication::sendEvent(w, &e);
}

void babel_test_key(int key, const QString &text, int modifiers)
{
    QQuickWindow *w = mainWindow();
    if (!w)
        return;
    const auto mods = Qt::KeyboardModifiers(modifiers);
    QKeyEvent press(QEvent::KeyPress, key, mods, text);
    QCoreApplication::sendEvent(w, &press);
    QKeyEvent release(QEvent::KeyRelease, key, mods, text);
    QCoreApplication::sendEvent(w, &release);
}

bool babel_test_grab(const QString &path)
{
    QQuickWindow *w = mainWindow();
    return w && w->grabWindow().save(path);
}
