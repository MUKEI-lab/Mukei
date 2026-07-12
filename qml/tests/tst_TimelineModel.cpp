#include "../timeline_model.h"

#include <QtTest/QTest>

class TimelineModelTest final : public QObject
{
    Q_OBJECT

private slots:
    void appliesSnapshotAndUpdatesOneRow()
    {
        MukeiTimelineModel model;
        const QString snapshot = QStringLiteral(R"json({
            "items": [
                {"rowId":"u1","type":"user_message","text":"hello","status":"completed"},
                {"rowId":"a1","type":"assistant_message","text":"partial","status":"streaming"}
            ],
            "has_older": true,
            "oldest_message_id": "u1"
        })json");

        QVERIFY(model.replaceFromSnapshotJson(snapshot));
        QCOMPARE(model.rowCount(), 2);
        QVERIFY(model.hasOlder());
        QCOMPARE(model.oldestMessageId(), QStringLiteral("u1"));
        QCOMPARE(model.data(model.index(1, 0), MukeiTimelineModel::TextRole).toString(),
                 QStringLiteral("partial"));

        QVERIFY(model.appendText(QStringLiteral("a1"), QStringLiteral(" response")));
        QCOMPARE(model.data(model.index(1, 0), MukeiTimelineModel::TextRole).toString(),
                 QStringLiteral("partial response"));
        QVERIFY(model.updateStatus(QStringLiteral("a1"), QStringLiteral("completed")));
        QCOMPARE(model.data(model.index(1, 0), MukeiTimelineModel::StatusRole).toString(),
                 QStringLiteral("completed"));
    }

    void prependsWithoutDuplicatingRows()
    {
        MukeiTimelineModel model;
        QVERIFY(model.replaceFromSnapshotJson(QStringLiteral(R"json({
            "items": [{"rowId":"m2","type":"assistant_message","text":"new"}],
            "has_older": true,
            "oldest_message_id": "m2"
        })json")));
        QVERIFY(model.prependFromSnapshotJson(QStringLiteral(R"json({
            "items": [
                {"rowId":"m1","type":"user_message","text":"old"},
                {"rowId":"m2","type":"assistant_message","text":"duplicate"}
            ],
            "has_older": false,
            "oldest_message_id": "m1"
        })json")));

        QCOMPARE(model.rowCount(), 2);
        QCOMPARE(model.data(model.index(0, 0), MukeiTimelineModel::RowIdRole).toString(),
                 QStringLiteral("m1"));
        QCOMPARE(model.data(model.index(1, 0), MukeiTimelineModel::TextRole).toString(),
                 QStringLiteral("new"));
        QVERIFY(!model.hasOlder());
    }
};

QTEST_GUILESS_MAIN(TimelineModelTest)
#include "tst_TimelineModel.moc"
