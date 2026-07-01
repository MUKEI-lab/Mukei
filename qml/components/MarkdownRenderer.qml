import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Column { id: root; property var ast: []; property string fallbackText: ""; spacing: Spacing.sm; Repeater { model: root.ast && root.ast.length ? root.ast : [{type:"paragraph", text: root.fallbackText}]; delegate: Text { width: root.width; text: modelData.text || ""; wrapMode: Text.Wrap; textFormat: Text.PlainText; color: Theme.p.inkPrimary; font: Type.bodyAI } } }
