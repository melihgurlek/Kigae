// og panel for Kigae: Plasma's stock bottom panel, as backup/og had it.
// Runs after Kigae's prelude (no panels, kigae.launchers set).
var panel = new Panel;
panel.location = "bottom";
panel.height = 40;
panel.floating = true;

var kickoff = panel.addWidget("org.kde.plasma.kickoff");
kickoff.currentConfigGroup = ["General"];
kickoff.writeConfig("icon", "start-here-kde-plasma-symbolic");

panel.addWidget("org.kde.plasma.pager");

var tasks = panel.addWidget("org.kde.plasma.icontasks");
tasks.currentConfigGroup = ["General"];
tasks.writeConfig("launchers", kigae.launchers);

panel.addWidget("org.kde.plasma.marginsseparator");

var tray = panel.addWidget("org.kde.plasma.systemtray");
tray.currentConfigGroup = ["General"];
tray.writeConfig("hiddenItems", ["Xwayland Video Bridge", "chrome_status_icon_1@brave", "org.kde.plasma.brightness"]);

var clock = panel.addWidget("org.kde.plasma.digitalclock");
clock.currentConfigGroup = ["Appearance"];
clock.writeConfig("use24hFormat", 2);

panel.addWidget("org.kde.plasma.showdesktop");
