#pragma once
#include <QObject>
#include <QVariantMap>
#include <QQuickWindow>
#include <cstddef>
#include <cstdint>
struct ChessFrame { unsigned char *data; std::size_t len; std::uint32_t width, height; };
using Dispatch = char *(*)(void *, const char *);
using FreeText = void (*)(char *);
using Render = ChessFrame (*)(void *, std::uint32_t, std::uint32_t);
using FreeFrame = void (*)(ChessFrame);
using Record = void (*)(void *, const unsigned char *, std::uint32_t, std::uint32_t);
class ChessBridge final : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap state READ state NOTIFY stateChanged)
public:
    ChessBridge(void *context, Dispatch dispatch, FreeText freeText, Record record);
    QVariantMap state() const { return m_state; }
    Q_INVOKABLE QVariantMap command(const QVariantMap &request);
    Q_INVOKABLE void copy(const QString &text);
    void tick();
    QQuickWindow *window = nullptr;
signals:
    void stateChanged();
    void uiAction(const QString &action);
private:
    void update(const QVariantMap &state);
    void *m_context;
    Dispatch m_dispatch;
    FreeText m_freeText;
    Record m_record;
    QVariantMap m_state;
};
