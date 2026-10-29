#include "babel-desktop/init.h"

#include <QtCore/QCoreApplication>
#include <QtGui/QClipboard>
#include <QtGui/QGuiApplication>
#include <QtCore/QtGlobal>
#include <QtQml/QQmlApplicationEngine>
#include <QtQuickControls2/QQuickStyle>
#include <QtWebEngineQuick/QtWebEngineQuick>

void babel_init_webengine()
{
    QtWebEngineQuick::initialize();
}

void babel_init_style()
{
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
        QQuickStyle::setStyle(QStringLiteral("org.kde.desktop"));
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
