// Kigae prelude: runs before every rice layout (SPEC.md §5).
// Remembers the dock launchers in kigae.launchers, then clears every panel and desktop widget,
// so the rice's script always starts from nothing.
var kigae = { launchers: "" };
panels().forEach(function (p) {
    p.widgets().forEach(function (w) {
        if (!kigae.launchers && (w.type == "org.kde.plasma.icontasks" || w.type == "org.kde.plasma.taskmanager")) {
            w.currentConfigGroup = ["General"];
            kigae.launchers = w.readConfig("launchers", "");
        }
    });
});
panels().forEach(function (p) { p.remove(); });
desktops().forEach(function (d) {
    d.widgets().forEach(function (w) { w.remove(); });
});
