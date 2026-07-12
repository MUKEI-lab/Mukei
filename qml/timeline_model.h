#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QJsonObject>
#include <QList>
#include <QVariantMap>

class MukeiTimelineModel final : public QAbstractListModel
{
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)
    Q_PROPERTY(bool hasOlder READ hasOlder NOTIFY pageStateChanged)
    Q_PROPERTY(QString oldestMessageId READ oldestMessageId NOTIFY pageStateChanged)

public:
    enum Role {
        RowIdRole = Qt::UserRole + 1,
        TypeRole,
        TextRole,
        PhaseRole,
        KindRole,
        StatusRole,
        TimestampRole,
        ToolNameRole,
        ParentIdRole,
        ConversationIdRole,
        BranchIdRole
    };
    Q_ENUM(Role)

    explicit MukeiTimelineModel(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    int count() const { return m_rows.size(); }
    QVariant data(const QModelIndex &index, int role = Qt::DisplayRole) const override;
    QHash<int, QByteArray> roleNames() const override;

    bool hasOlder() const { return m_hasOlder; }
    QString oldestMessageId() const { return m_oldestMessageId; }

    Q_INVOKABLE void clear();
    Q_INVOKABLE int indexOfRowId(const QString &rowId) const;
    Q_INVOKABLE void appendRow(const QVariantMap &row);
    Q_INVOKABLE void upsertRow(const QVariantMap &row);
    Q_INVOKABLE bool appendText(const QString &rowId, const QString &chunk);
    Q_INVOKABLE bool updateStatus(const QString &rowId, const QString &status);
    Q_INVOKABLE bool replaceFromSnapshotJson(const QString &snapshotJson);
    Q_INVOKABLE bool prependFromSnapshotJson(const QString &snapshotJson);

signals:
    void countChanged();
    void pageStateChanged();
    void snapshotRejected(const QString &safeReason);

private:
    struct Row {
        QString rowId;
        QString type;
        QString text;
        QString phase;
        QString kind;
        QString status;
        QString timestamp;
        QString toolName;
        QString parentId;
        QString conversationId;
        QString branchId;
    };

    static Row rowFromMap(const QVariantMap &map);
    static QVariantMap mapFromJsonObject(const QJsonObject &object);
    static bool parseSnapshot(const QString &json, QList<Row> *rows, bool *hasOlder, QString *oldestId);
    void rebuildIndex();
    void setPageState(bool hasOlder, const QString &oldestId);

    QList<Row> m_rows;
    QHash<QString, int> m_indexById;
    bool m_hasOlder = false;
    QString m_oldestMessageId;
};
