"""Drives a real xdg-desktop-portal file chooser over AT-SPI, for the Linux end-to-end tests.

Works with the GTK 3 dialog (xdg-desktop-portal-gtk), the GTK 4 one (xdg-desktop-portal-gnome)
and the Qt one (xdg-desktop-portal-kde): find the dialog, put the path in its file name field,
press its Open/Select button. GTK 3 reveals that field with an accessibility action; GTK 4 only
shows it when you type "/", which is what `type_text` is for.
"""

import time

import pyatspi

PORTAL_APPS = ("xdg-desktop-portal-gtk", "xdg-desktop-portal-gnome", "xdg-desktop-portal-kde")
OPEN_BUTTONS = ("Open", "&Open", "Select")


def _walk(node, depth=0):
    if node is None or depth > 40:
        return
    yield node
    try:
        children = list(node)
    except Exception:
        return
    for child in children:
        yield from _walk(child, depth + 1)


def _actions(node):
    try:
        action = node.queryAction()
        return {action.getName(i): i for i in range(action.nActions)}
    except Exception:
        return {}


def _showing(node):
    try:
        return node.getState().contains(pyatspi.STATE_SHOWING)
    except Exception:
        return False


def find_dialog(timeout=20):
    """(portal app name, dialog window) of the first file chooser on screen."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for app in pyatspi.Registry.getDesktop(0):
            try:
                if app is None or app.name not in PORTAL_APPS:
                    continue
                for win in app:
                    if win is not None and _showing(win):
                        return app.name, win
            except Exception:
                continue
        time.sleep(0.3)
    raise RuntimeError("no file chooser dialog appeared")


def _editable_texts(win):
    found = []
    for node in _walk(win):
        try:
            if _showing(node) and node.getState().contains(pyatspi.STATE_EDITABLE):
                node.queryEditableText()
                found.append(node)
        except Exception:
            pass
    return found


def pick(path, type_text=None, log=print):
    """Chooses `path` in the open file chooser. Returns the portal backend's name."""
    app, win = find_dialog()
    log(f"    file chooser: {app}")
    for node in _walk(win):
        actions = _actions(node)
        if "show_location" in actions:  # GTK 3
            node.queryAction().doAction(actions["show_location"])
            time.sleep(0.5)
            break
    texts = _editable_texts(win)
    if not texts and type_text:  # GTK 4: typing "/" opens the location entry
        type_text("/")
        time.sleep(1)
        texts = _editable_texts(win)
    if not texts:
        raise RuntimeError(f"the {app} dialog has no file name field")
    texts[0].queryEditableText().setTextContents(path)
    time.sleep(0.5)
    for node in _walk(win):
        if node.getRoleName() == "push button" and node.name in OPEN_BUTTONS and _showing(node):
            node.queryAction().doAction(0)
            return app
    raise RuntimeError(f"the {app} dialog has no Open button")
