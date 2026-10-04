#include "bridge.h"
#include <QGuiApplication>
#include <QQuickStyle>
#include <algorithm>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickImageProvider>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTimer>
#include <QClipboard>
#include <QFontDatabase>
#include <QIcon>
#include <QDebug>
#include <cstdio>
#include <atomic>
#include <memory>
#include <QQmlError>

class BoardImages final : public QQuickImageProvider {
public:
    BoardImages(void *context, Render render, FreeFrame freeFrame)
        : QQuickImageProvider(QQuickImageProvider::Image), m_context(context), m_render(render), m_freeFrame(freeFrame) {}
    QImage requestImage(const QString &, QSize *size, const QSize &requested) override {
        const auto w = std::clamp(requested.width(), 64, 4096);
        const auto h = std::clamp(requested.height(), 64, 4096);
        auto frame = m_render(m_context, w, h);
        if (!frame.data) return QImage();
        QImage image(frame.data, frame.width, frame.height, frame.width * 4, QImage::Format_RGBA8888);
        auto result = image.copy();
        if (size) *size = result.size();
        m_freeFrame(frame);
        return result;
    }
private:
    void *m_context;
    Render m_render;
    FreeFrame m_freeFrame;
};
ChessBridge::ChessBridge(void *context, Dispatch dispatch, FreeText freeText, Record record)
    : m_context(context), m_dispatch(dispatch), m_freeText(freeText), m_record(record) {
    command({{"command", "status"}});
}
QVariantMap ChessBridge::command(const QVariantMap &request) {
    const auto json = QJsonDocument::fromVariant(request).toJson(QJsonDocument::Compact);
    auto text = m_dispatch(m_context, json.constData());
    const auto response = QJsonDocument::fromJson(QByteArray(text)).object().toVariantMap();
    m_freeText(text);
    if (response.value("ok").toBool()) update(response.value("data").toMap());
    return response;
}
void ChessBridge::copy(const QString &text) { QGuiApplication::clipboard()->setText(text); }
void ChessBridge::update(const QVariantMap &state) {
    if (state != m_state) { m_state = state; emit stateChanged(); }
    for (const auto &value : state.value("ui_events").toList()) {
        const auto event = value.toMap();
        const auto data = event.value("data").toMap();
        const auto name = event.value("name").toString();
        if (name == "clipboard") copy(data.value("text").toString());
        if (name == "gui") emit uiAction(data.value("action").toString());
        if (name == "ui_resize" && window) window->resize(data.value("width").toInt(), data.value("height").toInt());
        if (name == "gui_screenshot" && window) {
            const auto path = data.value("path").toString();
            QTimer::singleShot(250, this, [this, path] {
                if (!window) return;
                const auto image = window->grabWindow();
                const QSize size(qRound(window->width() * window->devicePixelRatio()),
                                 qRound(window->height() * window->devicePixelRatio()));
                image.copy(QRect(QPoint(), size)).save(path);
            });
        }
    }
    if (state.value("close_allowed").toBool()) QGuiApplication::quit();
}
void ChessBridge::tick() {
    command({{"command", "poll"}});
    if (window && m_state.value("recording").toBool()) {
        const auto image = window->grabWindow().convertToFormat(QImage::Format_RGBA8888);
        if (!image.isNull()) m_record(m_context, image.constBits(), image.width(), image.height());
    }
}
static void initializeResources() { Q_INIT_RESOURCE(resources); }
extern "C" int chess_qt_run(void *context, Dispatch dispatch, FreeText freeText,
                            Render render, FreeFrame freeFrame, Record record) {
    initializeResources();
    qputenv("QSG_RHI_BACKEND", "vulkan");
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Vulkan);
    int argc = 1;
    char name[] = "chess-linux";
    char *argv[] = {name, nullptr};
    QGuiApplication application(argc, argv);
    application.setApplicationName("Chess");
    application.setApplicationDisplayName("Chess");
    application.setDesktopFileName("chess-linux");
    application.setOrganizationName("ChessLinux");
    application.setWindowIcon(QIcon(":/icons/chess.png"));
    QQuickStyle::setStyle("org.kde.desktop");
    for (const auto &font : {"NotoSans-Regular.ttf", "NotoSans-Medium.ttf", "NotoSerif-Regular.ttf"})
        QFontDatabase::addApplicationFont(QString(":/fonts/") + font);
    ChessBridge bridge(context, dispatch, freeText, record);
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty("chess", &bridge);
    engine.addImageProvider("board", new BoardImages(context, render, freeFrame));
    QObject::connect(&engine, &QQmlEngine::warnings, &bridge, [](const QList<QQmlError> &errors) { for (const auto &error : errors) std::fprintf(stderr, "%s\n", qPrintable(error.toString())); });
    engine.load(QUrl("qrc:/qml/Main.qml"));
    if (engine.rootObjects().isEmpty()) return 1;
    bridge.window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
    if (!bridge.window) return 2;
    auto verified = std::make_shared<std::atomic_bool>(false);
    QObject::connect(bridge.window, &QQuickWindow::beforeRendering, &bridge, [&, verified] {
        if (!verified->exchange(true)) {
            if (bridge.window->rendererInterface()->graphicsApi() != QSGRendererInterface::Vulkan)
                qFatal("The Kirigami interface requires Vulkan");
            std::fprintf(stderr, "Kirigami / Qt Quick Vulkan scene graph verified\n");
        }
    }, Qt::DirectConnection);
    QTimer timer;
    QObject::connect(&timer, &QTimer::timeout, &bridge, &ChessBridge::tick);
    timer.start(33);
    return application.exec();
}
