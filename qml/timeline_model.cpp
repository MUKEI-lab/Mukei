#include "timeline_model.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

MukeiTimelineModel::MukeiTimelineModel(QObject *parent)
    : QAbstractListModel(parent)
{
}

int MukeiTimelineModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : m_rows.size();
}

QVariant MukeiTimelineModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_rows.size())
        return {};

    const Row &row = m_rows.at(index.row());
    switch (role) {
    case RowIdRole: return row.rowId;
    case TypeRole: return row.type;
    case TextRole: return row.text;
    case PhaseRole: return row.phase;
    case KindRole: return row.kind;
    case StatusRole: return row.status;
    case TimestampRole: return row.timestamp;
    case ToolNameRole: return row.toolName;
    case ParentIdRole: return row.parentId;
    case ConversationIdRole: return row.conversationId;
    case BranchIdRole: return row.branchId;
    default: return {};
    }
}

QHash<int, QByteArray> MukeiTimelineModel::roleNames() const
{
    return {
        {RowIdRole, "rowId"},
        {TypeRole, "type"},
        {TextRole, "text"},
        {PhaseRole, "phase"},
        {KindRole, "kind"},
        {StatusRole, "status"},
        {TimestampRole, "timestamp"},
        {ToolNameRole, "toolName"},
        {ParentIdRole, "parentId"},
        {ConversationIdRole, "conversationId"},
        {BranchIdRole, "branchId"}
    };
}

void MukeiTimelineModel::clear()
{
    if (m_rows.isEmpty()) {
        setPageState(false, {});
        return;
    }
    beginResetModel();
    m_rows.clear();
    m_indexById.clear();
    endResetModel();
    setPageState(false, {});
    emit countChanged();
}

int MukeiTimelineModel::indexOfRowId(const QString &rowId) const
{
    return m_indexById.value(rowId, -1);
}

MukeiTimelineModel::Row MukeiTimelineModel::rowFromMap(const QVariantMap &map)
{
    Row row;
    row.rowId = map.value(QStringLiteral("rowId")).toString();
    row.type = map.value(QStringLiteral("type"), QStringLiteral("timeline_event")).toString();
    row.text = map.value(QStringLiteral("text")).toString();
    row.phase = map.value(QStringLiteral("phase")).toString();
    row.kind = map.value(QStringLiteral("kind")).toString();
    row.status = map.value(QStringLiteral("status")).toString();
    row.timestamp = map.value(QStringLiteral("timestamp")).toString();
    row.toolName = map.value(QStringLiteral("toolName")).toString();
    row.parentId = map.value(QStringLiteral("parentId")).toString();
    row.conversationId = map.value(QStringLiteral("conversationId")).toString();
    row.branchId = map.value(QStringLiteral("branchId")).toString();
    return row;
}

QVariantMap MukeiTimelineModel::mapFromJsonObject(const QJsonObject &object)
{
    QVariantMap map;
    for (auto it = object.constBegin(); it != object.constEnd(); ++it)
        map.insert(it.key(), it.value().toVariant());
    return map;
}

void MukeiTimelineModel::appendRow(const QVariantMap &rowMap)
{
    Row row = rowFromMap(rowMap);
    if (row.rowId.isEmpty())
        return;
    if (indexOfRowId(row.rowId) >= 0) {
        upsertRow(rowMap);
        return;
    }
    const int index = m_rows.size();
    beginInsertRows({}, index, index);
    m_rows.append(row);
    m_indexById.insert(row.rowId, index);
    endInsertRows();
    if (m_oldestMessageId.isEmpty())
        setPageState(m_hasOlder, row.rowId);
    emit countChanged();
}

void MukeiTimelineModel::upsertRow(const QVariantMap &rowMap)
{
    Row row = rowFromMap(rowMap);
    if (row.rowId.isEmpty())
        return;
    const int index = indexOfRowId(row.rowId);
    if (index < 0) {
        appendRow(rowMap);
        return;
    }
    m_rows[index] = row;
    const QModelIndex modelIndex = createIndex(index, 0);
    emit dataChanged(modelIndex, modelIndex);
}

bool MukeiTimelineModel::appendText(const QString &rowId, const QString &chunk)
{
    const int index = indexOfRowId(rowId);
    if (index < 0)
        return false;
    m_rows[index].text += chunk;
    m_rows[index].status = QStringLiteral("streaming");
    const QModelIndex modelIndex = createIndex(index, 0);
    emit dataChanged(modelIndex, modelIndex, {TextRole, StatusRole});
    return true;
}

bool MukeiTimelineModel::updateStatus(const QString &rowId, const QString &status)
{
    const int index = indexOfRowId(rowId);
    if (index < 0)
        return false;
    m_rows[index].status = status;
    const QModelIndex modelIndex = createIndex(index, 0);
    emit dataChanged(modelIndex, modelIndex, {StatusRole});
    return true;
}

bool MukeiTimelineModel::parseSnapshot(
    const QString &json,
    QList<Row> *rows,
    bool *hasOlder,
    QString *oldestId)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(json.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return false;

    const QJsonObject root = document.object();
    const QJsonArray items = root.value(QStringLiteral("items")).toArray();
    QList<Row> parsed;
    parsed.reserve(items.size());
    for (const QJsonValue &value : items) {
        if (!value.isObject())
            continue;
        const Row row = rowFromMap(mapFromJsonObject(value.toObject()));
        if (!row.rowId.isEmpty())
            parsed.append(row);
    }
    *rows = parsed;
    *hasOlder = root.value(QStringLiteral("has_older")).toBool(false);
    *oldestId = root.value(QStringLiteral("oldest_message_id")).toString();
    if (oldestId->isEmpty() && !parsed.isEmpty())
        *oldestId = parsed.first().rowId;
    return true;
}

bool MukeiTimelineModel::replaceFromSnapshotJson(const QString &snapshotJson)
{
    QList<Row> rows;
    bool hasOlder = false;
    QString oldestId;
    if (!parseSnapshot(snapshotJson, &rows, &hasOlder, &oldestId)) {
        emit snapshotRejected(QStringLiteral("invalid_chat_snapshot"));
        return false;
    }
    beginResetModel();
    m_rows = rows;
    rebuildIndex();
    endResetModel();
    setPageState(hasOlder, oldestId);
    emit countChanged();
    return true;
}

bool MukeiTimelineModel::prependFromSnapshotJson(const QString &snapshotJson)
{
    QList<Row> incoming;
    bool hasOlder = false;
    QString oldestId;
    if (!parseSnapshot(snapshotJson, &incoming, &hasOlder, &oldestId)) {
        emit snapshotRejected(QStringLiteral("invalid_chat_snapshot"));
        return false;
    }

    QList<Row> unique;
    unique.reserve(incoming.size());
    for (const Row &row : incoming) {
        if (!m_indexById.contains(row.rowId))
            unique.append(row);
    }
    if (!unique.isEmpty()) {
        beginInsertRows({}, 0, unique.size() - 1);
        for (int i = unique.size() - 1; i >= 0; --i)
            m_rows.prepend(unique.at(i));
        endInsertRows();
        rebuildIndex();
        emit countChanged();
    }
    setPageState(hasOlder, oldestId);
    return true;
}

void MukeiTimelineModel::rebuildIndex()
{
    m_indexById.clear();
    for (int i = 0; i < m_rows.size(); ++i)
        m_indexById.insert(m_rows.at(i).rowId, i);
}

void MukeiTimelineModel::setPageState(bool hasOlder, const QString &oldestId)
{
    if (m_hasOlder == hasOlder && m_oldestMessageId == oldestId)
        return;
    m_hasOlder = hasOlder;
    m_oldestMessageId = oldestId;
    emit pageStateChanged();
}
