// One rice on the rail: screenshot (or wallpaper) on top, a band in the rice's own colours
// below, and its kanji on a hanging tag.
import QtQuick
import QtQuick.Effects

Item {
    id: root

    required property var card
    required property var look          // card.light or card.dark
    required property string mood       // "light" | "dark"
    required property bool current      // the rice on screen now
    required property bool selected
    required property color accent      // the rice's most vivid colour
    property real dim: 0                // 0 = lit, 1 = pushed back
    property int dur: 250

    signal picked()
    signal moodPicked(string mood)

    readonly property var pal: look && look.palette ? look.palette
        : { background: "#1b1b22", surface: "#22222b", text: "#e8e4da", accent: "#c8a96b", swatches: [] }
    readonly property real u: width / 900       // design unit: sizes are drawn at 900px wide
    readonly property real imageH: width * 9 / 16
    readonly property real bandH: 132 * u
    readonly property real radius: 22 * u
    readonly property string image: card.preview ? card.preview : (look && look.wallpaper ? look.wallpaper : "")

    implicitHeight: imageH + bandH

    function fileUrl(p) { return p ? "file://" + encodeURI(p) : "" }

    RectangularShadow {
        anchors.fill: body
        radius: root.radius
        blur: 60 * root.u
        spread: 0
        offset.y: 26 * root.u
        color: Qt.rgba(0, 0, 0, 0.55 * (1 - root.dim * 0.5))
    }

    Item {
        id: body
        anchors.fill: parent
        layer.enabled: true
        layer.smooth: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: mask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1.0
        }

        Rectangle {
            anchors.fill: parent
            color: root.pal.background
            Behavior on color { ColorAnimation { duration: root.dur } }
        }

        Image {
            id: shot
            width: parent.width
            height: root.imageH
            source: root.fileUrl(root.image)
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            sourceSize.width: 1280
            smooth: true
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: root.dur } }
        }

        // the screenshot sinks into the band instead of stopping at a hard line
        Rectangle {
            width: parent.width
            height: 90 * root.u
            y: root.imageH - height
            gradient: Gradient {
                GradientStop { position: 0; color: "transparent" }
                GradientStop { position: 1; color: Qt.alpha(root.pal.background, 0.85) }
            }
        }

        // band: the rice's own window colour and text colour, so the card wears it
        Item {
            id: band
            y: root.imageH
            width: parent.width
            height: root.bandH

            Column {
                anchors.left: parent.left
                anchors.leftMargin: 38 * root.u
                anchors.verticalCenter: parent.verticalCenter
                anchors.verticalCenterOffset: -2 * root.u
                spacing: 6 * root.u

                Text {
                    text: root.card.name
                    color: root.pal.text
                    font.family: "Noto Serif CJK JP"
                    font.pixelSize: 38 * root.u
                    font.weight: Font.DemiBold
                    Behavior on color { ColorAnimation { duration: root.dur } }
                }
                Text {
                    text: root.card.tagline || ""
                    visible: text.length > 0
                    color: Qt.alpha(root.pal.text, 0.62)
                    font.pixelSize: 17 * root.u
                    font.letterSpacing: 0.4 * root.u
                    Behavior on color { ColorAnimation { duration: root.dur } }
                }
            }

            Column {
                anchors.right: parent.right
                anchors.rightMargin: 38 * root.u
                anchors.verticalCenter: parent.verticalCenter
                spacing: 14 * root.u

                // palette swatches
                Row {
                    anchors.right: parent.right
                    spacing: -6 * root.u
                    Repeater {
                        model: root.pal.swatches
                        Rectangle {
                            required property string modelData
                            width: 26 * root.u
                            height: width
                            radius: width / 2
                            color: modelData
                            border.width: 2.5 * root.u
                            border.color: root.pal.background
                            Behavior on color { ColorAnimation { duration: root.dur } }
                        }
                    }
                }

                // day / night, only for rices that have both
                Row {
                    anchors.right: parent.right
                    visible: root.card.daynight
                    spacing: 4 * root.u
                    Repeater {
                        model: [{ m: "light", g: "☀", t: "day" }, { m: "dark", g: "☾", t: "night" }]
                        Rectangle {
                            id: pill
                            required property var modelData
                            readonly property bool on: root.mood === modelData.m
                            width: pillText.implicitWidth + 24 * root.u
                            height: 30 * root.u
                            radius: height / 2
                            color: on ? root.pal.text : "transparent"
                            border.width: 1.2 * root.u
                            border.color: Qt.alpha(root.pal.text, on ? 1 : 0.3)
                            Behavior on color { ColorAnimation { duration: root.dur } }
                            Text {
                                id: pillText
                                anchors.centerIn: parent
                                text: pill.modelData.g + "  " + pill.modelData.t
                                color: pill.on ? root.pal.background : Qt.alpha(root.pal.text, 0.7)
                                font.pixelSize: 14 * root.u
                                font.letterSpacing: 0.6 * root.u
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.moodPicked(pill.modelData.m)
                            }
                        }
                    }
                }
            }
        }

        // pushed-back cards go dark instead of just transparent: it reads as depth
        Rectangle {
            anchors.fill: parent
            color: "#07070a"
            opacity: root.dim * 0.55
        }
    }

    Rectangle {
        id: mask
        anchors.fill: parent
        radius: root.radius
        visible: false
        layer.enabled: true
    }

    // the kanji on a hanging tag, like a shop's noren or a seal
    Rectangle {
        id: tag
        visible: (root.card.kanji || "").length > 0
        anchors.right: parent.right
        anchors.rightMargin: 40 * root.u
        y: -14 * root.u
        width: 76 * root.u
        height: kanji.implicitHeight + 46 * root.u
        radius: 6 * root.u
        color: root.accent
        Behavior on color { ColorAnimation { duration: root.dur } }
        opacity: 1 - root.dim * 0.35

        Column {
            id: kanji
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 20 * root.u
            spacing: -4 * root.u
            Repeater {
                model: Array.from(root.card.kanji || "")
                Text {
                    required property string modelData
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData
                    color: "#16141a"
                    font.family: "Noto Serif CJK JP"
                    font.weight: Font.Bold
                    font.pixelSize: 50 * root.u
                }
            }
        }
    }

    // badges on the screenshot
    Row {
        x: 26 * root.u
        y: 24 * root.u
        spacing: 8 * root.u

        Rectangle {
            visible: root.current
            width: wearing.implicitWidth + 26 * root.u
            height: 30 * root.u
            radius: height / 2
            color: Qt.rgba(0.04, 0.04, 0.06, 0.62)
            Text {
                id: wearing
                anchors.centerIn: parent
                text: "着用中 · wearing now"
                color: "#f3efe4"
                font.pixelSize: 13 * root.u
                font.letterSpacing: 0.8 * root.u
            }
        }
        Rectangle {
            visible: (root.card.problems || []).length > 0
            width: needs.implicitWidth + 26 * root.u
            height: 30 * root.u
            radius: height / 2
            color: Qt.rgba(0.85, 0.55, 0.15, 0.92)
            Text {
                id: needs
                anchors.centerIn: parent
                text: "needs kigae install"
                color: "#1a1206"
                font.pixelSize: 13 * root.u
                font.weight: Font.DemiBold
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        anchors.bottomMargin: root.card.daynight && root.selected ? root.bandH : 0
        cursorShape: Qt.PointingHandCursor
        onClicked: root.picked()
    }
}
