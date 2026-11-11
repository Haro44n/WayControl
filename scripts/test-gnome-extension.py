#!/usr/bin/env python3
"""Static contract and packaging checks for the optional GNOME Shell bridge."""
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
extension_dir = root / "packaging/gnome/extension"
metadata = json.loads((extension_dir / "metadata.json").read_text())
source = (extension_dir / "extension.js").read_text()
pkgbuild = (root / "packaging/arch/PKGBUILD").read_text()
nested_test = (root / "scripts/test-gnome-nested.sh").read_text()

assert metadata["uuid"] == "waycontrol@haro44n.github.io"
assert metadata["shell-version"][0] == "45"
assert metadata["shell-version"][-1] == "51"

xml_match = re.search(r"const INTERFACE_XML = `([\s\S]*?)`;", source)
assert xml_match, "missing fixed D-Bus XML interface"
interface = ET.fromstring(xml_match.group(1)).find("interface")
assert interface is not None and interface.attrib["name"] == "org.waycontrol.GnomeShell1"
expected = {
    "GetVersion": ("", "u"),
    "ListWorkspaces": ("", "s"),
    "ListWindows": ("", "s"),
    "SwitchWorkspace": ("s", "b"),
    "FocusWindow": ("s", "b"),
    "MoveWindow": ("ss", "b"),
    "ToggleFullscreen": ("s", "b"),
    "CloseWindow": ("s", "b"),
}
actual = {}
for method in interface.findall("method"):
    inputs = "".join(arg.attrib["type"] for arg in method.findall("arg") if arg.attrib.get("direction") == "in")
    outputs = "".join(arg.attrib["type"] for arg in method.findall("arg") if arg.attrib.get("direction") == "out")
    actual[method.attrib["name"]] = (inputs, outputs)
assert actual == expected, f"unexpected D-Bus method signatures: {actual}"
signal = interface.find("signal")
assert signal is not None and signal.attrib["name"] == "Changed"
assert [arg.attrib["type"] for arg in signal.findall("arg")] == ["s"]

assert "Gio.bus_own_name" in source and "Gio.DBusExportedObject.wrapJSObject" in source
assert "get_stable_sequence" in source and "_workspaceFor(id)" in source and "_windowFor(id)" in source
assert "make_fullscreen" in source and "unmake_fullscreen" in source
assert "eval(" not in source and "imports.misc" not in source
assert "toggle_floating" not in source.lower()
assert "gnome-shell-extension" in pkgbuild
assert "extension.js" in pkgbuild and "metadata.json" in pkgbuild
assert not re.search(r"gsettings\s+set|gnome-extensions\s+enable|systemctl.*enable", pkgbuild)
assert nested_test.index("gnome-shell --nested --wayland") < nested_test.index("gnome-extensions enable")
print("GNOME extension contract and opt-in packaging checks passed")
