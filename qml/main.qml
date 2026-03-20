import QtQuick
import QtQuick.Controls
import com.squeak.terminal

ApplicationWindow {
    id: root
    visible: true
    width: 800
    height: 600
    title: "Squeak"
    color: "#1e1e1e"

    TerminalView {
        id: terminal
        Component.onCompleted: start_reader()
    }

    Text {
        id: display
        anchors.fill: parent
        anchors.margins: 4
        text: terminal.screen_text
        font.family: "monospace"
        font.pixelSize: 14
        color: "#d4d4d4"
        wrapMode: Text.NoWrap
    }

    FocusScope {
        anchors.fill: parent
        focus: true

        Item {
            anchors.fill: parent
            focus: true

            Keys.onPressed: (event) => {
                terminal.key_pressed(event.text, event.key)
                event.accepted = true
            }

            Component.onCompleted: forceActiveFocus()
        }
    }
}
