import QtQuick
import Quickshell
import Quickshell.Io
import qs.Ui

// Bar badge for the nwg-notifications daemon. A second consumer of the
// same status-file contract the waybar module reads: the daemon writes
// nwg-notifications-status.json under XDG_RUNTIME_DIR on every state
// change (the SIGRTMIN+11 waybar ping keeps firing but is not needed
// here — FileView watches the file directly). The daemon composes the
// glyph/tooltip for every state (empty / unread / DND), so the widget
// displays rather than recomputes.
BarWidget {
  id: root
  moduleName: "nwg.notifications"

  // XDG_RUNTIME_DIR is guaranteed on Omarchy (systemd session) — the
  // only platform the shell runs on — so the daemon's cache-dir/tmp
  // fallback chain (src/paths.rs) is deliberately not mirrored here:
  // an unreachable branch in QML would only be drift risk.
  readonly property string runtimeDir: Quickshell.env("XDG_RUNTIME_DIR") || ""
  readonly property string statusPath: runtimeDir ? runtimeDir + "/nwg-notifications-status.json" : ""
  property var status: null

  FileView {
    path: root.statusPath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.parse(text())
    onLoadFailed: root.status = null
  }

  function parse(content) {
    try {
      var parsed = JSON.parse(String(content || ""))
      root.status = parsed && typeof parsed === "object" ? parsed : null
    } catch (e) {
      root.status = null
    }
  }

  // The daemon listens on realtime signals (panel: RTMIN+4, DND toggle:
  // RTMIN+5, DND menu: RTMIN+6) — same contract as the waybar module's
  // click bindings. bar.run executes via `bash -lc`, so the RTMIN name
  // (portable across glibc/musl offsets) and $(pidof) both resolve
  // there; pidof matches the exact binary rather than pkill -f's
  // any-cmdline-substring.
  function signalDaemon(offset) {
    if (root.bar) root.bar.run("kill -s RTMIN+" + offset + " $(pidof nwg-notifications)")
  }

  // Hidden until the daemon has written a status file: no runtime dir,
  // no file, or unparseable content all mean there is nothing truthful
  // to show.
  visible: statusPath !== "" && status !== null
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // WidgetButton's own MouseArea already accepts Left/Right/Middle
  // (Ui/WidgetButton.qml) and reports which one fired through
  // `signal pressed(int button)`, so a single button instance covers
  // all three clicks — no MouseArea overlay needed.
  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.status ? String(root.status.text || "") : ""
    tooltipText: root.status ? String(root.status.tooltip || "") : ""
    // Urgent (active) coloring while Do-Not-Disturb is on, driven by
    // the status file's class field — same signal waybar styles on.
    active: root.status !== null && root.status.class === "dnd"
    horizontalMargin: 6
    onPressed: function(mouseButton) {
      if (mouseButton === Qt.LeftButton) root.signalDaemon(4)
      else if (mouseButton === Qt.RightButton) root.signalDaemon(6)
      else if (mouseButton === Qt.MiddleButton) root.signalDaemon(5)
    }
  }
}
