//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
//@ pragma Env QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000
//@ pragma Env QT_SCALE_FACTOR=1

// Arctix graphical installer. Live ISO only (cleanup removes this file from
// installed systems). All real work is done by /usr/local/bin/arctix-install:
//   `options`  -> disks, defaults and choices (JSON)
//   `validate` -> the same checks the install runs (JSON)
//   `install --progress json` -> one JSON object per step
// Launch: qs -p ~/.config/quickshell/ii/installer.qml

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import Quickshell
import Quickshell.Io
import qs.services
import qs.modules.common
import qs.modules.common.widgets

ApplicationWindow {
    id: root
    visible: true
    title: "Install Arctix"
    minimumWidth: 640
    minimumHeight: 520
    width: 900
    height: 650
    color: Appearance.m3colors.m3background

    readonly property string installer: "/usr/local/bin/arctix-install"
    readonly property string configPath: "/tmp/arctix-install.toml"
    readonly property var pageTitles: ["Welcome", "Disk", "Account", "Region", "Summary", "Installing"]

    property var opts: null
    property int page: 0

    // Answers
    property string disk: ""
    property string username: ""
    property string password: ""
    property string password2: ""
    property string hostname: "arctix"
    property string timezone: "UTC"
    property string keymap: "us"
    property string language: "en_US.UTF-8"
    property bool update: true
    property var pageErrors: []

    // Install progress
    property string phase: "idle" // idle | running | done | failed
    property int stepN: 0
    property int stepTotal: 1
    property string stepDesc: ""
    property string endText: ""
    property var notes: []
    property string stderrText: ""

    Component.onCompleted: {
        MaterialThemeLoader.reapplyTheme();
        optionsProc.running = true;
    }

    onClosing: close => {
        // Never close in the middle of an install.
        if (root.phase === "running")
            close.accepted = false;
        else
            Qt.quit();
    }

    // ------------------------------------------------------------ backend

    // JSON strings are valid TOML basic strings, so this escapes safely.
    function q(s) {
        return JSON.stringify(s);
    }

    function configText() {
        return "# Written by the Arctix graphical installer.\n\n"
            + "[disk]\ndevice = " + q(root.disk) + "\nfilesystem = \"ext4\"\n\n"
            + "[install]\nmode = \"copy\"\nupdate = " + (root.update ? "true" : "false") + "\n\n"
            + "[system]\nhostname = " + q(root.hostname) + "\ntimezone = " + q(root.timezone)
            + "\nlocale = " + q(root.language) + "\nkeymap = " + q(root.keymap) + "\n\n"
            + "[user]\nname = " + q(root.username) + "\nshell = \"/usr/bin/fish\"\nsudo = true\n";
    }

    function usableDisks() {
        return root.opts ? root.opts.disks.filter(d => d.usable) : [];
    }

    function selectedDisk() {
        return root.opts ? root.opts.disks.find(d => d.path === root.disk) : null;
    }

    function goTo(n) {
        root.pageErrors = [];
        root.page = n;
        focusTimer.restart();
    }

    function next() {
        if (root.page === 1 && root.disk === "") {
            root.pageErrors = ["Pick a disk to install to."];
            return;
        }
        if (root.page === 2) {
            const local = [];
            if (root.username === "")
                local.push("Choose a username.");
            if (root.password === "")
                local.push("Choose a password.");
            else if (root.password !== root.password2)
                local.push("The two passwords are not the same.");
            // Still ask the validator, so every problem shows at once.
            validateProc.localErrors = local;
        } else {
            validateProc.localErrors = [];
        }
        if (root.page === 2 || root.page === 3) {
            validateProc.forPage = root.page;
            validateProc.stdinEnabled = true;
            validateProc.running = true;
            return;
        }
        root.goTo(root.page + 1);
    }

    // Which validation messages belong on which page.
    function errorsForPage(errors, p) {
        const words = p === 2 ? ["user.", "hostname"] : ["timezone", "locale", "keymap"];
        return errors.filter(e => words.some(w => e.indexOf(w) !== -1));
    }

    function startInstall() {
        // Only once, only from the summary, only when confirmed. A key press
        // must never be able to start a second install from a hidden page.
        if (root.phase !== "idle" || root.page !== 4 || confirmField.text.trim() !== root.disk || root.disk === "")
            return;
        confirmField.text = "";
        root.phase = "running";
        root.stepN = 0;
        root.notes = [];
        root.stderrText = "";
        root.goTo(5);
        writeConfigProc.stdinEnabled = true;
        writeConfigProc.running = true;
    }

    function addNote(text) {
        root.notes = root.notes.concat([text]).slice(-300);
    }

    Timer {
        id: focusTimer
        interval: 50
        onTriggered: {
            const item = pages.children[root.page];
            if (item && item.focusFirst)
                item.focusFirst();
        }
    }

    Process {
        id: optionsProc
        command: [root.installer, "options"]
        stdout: StdioCollector {
            id: optionsOut
            onStreamFinished: {
                try {
                    root.opts = JSON.parse(optionsOut.text);
                } catch (e) {
                    root.opts = {live: false, disks: [], defaults: {}, timezones: [], keymaps: [], locales: [], error: "Could not read options: " + e};
                    return;
                }
                const d = root.opts.defaults;
                root.hostname = d.hostname;
                root.timezone = d.timezone;
                root.keymap = d.keymap;
                root.language = d.locale;
                const usable = root.usableDisks();
                if (usable.length === 1)
                    root.disk = usable[0].path;
                focusTimer.restart();
            }
        }
    }

    Process {
        id: validateProc
        property int forPage: 0
        property var localErrors: []
        command: [root.installer, "validate"]
        onRunningChanged: {
            if (running) {
                write(root.configText());
                stdinEnabled = false;
            }
        }
        stdout: StdioCollector {
            id: validateOut
            onStreamFinished: {
                let result;
                try {
                    result = JSON.parse(validateOut.text);
                } catch (e) {
                    root.pageErrors = ["Could not check the answers: " + e];
                    return;
                }
                let mine = root.errorsForPage(result.errors, validateProc.forPage);
                // An empty username already has the friendlier local message.
                if (root.username === "")
                    mine = mine.filter(e => e.indexOf("user.name") === -1);
                mine = validateProc.localErrors.concat(mine);
                if (mine.length > 0)
                    root.pageErrors = mine;
                else
                    root.goTo(validateProc.forPage + 1);
            }
        }
    }

    Process {
        id: writeConfigProc
        command: ["sh", "-c", "umask 077; cat > " + root.configPath]
        onRunningChanged: {
            if (running) {
                write(root.configText());
                stdinEnabled = false;
            }
        }
        onExited: (code, status) => {
            if (code === 0) {
                installProc.stdinEnabled = true;
                installProc.running = true;
            } else {
                root.phase = "failed";
                root.endText = "Could not write " + root.configPath;
            }
        }
    }

    Process {
        id: installProc
        // The inhibitor keeps hypridle and logind from locking or suspending
        // the machine in the middle of the install.
        command: ["systemd-inhibit", "--what=idle:sleep", "--who=Arctix installer", "--why=Installing Arctix",
            "sudo", "-n", root.installer, "install", root.configPath, "--yes", "--password-stdin", "--progress", "json"]
        onRunningChanged: {
            if (running) {
                // The password only ever travels on stdin.
                write(root.password + "\n");
                stdinEnabled = false;
            }
        }
        stdout: SplitParser {
            onRead: line => {
                let ev;
                try {
                    ev = JSON.parse(line);
                } catch (e) {
                    root.addNote(line);
                    return;
                }
                if (ev.type === "step") {
                    root.stepN = ev.n;
                    root.stepTotal = ev.total;
                    root.stepDesc = ev.desc;
                    root.addNote(ev.n + "/" + ev.total + "  " + ev.desc);
                } else if (ev.type === "note") {
                    root.addNote("      " + ev.text);
                } else if (ev.type === "done") {
                    root.phase = "done";
                    root.endText = ev.text;
                    // Enter now does the obvious thing.
                    Qt.callLater(() => rebootButton.forceActiveFocus());
                } else if (ev.type === "error") {
                    root.phase = "failed";
                    root.endText = ev.text;
                }
            }
        }
        stderr: SplitParser {
            onRead: line => root.stderrText += line + "\n"
        }
        onExited: (code, status) => {
            root.password = "";
            root.password2 = "";
            if (root.phase === "running") {
                root.phase = code === 0 ? "done" : "failed";
                root.endText = code === 0 ? "Arctix is installed." : (root.stderrText.trim() || ("The installer stopped (exit code " + code + ")."));
            }
        }
    }

    // ------------------------------------------------------------ widgets

    component PageTitle: StyledText {
        Layout.fillWidth: true
        color: Appearance.colors.colOnLayer0
        wrapMode: Text.Wrap
        font {
            family: Appearance.font.family.title
            pixelSize: Appearance.font.pixelSize.title
            variableAxes: Appearance.font.variableAxes.title
        }
    }

    component Body: StyledText {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Appearance.colors.colOnLayer1
        font.pixelSize: Appearance.font.pixelSize.normal
    }

    component FieldLabel: StyledText {
        color: Appearance.colors.colSubtext
        font.pixelSize: Appearance.font.pixelSize.smaller
    }

    component NavButton: RippleButtonWithIcon {
        property bool primary: false
        Layout.preferredHeight: 44
        buttonRadius: Appearance.rounding.full
        colBackground: primary ? Appearance.colors.colSecondaryContainer : Appearance.colors.colLayer2
        colBackgroundHover: primary ? Appearance.colors.colSecondaryContainerHover : Appearance.colors.colLayer1Hover
        Keys.onReturnPressed: clicked()
        Keys.onEnterPressed: clicked()
    }

    // A search box over a long list (timezones, keymaps, languages).
    // Typing filters; Enter or a click picks.
    component Picker: ColumnLayout {
        id: picker
        property string label
        property var model: []
        property string value
        signal picked(string v)
        property var matches: picker.model.filter(m => m.toLowerCase().indexOf(search.text.toLowerCase()) !== -1).slice(0, 300)
        function focusSearch() {
            search.forceActiveFocus();
        }
        spacing: 4
        FieldLabel {
            text: picker.label + ":  " + picker.value
        }
        MaterialTextField {
            id: search
            Layout.fillWidth: true
            placeholderText: "Type to search"
            // Enter with an empty search keeps the current value; otherwise it
            // takes the best match (an exact match wins over the first one).
            function choose() {
                const typed = text.trim().toLowerCase();
                if (typed === "")
                    picker.picked(picker.value);
                else if (picker.matches.length > 0)
                    picker.picked(picker.matches.find(m => m.toLowerCase() === typed) ?? picker.matches[0]);
            }
            Keys.onReturnPressed: choose()
            Keys.onEnterPressed: choose()
        }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: 104
            clip: true
            model: picker.matches
            boundsBehavior: Flickable.StopAtBounds
            // Keep the current value in view when the list is not filtered.
            function showValue() {
                const i = picker.matches.indexOf(picker.value);
                if (i >= 0)
                    positionViewAtIndex(i, ListView.Center);
            }
            onCountChanged: showValue()
            Connections {
                target: picker
                function onValueChanged() { list.showValue(); }
            }
            delegate: RippleButton {
                required property string modelData
                width: list.width
                implicitHeight: 26
                toggled: modelData === picker.value
                buttonRadius: Appearance.rounding.small
                onClicked: picker.picked(modelData)
                contentItem: StyledText {
                    leftPadding: 10
                    verticalAlignment: Text.AlignVCenter
                    text: parent.modelData
                    color: parent.toggled ? Appearance.colors.colOnPrimary : Appearance.colors.colOnLayer1
                }
            }
        }
    }

    // ------------------------------------------------------------ layout

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 8

        // Header: title and where we are
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            MaterialSymbol {
                text: "install_desktop"
                iconSize: 26
                color: Appearance.colors.colPrimary
            }
            StyledText {
                text: "Install Arctix"
                color: Appearance.colors.colOnLayer0
                font.pixelSize: Appearance.font.pixelSize.larger
            }
            Item { Layout.fillWidth: true }
            Repeater {
                model: root.pageTitles
                StyledText {
                    required property string modelData
                    required property int index
                    text: modelData
                    font.pixelSize: Appearance.font.pixelSize.smaller
                    color: index === root.page ? Appearance.colors.colPrimary : Appearance.colors.colSubtext
                    font.weight: index === root.page ? Font.DemiBold : Font.Normal
                    rightPadding: 8
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: Appearance.m3colors.m3surfaceContainerLow
            radius: Appearance.rounding.windowRounding - 8

            StackLayout {
                id: pages
                anchors.fill: parent
                anchors.margins: 28
                currentIndex: root.page

                // 0: welcome
                ColumnLayout {
                    spacing: 14
                    function focusFirst() { welcomeNext.forceActiveFocus(); }
                    PageTitle { text: "Welcome to Arctix" }
                    Body {
                        text: "This puts Arctix on a disk in this computer: the same system you are using right now, "
                            + "set up for you, with your own account. It takes a few minutes."
                    }
                    Body {
                        text: "Nothing on any disk changes until the last step, where you type the disk's name to confirm."
                    }
                    NoticeBox {
                        Layout.fillWidth: true
                        visible: root.opts !== null && !root.opts.live
                        materialIcon: "error"
                        text: root.opts && root.opts.error ? root.opts.error : "This is not the Arctix live system, so installing is not possible here."
                    }
                    StyledText {
                        visible: root.opts === null
                        text: "Looking at this computer..."
                        color: Appearance.colors.colSubtext
                    }
                    Item { Layout.fillHeight: true }
                    RowLayout {
                        Item { Layout.fillWidth: true }
                        NavButton {
                            id: welcomeNext
                            materialIcon: "arrow_forward"
                            mainText: "Start"
                            primary: true
                            enabled: root.opts !== null && root.opts.live
                            onClicked: root.next()
                        }
                    }
                }

                // 1: disk
                ColumnLayout {
                    spacing: 10
                    function focusFirst() {
                        // A preselected disk means Next is the likely key; else the first usable disk.
                        let target = diskNext;
                        if (root.disk === "") {
                            for (let i = 0; i < diskRepeater.count; i++) {
                                if (diskRepeater.itemAt(i).enabled) { target = diskRepeater.itemAt(i); break; }
                            }
                        }
                        target.forceActiveFocus();
                    }
                    PageTitle { text: "Where should Arctix go?" }
                    Body { text: "Everything on the disk you pick will be erased." }
                    Repeater {
                        id: diskRepeater
                        model: root.opts ? root.opts.disks : []
                        StyledRadioButton {
                            required property var modelData
                            Layout.fillWidth: true
                            enabled: modelData.usable
                            checked: modelData.path === root.disk
                            // ii's radio button shows only `description`.
                            description: modelData.path + "    " + modelData.size_human
                                + (modelData.model ? "    " + modelData.model : "")
                                + (modelData.transport ? "    (" + modelData.transport + ")" : "")
                                + (modelData.usable ? "" : "\nNot usable: " + modelData.reason)
                            onClicked: root.disk = modelData.path
                            Keys.onReturnPressed: { root.disk = modelData.path; root.next(); }
                            Keys.onEnterPressed: { root.disk = modelData.path; root.next(); }
                        }
                    }
                    Item { Layout.fillHeight: true }
                    RowLayout {
                        NavButton { materialIcon: "arrow_back"; mainText: "Back"; onClicked: root.goTo(0) }
                        Item { Layout.fillWidth: true }
                        NavButton { id: diskNext; materialIcon: "arrow_forward"; primary: true; mainText: "Next"; onClicked: root.next() }
                    }
                }

                // 2: account
                ColumnLayout {
                    spacing: 6
                    function focusFirst() { userField.forceActiveFocus(); }
                    PageTitle { text: "Your account" }
                    Body { text: "You log in with this. It can use sudo with the same password." }
                    FieldLabel { text: "Username" }
                    MaterialTextField {
                        id: userField
                        Layout.fillWidth: true
                        placeholderText: "lowercase, like cammy"
                        text: root.username
                        onTextChanged: root.username = text
                        KeyNavigation.tab: passField
                        Keys.onReturnPressed: root.next()
                        Keys.onEnterPressed: root.next()
                    }
                    FieldLabel { text: "Password" }
                    MaterialTextField {
                        id: passField
                        Layout.fillWidth: true
                        echoMode: TextInput.Password
                        text: root.password
                        onTextChanged: root.password = text
                        KeyNavigation.tab: pass2Field
                        Keys.onReturnPressed: root.next()
                        Keys.onEnterPressed: root.next()
                    }
                    FieldLabel { text: "Password again" }
                    MaterialTextField {
                        id: pass2Field
                        Layout.fillWidth: true
                        echoMode: TextInput.Password
                        text: root.password2
                        onTextChanged: root.password2 = text
                        KeyNavigation.tab: hostField
                        Keys.onReturnPressed: root.next()
                        Keys.onEnterPressed: root.next()
                    }
                    FieldLabel { text: "Computer name" }
                    MaterialTextField {
                        id: hostField
                        Layout.fillWidth: true
                        text: root.hostname
                        onTextChanged: root.hostname = text
                        KeyNavigation.tab: accountNext
                        Keys.onReturnPressed: root.next()
                        Keys.onEnterPressed: root.next()
                    }
                    Item { Layout.fillHeight: true }
                    RowLayout {
                        NavButton { materialIcon: "arrow_back"; mainText: "Back"; onClicked: root.goTo(1) }
                        Item { Layout.fillWidth: true }
                        NavButton { id: accountNext; materialIcon: "arrow_forward"; primary: true; mainText: "Next"; onClicked: root.next() }
                    }
                }

                // 3: region
                ColumnLayout {
                    spacing: 6
                    function focusFirst() { tzPicker.focusSearch(); }
                    PageTitle { text: "Region and keyboard" }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 16
                        Picker {
                            id: tzPicker
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            label: "Timezone"
                            model: root.opts ? root.opts.timezones : []
                            value: root.timezone
                            onPicked: v => { root.timezone = v; kmPicker.focusSearch(); }
                        }
                        Picker {
                            id: kmPicker
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            label: "Keyboard (console and desktop)"
                            model: root.opts ? root.opts.keymaps : []
                            value: root.keymap
                            onPicked: v => { root.keymap = v; locPicker.focusSearch(); }
                        }
                    }
                    Picker {
                        id: locPicker
                        Layout.fillWidth: true
                        label: "Language"
                        model: root.opts ? root.opts.locales : []
                        value: root.language
                        onPicked: v => { root.language = v; updateSwitch.forceActiveFocus(); }
                    }
                    RowLayout {
                        StyledSwitch {
                            id: updateSwitch
                            checked: root.update
                            onCheckedChanged: root.update = checked
                            KeyNavigation.tab: regionNext
                            // Space toggles; Enter moves on like the other pages.
                            Keys.onReturnPressed: root.next()
                            Keys.onEnterPressed: root.next()
                        }
                        Body {
                            text: "Update to the newest packages if this computer is online"
                        }
                    }
                    Item { Layout.fillHeight: true }
                    RowLayout {
                        NavButton { materialIcon: "arrow_back"; mainText: "Back"; onClicked: root.goTo(2) }
                        Item { Layout.fillWidth: true }
                        NavButton { id: regionNext; materialIcon: "arrow_forward"; primary: true; mainText: "Next"; onClicked: root.next() }
                    }
                }

                // 4: summary and typed confirmation
                ColumnLayout {
                    spacing: 8
                    function focusFirst() { confirmField.text = ""; confirmField.forceActiveFocus(); }
                    PageTitle { text: "Ready to install" }
                    GridLayout {
                        columns: 2
                        columnSpacing: 18
                        rowSpacing: 4
                        FieldLabel { text: "Disk" }
                        Body { text: root.disk + (root.selectedDisk() ? "  (" + root.selectedDisk().size_human + ")" : "") + "  will be erased" }
                        FieldLabel { text: "Account" }
                        Body { text: root.username + ", with sudo" }
                        FieldLabel { text: "Computer name" }
                        Body { text: root.hostname }
                        FieldLabel { text: "Timezone" }
                        Body { text: root.timezone }
                        FieldLabel { text: "Keyboard" }
                        Body { text: root.keymap }
                        FieldLabel { text: "Language" }
                        Body { text: root.language }
                        FieldLabel { text: "Updates" }
                        Body { text: root.update ? "yes, if online" : "no" }
                    }
                    NoticeBox {
                        Layout.fillWidth: true
                        materialIcon: "warning"
                        text: "Everything on " + root.disk + " will be erased. Type " + root.disk + " below to confirm."
                    }
                    MaterialTextField {
                        id: confirmField
                        Layout.fillWidth: true
                        placeholderText: root.disk
                        KeyNavigation.tab: installButton
                        Keys.onReturnPressed: if (installButton.enabled) root.startInstall()
                    }
                    Item { Layout.fillHeight: true }
                    RowLayout {
                        NavButton { materialIcon: "arrow_back"; mainText: "Back"; onClicked: root.goTo(3) }
                        Item { Layout.fillWidth: true }
                        NavButton {
                            id: installButton
                            materialIcon: "install_desktop"
                            mainText: "Erase and install"
                            primary: true
                            enabled: confirmField.text.trim() === root.disk && root.disk !== ""
                            onClicked: root.startInstall()
                        }
                    }
                }

                // 5: progress
                ColumnLayout {
                    spacing: 10
                    // Take focus off the hidden summary page's fields.
                    function focusFirst() { noteList.forceActiveFocus(); }
                    PageTitle {
                        text: root.phase === "done" ? "Arctix is installed" : root.phase === "failed" ? "The install did not finish" : "Installing Arctix..."
                    }
                    StyledProgressBar {
                        Layout.fillWidth: true
                        valueBarWidth: parent.width
                        value: root.phase === "done" ? 1 : root.stepN / Math.max(1, root.stepTotal)
                    }
                    Body {
                        text: root.phase === "running" ? root.stepDesc : root.endText
                        color: root.phase === "failed" ? Appearance.colors.colError : Appearance.colors.colOnLayer1
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        color: Appearance.colors.colLayer1
                        radius: Appearance.rounding.small
                        ListView {
                            id: noteList
                            anchors.fill: parent
                            anchors.margins: 10
                            clip: true
                            model: root.notes
                            // Deferred: the model is replaced per line, and the view
                            // shrinks when the finish buttons appear.
                            onCountChanged: Qt.callLater(positionViewAtEnd)
                            onHeightChanged: Qt.callLater(positionViewAtEnd)
                            delegate: StyledText {
                                required property string modelData
                                width: noteList.width
                                text: modelData
                                elide: Text.ElideRight
                                font.family: Appearance.font.family.monospace
                                font.pixelSize: Appearance.font.pixelSize.smaller
                                color: Appearance.colors.colSubtext
                            }
                        }
                    }
                    Body {
                        visible: root.phase === "failed"
                        text: "Nothing is left mounted. The full log is in /tmp/arctix-install.log."
                    }
                    RowLayout {
                        Item { Layout.fillWidth: true }
                        NavButton {
                            visible: root.phase !== "running"
                            materialIcon: "close"
                            mainText: "Close"
                            onClicked: root.close()
                        }
                        NavButton {
                            id: rebootButton
                            visible: root.phase === "done"
                            materialIcon: "restart_alt"
                            mainText: "Reboot now"
                            primary: true
                            onClicked: Quickshell.execDetached(["systemctl", "reboot"])
                        }
                    }
                }
            }
        }

        // Errors for the current page
        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            visible: root.pageErrors.length > 0
            Repeater {
                model: root.pageErrors
                StyledText {
                    required property string modelData
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: modelData
                    color: Appearance.colors.colError
                }
            }
        }
    }
}
