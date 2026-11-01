// Kigae helpers for plasmashell scripts (SPEC.md §5). Every Kigae layout call starts with this.
//
// Two things don't work inside the script that builds the panels: Plasma drops tray settings
// written while the tray is being created, and a desktop widget can't take the spot of one the
// prelude is still removing. Rice scripts declare them with kigae.tray() and kigae.desktop();
// Kigae applies them in a second call once the panels exist and the old widgets are gone.
var kigae = {
    launchers: "",
    state: { trays: [], desk: [] },

    // groups: {"General": {"hiddenItems": [...]}}; applies to the nth tray the script creates
    tray: function (w, groups) {
        this.state.trays.push(groups);
    },

    // a desktop widget at x,y with this size; config: {"General": {"key": value}}
    desktop: function (type, x, y, width, height, config) {
        this.state.desk.push({ type: type, rect: [x, y, width, height], config: config || {} });
    },

    done: function () {
        if (this.state.trays.length || this.state.desk.length)
            print("KIGAE-STATE " + JSON.stringify(this.state));
    },

    trays: function () {
        var t = [];
        panels().forEach(function (p) {
            p.widgets().forEach(function (w) {
                if (w.type == "org.kde.plasma.systemtray") t.push(w);
            });
        });
        return t;
    },

    // applet id → [x, y, w, h] from the folder view's ItemGeometries
    geometries: function (d) {
        d.currentConfigGroup = [];
        var g = {};
        String(d.readConfig("ItemGeometriesHorizontal", "")).split(";").forEach(function (e) {
            var m = e.split(":");
            if (m.length == 2) g[m[0].replace("Applet-", "")] = m[1].split(",").slice(0, 4).map(Number);
        });
        return g;
    },

    // what the layout dump leaves out: tray settings and desktop widget positions
    capture: function () {
        var st = { trays: [], desk: [] };
        this.trays().forEach(function (w) {
            w.currentConfigGroup = ["General"];
            var cfg = {};
            w.configKeys.forEach(function (k) { cfg[k] = w.readConfig(k); });
            st.trays.push({ General: cfg });
        });
        var d = desktops()[0];
        var g = this.geometries(d);
        d.widgets().forEach(function (w) {
            var config = {};
            [""].concat(w.configGroups).forEach(function (grp) {
                w.currentConfigGroup = grp ? grp.split("/") : [];
                var keys = {};
                w.configKeys.forEach(function (k) { keys[k] = w.readConfig(k); });
                if (w.configKeys.length) config[grp] = keys;
            });
            st.desk.push({ type: w.type, rect: g[w.id] || [0, 0, 0, 0], config: config });
        });
        print("KIGAE-STATE " + JSON.stringify(st));
    },

    restore: function (st) {
        var trays = this.trays();
        st.trays.forEach(function (groups, i) {
            var w = trays[i];
            if (!w) return;
            for (var grp in groups) {
                w.currentConfigGroup = grp.split("/");
                for (var k in groups[grp]) w.writeConfig(k, groups[grp][k]);
            }
            w.reloadConfig();
        });
        // the folder view only honours a position when the spot is free, so desktop widgets
        // are added here, after the prelude's removals have finished
        var d = desktops()[0];
        st.desk.forEach(function (e) {
            var r = e.rect;
            var w = r[2] ? d.addWidget(e.type, r[0], r[1], r[2], r[3]) : d.addWidget(e.type);
            for (var grp in e.config) {
                w.currentConfigGroup = grp ? grp.split("/") : [];
                for (var k in e.config[grp]) w.writeConfig(k, e.config[grp][k]);
            }
        });
    },

    // remember the dock launchers, then remove every panel and desktop widget
    clear: function () {
        var self = this;
        panels().forEach(function (p) {
            p.widgets().forEach(function (w) {
                if (!self.launchers && (w.type == "org.kde.plasma.icontasks" || w.type == "org.kde.plasma.taskmanager")) {
                    w.currentConfigGroup = ["General"];
                    self.launchers = w.readConfig("launchers", "");
                }
            });
        });
        panels().forEach(function (p) { p.remove(); });
        desktops().forEach(function (d) {
            d.widgets().forEach(function (w) { w.remove(); });
        });
    }
};
