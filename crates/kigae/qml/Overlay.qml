// The fitting room (PLAN.md phase 2): rices hang on a rail, the room takes on the one you
// look at, and the one you pick grows to fill the screen while Kigae switches behind it.
import QtQuick
import QtQuick.Window
import QtQuick.Effects
import org.kde.layershell as LayerShell
import dev.kigae

Window {
    id: win
    visible: false
    color: "transparent"
    title: "Kigae"

    LayerShell.Window.scope: "kigae"
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityExclusive
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop | LayerShell.Window.AnchorBottom
        | LayerShell.Window.AnchorLeft | LayerShell.Window.AnchorRight
    LayerShell.Window.exclusionZone: -1

    Kigae {
        id: kigae
        Component.onCompleted: attach()
        onShowRequested: win.request()
        onFinished: (ok, message) => win.done(ok, message)
    }

    // ── state ────────────────────────────────────────────────────────────────────
    readonly property var st: { try { return JSON.parse(kigae.state || "{}") } catch (e) { return {} } }
    readonly property var cards: st.cards || []
    readonly property int n: cards.length
    readonly property real anim: st.anim === undefined ? 1 : st.anim
    function dur(ms) { return Math.round(ms * anim) }

    property int sel: 0
    property real pos: 0                // sel, animated; cards place themselves by i - pos
    Behavior on pos { enabled: !win.jump; NumberAnimation { duration: win.dur(340); easing.type: Easing.OutCubic } }
    property bool jump: false
    property var moods: ({})            // id → "light" | "dark", picked on the card
    property bool drawer: false
    property bool stepping: false       // the chosen card is filling the screen
    property bool holding: false        // opened with Meta held: letting go wears
    property bool metaSeen: false
    property double shownAt: 0
    property string error: ""

    readonly property var selCard: n ? cards[Math.max(0, Math.min(sel, n - 1))] : null
    function moodOf(c) { return c && c.daynight ? (moods[c.id] || st.mood || "dark") : "light" }
    function lookOf(c) { return c ? (moodOf(c) === "light" ? c.light : c.dark) : null }
    readonly property var selLook: lookOf(selCard)

    // the room takes the rice's most vivid colour: its accent is often a muted selection grey
    function chroma(h) {
        const c = Qt.color(h)
        return Math.max(c.r, c.g, c.b) - Math.min(c.r, c.g, c.b)
    }
    function vivid(look) {
        const p = look && look.palette
        if (!p) return "#c8a96b"
        let best = p.accent
        for (const s of [p.accent].concat(p.swatches))
            if (chroma(s) > chroma(best) + 0.08) best = s
        return best
    }
    property color accent: vivid(selLook)
    Behavior on accent { ColorAnimation { duration: win.dur(300) } }
    readonly property color chrome: "#f2eee4"

    function fileUrl(p) { return p ? "file://" + encodeURI(p) : "" }

    // ── open, move, wear, close ──────────────────────────────────────────────────
    function request() {
        if (!visible) { open(); return }
        if (stepping) return
        if (Date.now() - shownAt < 120) return        // the same press, twice
        shownAt = Date.now()
        step(1)                                       // T tapped again while Meta+Shift held
    }

    function open() {
        kigae.reload()
        const cur = cards.findIndex(c => c.id === st.current)
        jump = true
        sel = n ? (cur + 1) % n : 0
        pos = sel
        jump = false
        moods = ({})
        drawer = false
        stepping = false
        error = ""
        metaSeen = false
        holding = true                                // until we learn otherwise
        shownAt = Date.now()
        room.opacity = 0
        rail.opacity = 0
        rail.lift = 40
        veil.visible = false
        visible = true
        keys.forceActiveFocus()
        fadeIn.restart()
    }

    function step(d) {
        if (!n || stepping) return
        const next = (sel + d + n) % n
        if (n >= 4) pos += d                          // go round
        sel = next
        if (n < 4) pos = sel                          // few rices: slide back across
    }

    function setMood(c, m) {
        const o = Object.assign({}, moods)
        o[c.id] = m
        moods = o
    }

    function wear() {
        const c = selCard
        if (!c || stepping || kigae.busy) return
        if ((c.problems || []).length) {
            error = c.name + " isn't installed yet: " + c.problems[0] + "\nRun  kigae install " + c.id
            return
        }
        const mood = c.daynight ? moodOf(c) : ""
        if (c.id === st.current && (!c.daynight || mood === st.mood)) { close(); return }
        error = ""
        stepping = true
        const r = rail.itemAt(sel)
        veil.from = r ? r.mapToItem(null, 0, 0, r.width, r.height * r.scale / r.scale) : Qt.rect(0, 0, width, height)
        veil.start(lookOf(c))
        kigae.apply(c.id, mood)
    }

    function done(ok, message) {
        if (!stepping) {                              // undo from the drawer
            error = ok ? "" : message
            if (ok) close()
            return
        }
        if (ok) {
            veil.reveal()
        } else {
            stepping = false
            veil.back()
            error = message
        }
    }

    function close() {
        fadeOut.restart()
    }

    SequentialAnimation {
        id: fadeIn
        ParallelAnimation {
            NumberAnimation { target: room; property: "opacity"; to: 1; duration: win.dur(240); easing.type: Easing.OutQuad }
            SequentialAnimation {
                PauseAnimation { duration: win.dur(60) }
                ParallelAnimation {
                    NumberAnimation { target: rail; property: "opacity"; to: 1; duration: win.dur(300); easing.type: Easing.OutCubic }
                    NumberAnimation { target: rail; property: "lift"; to: 0; duration: win.dur(340); easing.type: Easing.OutCubic }
                }
            }
        }
    }
    SequentialAnimation {
        id: fadeOut
        ParallelAnimation {
            NumberAnimation { target: room; property: "opacity"; to: 0; duration: win.dur(200); easing.type: Easing.InQuad }
            NumberAnimation { target: rail; property: "opacity"; to: 0; duration: win.dur(160) }
            NumberAnimation { target: veil; property: "opacity"; to: 0; duration: win.dur(200) }
        }
        ScriptAction { script: { win.visible = false; win.stepping = false; veil.visible = false } }
    }

    // ── the room: the selected rice's wallpaper, blurred and dimmed ─────────────
    Item {
        id: room
        anchors.fill: parent
        opacity: 0

        Rectangle { anchors.fill: parent; color: "#0b0b0f" }

        Item {
            id: walls
            anchors.fill: parent
            layer.enabled: true
            layer.effect: MultiEffect {
                blurEnabled: true
                blur: 1.0
                blurMax: 64
                saturation: -0.1
                brightness: -0.08
            }

            // two layers so a new wallpaper fades in over the old one once it has loaded
            property bool front: true
            property string want: win.selLook && win.selLook.wallpaper ? win.selLook.wallpaper
                : (win.selCard && win.selCard.preview ? win.selCard.preview : "")
            onWantChanged: {
                const back = front ? wallB : wallA
                if (back.path === want) { front = !front; return }
                back.path = want
            }
            function ready(img) {
                if (img.path === want && img !== (front ? wallA : wallB)) front = !front
            }

            Wall { id: wallA; shown: walls.front }
            Wall { id: wallB; shown: !walls.front }
        }

        // dim, heavier at the bottom where the hints sit
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.0; color: Qt.rgba(0.02, 0.02, 0.03, 0.42) }
                GradientStop { position: 0.6; color: Qt.rgba(0.02, 0.02, 0.03, 0.50) }
                GradientStop { position: 1.0; color: Qt.rgba(0.02, 0.02, 0.03, 0.78) }
            }
        }

        MouseArea {                                   // a click on the room closes it
            anchors.fill: parent
            onClicked: if (!win.stepping) win.close()
            onWheel: wheel => {
                acc += wheel.angleDelta.x !== 0 ? -wheel.angleDelta.x : -wheel.angleDelta.y
                while (acc >= 120) { win.step(1); acc -= 120 }
                while (acc <= -120) { win.step(-1); acc += 120 }
            }
            property real acc: 0
        }
    }

    component Wall: Image {
        property string path: ""
        property bool shown: false
        anchors.fill: parent
        source: win.fileUrl(path)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        cache: true
        sourceSize.width: 1600
        opacity: shown && status === Image.Ready ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: win.dur(420); easing.type: Easing.InOutQuad } }
        onStatusChanged: if (status === Image.Ready) walls.ready(this)
    }

    // ── header ───────────────────────────────────────────────────────────────────
    Row {
        x: 56
        y: 44
        spacing: 18
        opacity: rail.opacity * (win.stepping ? 0 : 1)
        Behavior on opacity { NumberAnimation { duration: win.dur(200) } }
        Text {
            text: "着替え"
            color: win.chrome
            font.family: "Noto Serif CJK JP"
            font.pixelSize: 34
            font.weight: Font.DemiBold
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Text { text: "KIGAE"; color: win.chrome; font.pixelSize: 12; font.letterSpacing: 4; font.weight: Font.DemiBold }
            Text { text: "fitting room"; color: Qt.alpha(win.chrome, 0.55); font.pixelSize: 12; font.letterSpacing: 1.5 }
        }
    }
    Text {
        anchors.right: parent.right
        anchors.rightMargin: 56
        y: 52
        visible: win.n > 1
        opacity: rail.opacity * (win.stepping ? 0 : 1)
        text: (win.sel + 1) + " / " + win.n
        color: Qt.alpha(win.chrome, 0.6)
        font.pixelSize: 15
        font.letterSpacing: 3
        font.features: { "tnum": 1 }
    }

    // ── the rail ─────────────────────────────────────────────────────────────────
    Item {
        id: rail
        anchors.fill: parent
        property real lift: 0
        opacity: 0
        readonly property real cw: Math.min(win.width * 0.44, (win.height * 0.62) / (9 / 16 + 132 / 900))
        readonly property real cy: win.height * 0.47 + lift

        function itemAt(i) { return cardsRep.itemAt(i) }

        Repeater {
            id: cardsRep
            model: win.cards
            RiceCard {
                id: rc
                required property var modelData
                required property int index
                card: modelData
                look: win.lookOf(modelData)
                mood: win.moodOf(modelData)
                current: modelData.id === win.st.current
                selected: index === win.sel
                accent: win.vivid(look)
                dur: win.dur(260)

                // signed distance from the middle, wrapped when there are enough to go round
                readonly property real d: {
                    let x = index - win.pos
                    if (win.n >= 4) x = ((x % win.n) + win.n * 1.5) % win.n - win.n / 2
                    return x
                }
                readonly property real ad: Math.abs(d)
                readonly property real spread: rail.cw * (0.70 * Math.min(ad, 1) + 0.46 * Math.max(ad - 1, 0))

                width: rail.cw
                height: implicitHeight
                x: win.width / 2 - width / 2 + Math.sign(d) * spread
                y: rail.cy - height / 2
                scale: 1 / (1 + 0.42 * ad)
                z: 100 - ad * 10
                dim: Math.min(ad, 1)
                opacity: ad > 2.4 ? 0 : (win.stepping && !selected ? 0 : 1)
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: win.dur(220) } }
                transform: Rotation {
                    origin.x: rc.width / 2
                    origin.y: rc.height / 2
                    axis { x: 0; y: 1; z: 0 }
                    angle: Math.max(-34, Math.min(34, -rc.d * 30))
                }

                onPicked: index === win.sel ? win.wear() : win.step(index - win.sel)
                onMoodPicked: m => win.setMood(modelData, m)
            }
        }
    }

    // ── hints and errors ─────────────────────────────────────────────────────────
    Row {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 40
        spacing: 30
        opacity: rail.opacity * (win.stepping ? 0 : 0.85)
        Repeater {
            model: [
                { k: "← →", t: "choose" },
                { k: "↑ ↓", t: "day · night", dn: true },
                { k: "⏎", t: "wear" },
                { k: "⇥", t: "drawer" },
                { k: "esc", t: "leave" },
            ]
            Row {
                required property var modelData
                visible: !modelData.dn || (win.selCard && win.selCard.daynight)
                spacing: 9
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.max(28, key.implicitWidth + 14)
                    height: 24
                    radius: 6
                    color: Qt.alpha(win.chrome, 0.10)
                    border.width: 1
                    border.color: Qt.alpha(win.chrome, 0.22)
                    Text { id: key; anchors.centerIn: parent; text: parent.parent.modelData.k; color: win.chrome; font.pixelSize: 13 }
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: parent.modelData.t
                    color: Qt.alpha(win.chrome, 0.7)
                    font.pixelSize: 14
                    font.letterSpacing: 0.5
                }
            }
        }
    }

    Rectangle {
        id: toast
        visible: win.error.length > 0
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 92
        width: Math.min(win.width * 0.6, toastText.implicitWidth + 48 + undoBtn.width)
        height: toastText.implicitHeight + 28
        radius: 14
        color: Qt.rgba(0.10, 0.06, 0.06, 0.92)
        border.width: 1
        border.color: Qt.rgba(0.9, 0.4, 0.35, 0.6)
        z: 300
        Text {
            id: toastText
            anchors.left: parent.left
            anchors.leftMargin: 20
            anchors.right: undoBtn.left
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            text: win.error
            color: "#f6dcd6"
            font.pixelSize: 14
            wrapMode: Text.Wrap
        }
        Rectangle {
            id: undoBtn
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            width: undoText.implicitWidth + 24
            height: 30
            radius: 8
            color: Qt.rgba(1, 1, 1, 0.1)
            Text { id: undoText; anchors.centerIn: parent; text: "Undo last switch"; color: "#f6dcd6"; font.pixelSize: 13 }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: kigae.undo() }
        }
    }

    // ── stepping in: the chosen rice grows to fill the screen, then dissolves ────
    Item {
        id: veil
        visible: false
        z: 500
        property rect from: Qt.rect(0, 0, 0, 0)
        property real t: 0                            // 0 = card, 1 = full screen
        x: from.x * (1 - t)
        y: from.y * (1 - t)
        width: from.width + (win.width - from.width) * t
        height: from.height + (win.height - from.height) * t
        layer.enabled: visible && t < 1
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: veilMask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1.0
        }

        function start(look) {
            veilImg.path = look && look.wallpaper ? look.wallpaper : (win.selCard.preview || "")
            t = 0
            opacity = 1
            visible = true
            grow.restart()
        }
        function reveal() { grow.stop(); dissolve.restart() }
        function back() { grow.stop(); shrink.restart() }

        Rectangle { anchors.fill: parent; color: "#0b0b0f" }
        Image {
            id: veilImg
            property string path: ""
            anchors.fill: parent
            source: win.fileUrl(path)
            fillMode: Image.PreserveAspectCrop
            sourceSize.width: 2560
        }
        Rectangle {                                   // a quiet "dressing" mark while it works
            anchors.centerIn: parent
            visible: veil.t > 0.95
            width: 64; height: 64; radius: 32
            color: "transparent"
            border.width: 2
            border.color: Qt.alpha("#ffffff", 0.35)
            opacity: kigae.busy ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 300 } }
            RotationAnimator on rotation { from: 0; to: 360; duration: 1600; loops: Animation.Infinite; running: kigae.busy && veil.visible }
            Rectangle { width: 10; height: 10; radius: 5; color: win.accent; x: parent.width / 2 - 5; y: -5 }
        }

        NumberAnimation { id: grow; target: veil; property: "t"; to: 1; duration: win.dur(420); easing.type: Easing.InOutCubic }
        NumberAnimation { id: shrink; target: veil; property: "t"; to: 0; duration: win.dur(300); easing.type: Easing.InOutCubic
            onFinished: veil.visible = false }
        SequentialAnimation {
            id: dissolve
            NumberAnimation { target: veil; property: "t"; to: 1; duration: win.dur(120) }
            PauseAnimation { duration: 400 }          // let plasmashell settle under the veil
            ScriptAction { script: { room.opacity = 0; rail.opacity = 0 } }
            NumberAnimation { target: veil; property: "opacity"; to: 0; duration: win.dur(450); easing.type: Easing.InOutQuad }
            ScriptAction { script: { win.visible = false; win.stepping = false; veil.visible = false } }
        }
    }
    Rectangle {
        id: veilMask
        width: veil.width
        height: veil.height
        radius: 22 * (rail.cw / 900) * (1 - veil.t)
        visible: false
        layer.enabled: true
    }

    // ── drawer: history with undo, and each rice's health ────────────────────────
    Rectangle {
        id: drawerPanel
        width: 420
        height: parent.height
        x: win.drawer ? parent.width - width : parent.width
        Behavior on x { NumberAnimation { duration: win.dur(280); easing.type: Easing.OutCubic } }
        z: 400
        color: Qt.rgba(0.05, 0.05, 0.07, 0.9)
        Rectangle { width: 2; height: parent.height; color: win.accent }
        MouseArea { anchors.fill: parent }            // keep clicks off the room

        Column {
            x: 40
            y: 52
            width: parent.width - 80
            spacing: 14

            Text { text: "記録  history"; color: win.chrome; font.pixelSize: 20; font.family: "Noto Serif CJK JP" }
            Repeater {
                model: win.st.history || []
                Column {
                    required property var modelData
                    required property int index
                    spacing: 2
                    opacity: modelData.status === "Done" ? 1 : 0.45
                    Text {
                        text: (modelData.from || "?") + "  →  " + modelData.to + (modelData.mood ? (modelData.mood === "light" ? "  ☀" : "  ☾") : "")
                        color: win.chrome
                        font.pixelSize: 15
                    }
                    Text {
                        text: new Date(modelData.ts * 1000).toLocaleString(Qt.locale(), "ddd d MMM, HH:mm")
                            + (modelData.status === "Done" ? "" : "  · " + modelData.status.toLowerCase())
                        color: Qt.alpha(win.chrome, 0.5)
                        font.pixelSize: 12
                    }
                }
            }
            Rectangle {
                visible: (win.st.history || []).length > 0
                width: undoLbl.implicitWidth + 28
                height: 34
                radius: 9
                color: Qt.alpha(win.accent, 0.18)
                border.width: 1
                border.color: Qt.alpha(win.accent, 0.6)
                Text { id: undoLbl; anchors.centerIn: parent; text: kigae.busy ? "undoing…" : "Undo last switch"; color: win.chrome; font.pixelSize: 14 }
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: kigae.undo() }
            }

            Item { width: 1; height: 18 }
            Text { text: "点検  health"; color: win.chrome; font.pixelSize: 20; font.family: "Noto Serif CJK JP" }
            Repeater {
                model: win.cards
                Column {
                    required property var modelData
                    spacing: 2
                    width: parent.width
                    Text {
                        text: ((modelData.problems || []).length ? "●  " : "✓  ") + modelData.name
                        color: (modelData.problems || []).length ? "#e8a24a" : win.chrome
                        font.pixelSize: 15
                    }
                    Repeater {
                        model: modelData.problems || []
                        Text {
                            required property string modelData
                            width: parent.width
                            text: "    " + modelData
                            wrapMode: Text.Wrap
                            color: Qt.alpha(win.chrome, 0.55)
                            font.pixelSize: 12
                        }
                    }
                }
            }

            Item { width: 1; height: 18 }
            Text { text: "写す  capture"; color: win.chrome; font.pixelSize: 20; font.family: "Noto Serif CJK JP" }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                text: "Made something new on this desktop? Keep it as a rice:"
                color: Qt.alpha(win.chrome, 0.6)
                font.pixelSize: 13
            }
            Text { text: "kigae capture <name>"; color: win.chrome; font.family: "monospace"; font.pixelSize: 13 }
        }
    }

    // ── keys ─────────────────────────────────────────────────────────────────────
    Item {
        id: keys
        focus: true
        Keys.onPressed: e => {
            const meta = e.key === Qt.Key_Meta || e.key === Qt.Key_Super_L || e.key === Qt.Key_Super_R
            if (meta) { win.metaSeen = true; return }
            if (e.key !== Qt.Key_Shift && e.key !== Qt.Key_T) win.holding = false
            switch (e.key) {
            case Qt.Key_Escape:
                if (win.drawer) win.drawer = false
                else if (!win.stepping) win.close()
                break
            case Qt.Key_Right: case Qt.Key_L:
                win.step(1); break
            case Qt.Key_Left: case Qt.Key_H:
                win.step(-1); break
            case Qt.Key_T:
                win.step(e.modifiers & Qt.ShiftModifier && !(e.modifiers & Qt.MetaModifier) ? -1 : 1); break
            case Qt.Key_Backtab:
                win.step(-1); break
            case Qt.Key_Up: case Qt.Key_K:
                if (win.selCard && win.selCard.daynight) win.setMood(win.selCard, "light"); break
            case Qt.Key_Down: case Qt.Key_J:
                if (win.selCard && win.selCard.daynight) win.setMood(win.selCard, "dark"); break
            case Qt.Key_Tab: case Qt.Key_Menu:
                win.drawer = !win.drawer; break
            case Qt.Key_Return: case Qt.Key_Enter: case Qt.Key_Space:
                win.wear(); break
            default:
                return
            }
            e.accepted = true
        }
        Keys.onReleased: e => {
            const meta = e.key === Qt.Key_Meta || e.key === Qt.Key_Super_L || e.key === Qt.Key_Super_R
            // Meta was held when we opened and is let go now: wear what's selected (Alt-Tab style).
            // If Meta was already up when we got focus, no release comes and we stay open.
            if (meta && win.holding && !win.metaSeen && !e.isAutoRepeat) win.wear()
        }
    }
}
