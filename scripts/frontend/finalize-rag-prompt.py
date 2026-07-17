#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / "qml/screens/RagRebuildPromptScreen.qml"

text = PATH.read_text(encoding="utf-8")

if 'import "../architecture"\n' not in text:
    text = text.replace(
        'import "../theme"\n',
        'import "../architecture"\nimport "../theme"\n',
        1,
    )

if 'objectName: "ragRebuildPromptScreen"' not in text:
    text = text.replace(
        "Page {\n    id: root\n",
        'Page {\n    id: root\n    objectName: "ragRebuildPromptScreen"\n',
        1,
    )

if 'objectName: "ragRebuildUnavailableButton"' not in text:
    text = text.replace(
        '''        RowLayout {
            PrimaryButton {
                text: qsTr("Rebuild now")
            }
            GhostButton {
                text: qsTr("Skip for now")
            }
        }
''',
        '''        Text {
            Layout.fillWidth: true
            text: qsTr("Rebuild is not available in this runtime yet. No indexing action will be simulated or started silently.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
        RowLayout {
            PrimaryButton {
                objectName: "ragRebuildUnavailableButton"
                // interaction-audit: exempt — awaiting a supported local rebuild operation.
                text: qsTr("Rebuild unavailable")
                enabled: false
            }
            GhostButton {
                objectName: "ragRebuildSkipButton"
                text: qsTr("Skip for now")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
        }
''',
        1,
    )

PATH.write_text(text, encoding="utf-8")
print("RAG prompt actions finalized")
