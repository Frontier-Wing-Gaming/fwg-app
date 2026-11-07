"""Stand-in for xdg-desktop-portal's FileChooser, for the Linux end-to-end tests.

    python3 fake_portal.py <file to "pick"> <log file>

Owns org.freedesktop.portal.Desktop on the session bus. OpenFile answers as if the user had picked
the given file: it returns a Request handle and then emits Request.Response with its URI, the way
the real portal does after its dialog closes. Each call is appended to the log file as JSON.
"""

import json
import sys
import urllib.parse

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

FILE_CHOOSER = "org.freedesktop.portal.FileChooser"
REQUEST = "org.freedesktop.portal.Request"

PICK, LOG = sys.argv[1], sys.argv[2]

DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()


class Request(dbus.service.Object):
    @dbus.service.signal(REQUEST, signature="ua{sv}")
    def Response(self, response, results):
        pass

    @dbus.service.method(REQUEST)
    def Close(self):
        self.remove_from_connection()


class Portal(dbus.service.Object):
    def __init__(self):
        super().__init__(bus, "/org/freedesktop/portal/desktop")
        self.count = 0

    @dbus.service.method(FILE_CHOOSER, in_signature="ssa{sv}", out_signature="o", sender_keyword="sender")
    def OpenFile(self, parent_window, title, options, sender=None):
        self.count += 1
        token = str(options.get("handle_token", f"fwg{self.count}"))
        path = "/org/freedesktop/portal/desktop/request/{}/{}".format(
            sender.lstrip(":").replace(".", "_"), token)
        request = Request(bus, path)
        with open(LOG, "a") as log:
            log.write(json.dumps({"call": "OpenFile", "title": str(title),
                                  "filters": len(options.get("filters", []))}) + "\n")
        uri = "file://" + urllib.parse.quote(PICK)

        def respond():
            request.Response(dbus.UInt32(0), {
                "uris": dbus.Array([uri], signature="s"),
                "choices": dbus.Array([], signature="(ss)"),
            })
            return False

        GLib.timeout_add(300, respond)
        return dbus.ObjectPath(path)

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface, prop):
        return self.GetAll(interface)[prop]

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface == FILE_CHOOSER:
            return {"version": dbus.UInt32(4)}
        raise dbus.exceptions.DBusException("org.freedesktop.DBus.Error.UnknownInterface",
                                            interface)


name = dbus.service.BusName("org.freedesktop.portal.Desktop", bus)
portal = Portal()
print("fake FileChooser portal ready", flush=True)
GLib.MainLoop().run()
