"""Read resolved GTK theme roles in a separate process from GPUI's event loop."""
import json
import sys

import gi

gi.require_version('Gtk', '3.0')
from gi.repository import Gio, Gtk

initialized, _ = Gtk.init_check([])
if not initialized:
    raise RuntimeError('GTK could not connect to the desktop display')
settings = Gtk.Settings.get_default()
preference, platform = sys.argv[1:]
if preference == 'system':
    interface = Gio.Settings.new('org.gnome.desktop.interface')
    scheme = interface.get_string('color-scheme')
    dark = scheme == 'prefer-dark' or (scheme == 'default' and platform == 'dark')
else:
    dark = preference == 'dark'
settings.set_property('gtk-application-prefer-dark-theme', dark)
window = Gtk.Window()
context = window.get_style_context()
roles = {}
for name in (
    'theme_bg_color', 'theme_fg_color', 'theme_base_color', 'theme_text_color',
    'theme_selected_bg_color', 'theme_selected_fg_color', 'tooltip_bg_color',
    'tooltip_fg_color', 'borders', 'insensitive_fg_color',
):
    found, color = context.lookup_color(name)
    if found:
        roles[name] = [color.red, color.green, color.blue, color.alpha]
print(json.dumps(roles))
window.destroy()
