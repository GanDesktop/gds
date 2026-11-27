"""Images workflows verify exported pixels with Pillow, independent of GDS rendering."""

import base64
import io

from PIL import Image, ImageChops
import pytest

from driver import eventually

pytestmark = pytest.mark.images


def test_I01_gallery_navigation_and_panels(case):
    original = case.sample_image('01-first.png')
    case.sample_image('02-second.png', (60, 90))
    case.launch('images', case.path('01-first.png'))
    case.expect('01-first.png', 'image')
    case.expect(contains='80 × 48')
    case.key('right')
    case.expect('02-second.png', 'image')
    case.expect(contains='60 × 90')
    case.key('left')
    case.expect('01-first.png', 'image')
    case.key('ctrl+i')
    case.expect_state('Open right sidebar', 'pressed', False)
    case.key('ctrl+b')
    case.expect_state('Toggle Sidebar', 'pressed', False)
    case.activate('02-second.png', 'button')
    case.expect('02-second.png', 'image')
    case.activate('01-first.png', 'button')
    case.key('ctrl+1')
    case.expect('01-first.png', 'image')
    case.expect_image('01-first.png', original)
    case.checkpoint('gallery')


def test_I02_rotate_undo_redo_save_and_reopen(case):
    original = case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('r')
    case.expect(contains='48 × 80')
    case.key('ctrl+z')
    case.expect(contains='80 × 48')
    case.key('ctrl+shift+z')
    case.expect(contains='48 × 80')
    case.key('ctrl+s')
    case.expect_image('original.png', original.transpose(Image.Transpose.ROTATE_270))
    case.quit()
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.expect(contains='48 × 80')


def test_I03_flip_save_preserves_exact_pixels(case):
    original = case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('h')
    case.key('ctrl+s')
    case.expect_image('original.png', original.transpose(Image.Transpose.FLIP_LEFT_RIGHT))


def test_I04_copy_image_to_real_desktop_clipboard(case):
    original = case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    with case.step('Read image/png through guest Wayland clipboard'):
        encoded = case.desktop.call('clipboard', mime='image/png')
        copied = Image.open(io.BytesIO(base64.b64decode(encoded))).convert('RGBA')
        assert copied.size == original.size
        assert copied.tobytes() == original.tobytes()


def test_I05_stdin_capture_save_and_exit(case):
    original = case.sample_image('capture-source.png')
    case.launch('images', '--stdin', '--output', case.path('saved.png'), '--early-exit',
                stdin=case.path('capture-source.png'))
    case.expect(role='image')
    case.key('ctrl+s')
    case.expect_image('saved.png', original)
    eventually(lambda: case.desktop.call('alive', pid=case.pid), lambda alive: not alive,
               'capture exits after save')
    case.expect_exit(0)
    case.expect_image('capture-source.png', original)


def test_I06_rename_image_preserves_contents(case):
    original = case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('ctrl+l')
    case.expect_entry()
    case.type('uncommitted.png')
    case.key('tab')
    case.expect('Rename File', 'entry', present=False)
    case.expect('original.png', 'label')
    case.expect_file('uncommitted.png', exists=False)
    case.key('ctrl+l')
    case.expect_entry()
    case.key('ctrl+a')
    case.type('renamed.png')
    case.key('enter')
    case.expect('renamed.png', 'image')
    case.expect_image('renamed.png', original)
    case.expect_file('original.png', exists=False)


def test_I07_external_change_blocks_overwrite(case):
    case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('r')
    case.expect(contains='48 × 80')
    external = case.sample_image('original.png', (30, 20))
    case.key('ctrl+s')
    case.expect(contains='was modified externally', role='notification')
    case.expect_image('original.png', external)


def test_I08_settings_return_to_document(case):
    original = case.sample_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    case.key('ctrl+comma')
    case.expect('Appearance', 'heading')
    case.key('ctrl+tab')
    case.expect('Viewer', 'heading')
    case.activate('Export & Privacy', 'button')
    case.expect('Export & Privacy', 'heading')
    case.expect('JPEG Export Quality', 'slider')
    case.checkpoint('export-settings')
    case.key('ctrl+shift+tab')
    case.expect('Viewer', 'heading')
    case.activate('Appearance', 'button')
    case.key('escape')
    case.expect('original.png', 'image')
    case.expect_image('original.png', original)


def test_I09_crop_with_real_pointer_and_save(case):
    original = case.coordinate_image('original.png')
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    target = case.image_target(original)
    case.key('3')
    case.drag_image(target, (40, 30), (240, 150))
    case.activate('Apply Crop')
    case.expect('Apply Crop', present=False)
    width, height = case.image_dimensions()
    # Absolute tablet coordinates can straddle a pixel boundary by less than one pixel.
    assert 198 <= width <= 202 and 118 <= height <= 122
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    cropped = Image.open(io.BytesIO(case.desktop.read(case.path('original.png')))).convert('RGBA')
    left, top, _, _ = cropped.getpixel((0, 0))
    assert 39 <= left <= 41 and 29 <= top <= 31
    assert 239 <= left + width <= 241 and 149 <= top + height <= 151
    case.expect_image('original.png', original.crop((left, top, left + width, top + height)))


def test_I10_rectangle_annotation_is_exported(case):
    original = case.sample_image('original.png', (320, 200))
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    target = case.image_target(original)
    case.key('4')
    case.drag_image(target, (40, 30), (240, 150))
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    with case.step('Verify annotation pixels and unchanged surrounding content'):
        saved = Image.open(io.BytesIO(case.desktop.read(case.path('original.png')))).convert('RGB')
        assert saved.size == original.size
        changes = ImageChops.difference(saved, original.convert('RGB'))
        bounds = changes.getbbox()
        assert bounds is not None, 'Saved annotation must change pixels'
        assert 30 <= bounds[0] <= 45 and 20 <= bounds[1] <= 35
        assert 235 <= bounds[2] <= 250 and 145 <= bounds[3] <= 160
        assert changes.crop((135, 25, 145, 35)).getbbox() is not None
        assert changes.crop((35, 85, 45, 95)).getbbox() is not None
        assert changes.crop((130, 80, 150, 100)).getbbox() is None
        assert changes.crop((0, 0, 320, 15)).getbbox() is None
        assert changes.crop((0, 170, 320, 200)).getbbox() is None


def test_I11_move_annotation_and_repeat_delete_safety(case):
    original = case.sample_image('original.png', (320, 200))
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    target = case.image_target(original)
    case.key('4')
    case.drag_image(target, (40, 30), (120, 90))
    case.key('1')
    # Drag on the top stroke away from NW (40, 30) and N (80, 30) handles to translate (+100, +50).
    case.drag_image(target, (60, 30), (160, 80))
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    with case.step('Verify moved rectangle pixels in guest Wayland clipboard'):
        encoded = case.desktop.call('clipboard', mime='image/png')
        copied = Image.open(io.BytesIO(base64.b64decode(encoded))).convert('RGBA')
        assert copied.size == original.size
        changes = ImageChops.difference(copied.convert('RGB'), original.convert('RGB'))
        assert changes.crop((35, 25, 75, 35)).getbbox() is None
        assert changes.crop((35, 25, 125, 95)).getbbox() is None
        assert changes.crop((135, 75, 225, 145)).getbbox() is not None
        assert changes.crop((175, 75, 185, 86)).getbbox() is not None
        assert changes.crop((135, 105, 146, 115)).getbbox() is not None
        assert changes.crop((170, 105, 190, 115)).getbbox() is None
        assert changes.crop((0, 0, 320, 65)).getbbox() is None
        assert changes.crop((0, 155, 320, 200)).getbbox() is None
    case.key('delete')
    case.key('delete')
    case.key('delete')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    case.expect_file('original.png', exists=True)
    case.expect_empty_trash()
    case.expect_image('original.png', original)


def test_I12_rotate_and_crop_preserve_annotations(case):
    original = case.sample_image('original.png', (320, 200))
    case.launch('images', case.path('original.png'))
    case.expect('original.png', 'image')
    target = case.image_target(original)
    case.key('4')
    case.drag_image(target, (60, 40), (160, 120))
    case.key('escape')
    case.key('escape')
    case.key('r')
    case.expect(contains='200 × 320')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    with case.step('Verify rotated rectangle strokes and unchanged background'):
        expected_base = original.transpose(Image.Transpose.ROTATE_270).convert('RGB')
        saved = Image.open(io.BytesIO(case.desktop.read(case.path('original.png')))).convert('RGB')
        assert saved.size == (200, 320)
        changes = ImageChops.difference(saved, expected_base)
        bounds = changes.getbbox()
        assert bounds is not None, 'Rotated annotation must change pixels'
        assert 74 <= bounds[0] <= 85 and 54 <= bounds[1] <= 65
        assert 155 <= bounds[2] <= 166 and 155 <= bounds[3] <= 166
        assert changes.crop((115, 55, 125, 66)).getbbox() is not None
        assert changes.crop((75, 105, 86, 115)).getbbox() is not None
        assert changes.crop((95, 75, 145, 145)).getbbox() is None
        assert changes.crop((0, 0, 70, 320)).getbbox() is None
        assert changes.crop((170, 0, 200, 320)).getbbox() is None
    case.quit()
    crop_src = case.coordinate_image('crop-source.png')
    case.launch('images', case.path('crop-source.png'))
    case.expect('crop-source.png', 'image')
    crop_target = case.image_target(crop_src)
    case.key('4')
    case.drag_image(crop_target, (60, 40), (160, 120))
    case.key('escape')
    case.key('3')
    case.drag_image(crop_target, (90, 20), (240, 160))
    case.activate('Apply Crop')
    case.expect('Apply Crop', present=False)
    width, height = case.image_dimensions()
    assert 148 <= width <= 152 and 138 <= height <= 142
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    with case.step('Verify cropped clipboard retains clipped rectangle without border artifacts'):
        encoded = case.desktop.call('clipboard', mime='image/png')
        cropped = Image.open(io.BytesIO(base64.b64decode(encoded))).convert('RGBA')
        assert cropped.size == (width, height)
        left, top, blue, _ = cropped.getpixel((0, 0))
        assert blue == 123 and 89 <= left <= 91 and 19 <= top <= 21
        expected_crop = crop_src.crop((left, top, left + width, top + height)).convert('RGB')
        crop_changes = ImageChops.difference(cropped.convert('RGB'), expected_crop)
        assert crop_changes.crop((20, 40 - top - 5, 45, 40 - top + 6)).getbbox() is not None
        assert crop_changes.crop((160 - left - 5, 50, 160 - left + 6, 70)).getbbox() is not None
        assert crop_changes.crop((20, 120 - top - 5, 45, 120 - top + 6)).getbbox() is not None
        assert crop_changes.crop((0, 40 - top + 8, 6, 120 - top - 8)).getbbox() is None
        assert crop_changes.crop((15, 40 - top + 8, 160 - left - 10, 120 - top - 8)).getbbox() is None
        assert crop_changes.crop((160 - left + 8, 0, width, height)).getbbox() is None
    case.key('ctrl+z')
    case.expect(contains='320 × 200')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    with case.step('Verify crop undo restores complete rectangle'):
        restored = Image.open(io.BytesIO(case.desktop.read(case.path('crop-source.png')))).convert('RGB')
        assert restored.size == (320, 200)
        restored_changes = ImageChops.difference(restored, crop_src.convert('RGB'))
        assert restored_changes.crop((55, 70, 66, 90)).getbbox() is not None
        assert restored_changes.crop((155, 70, 166, 90)).getbbox() is not None
        assert restored_changes.crop((75, 55, 145, 105)).getbbox() is None


def test_I13_text_draft_saved_from_focused_editor(case):
    original = case.sample_image('text.png', (320, 200))
    case.launch('images', case.path('text.png'))
    case.expect('text.png', 'image')
    target = case.image_target(original)
    case.key('8')
    x, y = target['x'] + 70, target['y'] + 50
    case.desktop.pointer(x, y, target['screen'], True)
    case.desktop.pointer(x, y, target['screen'], False)
    case.expect('Add Text', 'button')
    case.expect_entry()
    entry = case.expect('Text', 'entry')[0]['bounds']
    cancel = case.expect('Cancel', 'button')[0]['bounds']
    apply = case.expect('Add Text', 'button')[0]['bounds']
    assert cancel[1] >= entry[1] + entry[3] - 1 and apply[1] == cancel[1]
    assert cancel[0] < apply[0]
    case.type('discard')
    case.checkpoint('text-editor')
    case.activate('Cancel', 'button')
    case.expect('Add Text', present=False)
    case.desktop.pointer(x, y, target['screen'], True)
    case.desktop.pointer(x, y, target['screen'], False)
    case.expect_entry()
    case.type('GDS')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    saved = Image.open(io.BytesIO(case.desktop.read(case.path('text.png')))).convert('RGB')
    changes = ImageChops.difference(saved, original.convert('RGB'))
    bounds = changes.getbbox()
    assert bounds is not None
    assert 65 <= bounds[0] < bounds[2] <= 145 and 45 <= bounds[1] < bounds[3] <= 95


def test_I14_cancel_pending_drag_before_clipboard_export(case):
    original = case.sample_image('draft.png', (320, 200))
    case.launch('images', case.path('draft.png'))
    case.expect('draft.png', 'image')
    target = case.image_target(original)
    case.key('4')
    case.activate('Copy Image', focus=True)
    case.desktop.pointer(target['x'] + 30, target['y'] + 30, target['screen'], True)
    try:
        case.expect_state('draft.png', 'focused')
        case.desktop.pointer(target['x'] + 170, target['y'] + 120, target['screen'])
        case.key('ctrl+c')
        case.expect('Copied image to clipboard')
    finally:
        case.desktop.pointer(target['x'] + 170, target['y'] + 120, target['screen'], False)
    encoded = case.desktop.call('clipboard', mime='image/png')
    copied = Image.open(io.BytesIO(base64.b64decode(encoded))).convert('RGBA')
    assert copied.size == original.size and copied.tobytes() == original.tobytes()
    case.quit()
    case.expect_image('draft.png', original)


def test_I15_app_menu_and_discard_confirmation(case):
    original = case.sample_image('first.png')
    case.sample_image('next.png')
    case.launch('images', case.path('first.png'))
    case.expect('first.png', 'image')
    case.activate('Application Menu')
    case.activate('Settings')
    case.expect('Viewer')
    case.key('escape')
    case.expect('first.png', 'image')
    case.key('r')
    case.expect(contains='48 × 80')
    case.key('right')
    def action_widths():
        actions = {n['name']: n['bounds'][2] for n in case.tree()
                   if n['role'] == 'button' and 'showing' in n['states']}
        return (actions.get('Keep Editing', 0), actions.get('Discard', 0))

    case.expect('Keep Editing', 'button')
    widths = action_widths()
    alt = {'type': 'qcode', 'data': 'alt'}
    case.desktop.qmp([{'type': 'key', 'data': {'key': alt, 'down': True}}])
    try:
        eventually(action_widths, lambda w: w[0] > widths[0] and w[1] > widths[1],
                   'both dialog buttons reveal their shortcut hints')
        case.checkpoint('discard-shortcuts')
    finally:
        case.desktop.qmp([{'type': 'key', 'data': {'key': alt, 'down': False}}])
    case.key('alt+c')
    case.expect('Keep Editing', present=False)
    case.expect(contains='48 × 80')
    case.key('right')
    case.expect('Discard', 'button')
    case.key('alt+a')
    case.expect('next.png', 'image')
    case.expect_image('first.png', original)


def test_I16_privacy_draft_survives_pixel_export(case):
    original = case.sample_image('private.png')
    exif = Image.Exif()
    exif[315] = 'Photographer'
    exif[305] = 'Private workstation'
    payload = io.BytesIO()
    original.save(payload, format='PNG', exif=exif)
    case.write('private.png', payload.getvalue())
    case.launch('images', case.path('private.png'))
    case.expect('private.png', 'image')
    case.key('r')
    case.expect(contains='48 × 80')
    case.activate('Clear Metadata')
    case.expect('Privacy tags cleared in draft; save to apply.')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    case.expect_image('private.png', original.transpose(Image.Transpose.ROTATE_270))
    saved = Image.open(io.BytesIO(case.desktop.read(case.path('private.png'))))
    assert 315 not in saved.getexif() and 305 not in saved.getexif()


def test_I17_checkerboard_is_display_only(case):
    base = case.sample_image('a.png', (320, 200))
    transparent = base.copy()
    transparent.paste((0, 0, 0, 0), (0, 0, 96, 64))
    output = io.BytesIO()
    transparent.save(output, format='PNG')
    case.write('b.png', output.getvalue())
    case.launch('images', case.path('a.png'))
    case.expect('a.png', 'image')
    target = case.image_target(base)
    case.key('right')
    case.expect('b.png', 'image')
    case.key('ctrl+1')

    def backdrop(name):
        path = case.artifacts / name
        case.desktop.screenshot(path)
        with Image.open(path) as screen:
            x, y = target['x'], target['y']
            # Match opaque pixels too, so a blank or displaced canvas cannot pass.
            screen = screen.convert('RGB')
            if screen.getpixel((x + 110, y + 60)) != base.getpixel((110, 60))[:3]:
                return set()
            patch = screen.crop((x + 3, y + 3, x + 50, y + 35))
            return {color for _, color in patch.getcolors(patch.width * patch.height)}

    eventually(lambda: backdrop('checkerboard-on.png'), lambda colors: len(colors) == 2,
               'two alternating square colors behind transparent pixels')
    case.key('ctrl+comma')
    case.activate('Viewer', 'button')
    case.activate('Alpha checkerboard backdrop', 'toggle button')
    case.key('escape')
    case.expect('b.png', 'image')
    case.key('ctrl+1')
    eventually(lambda: backdrop('checkerboard-off.png'), lambda colors: len(colors) == 1,
               'solid backdrop after disabling checkerboard')
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    copied = Image.open(io.BytesIO(base64.b64decode(case.desktop.call('clipboard', mime='image/png'))))
    assert copied.convert('RGBA').tobytes() == transparent.tobytes()


def test_I18_fixed_right_panel_collapses_and_persists(case):
    case.sample_image('panel.png')
    case.launch('images', case.path('panel.png'))
    before = case.expect('panel.png', 'image')[0]['bounds'][2]
    case.expect('Resize properties panel', present=False)
    case.activate('Application Menu')
    case.expect('Settings', 'menu item')
    case.expect('Collapse right sidebar', 'menu item', present=False)
    case.key('escape')
    case.activate('Collapse right sidebar')
    case.expect_state('Open right sidebar', 'pressed', False)

    def viewport_width():
        return next((n['bounds'][2] for n in case.tree() if n['name'] == 'panel.png'
                     and n['role'] == 'image' and 'showing' in n['states']), None)

    collapsed = eventually(viewport_width, lambda width: width is not None and width >= before + 216,
                           'collapsing the right panel releases its width')
    case.quit()
    case.launch('images', case.path('panel.png'))
    case.expect_state('Open right sidebar', 'pressed', False)
    eventually(viewport_width, lambda width: width == collapsed, 'restored collapsed panel')
    case.key('ctrl+i')
    case.expect_state('Collapse right sidebar', 'pressed', True)
    eventually(viewport_width, lambda width: width == before, 'restored open panel width')


def expect_style_value(case, name, value):
    eventually(case.tree, lambda nodes: any(n['name'] == name and n['description'] == value
               and 'showing' in n['states'] for n in nodes), f'{name} shows {value}')


def test_I19_ring_and_triangle_color_applies_to_annotations(case):
    original = case.sample_image('palette.png', (320, 200))
    case.launch('images', case.path('palette.png'))
    case.expect('palette.png', 'image')
    target = case.image_target(original)
    viewport = case.expect('palette.png', 'image')[0]['bounds']
    # AT-SPI reports window coordinates. Locate the screen origin via the 1:1 image.
    window_x = target['x'] - viewport[0] - round((viewport[2] - original.width) / 2)
    window_y = target['y'] - viewport[1] - round((viewport[3] - original.height) / 2)

    def pick_color(start, end):
        x, y, w, h = case.expect('Color selector')[0]['bounds']
        def ring_bounds():
            path = case.artifacts / 'wheel.png'
            case.desktop.screenshot(path)
            with Image.open(path) as shot:
                # Locate the painted ring, including the popover opening transition.
                box = (window_x + x - 12, window_y + y - 12, window_x + x + w + 12, window_y + y + h + 12)
                _, saturation, value = shot.crop(box).convert('HSV').split()
                saturated = saturation.point(lambda s: 255 if s >= 248 else 0)
                bright = value.point(lambda v: 255 if v >= 248 else 0)
                bounds = ImageChops.darker(saturated, bright).getbbox()
                if bounds and abs((bounds[2] - bounds[0]) - (bounds[3] - bounds[1])) < 4:
                    return (box[0] + (bounds[0] + bounds[2]) / 2,
                            box[1] + (bounds[1] + bounds[3]) / 2,
                            (bounds[2] - bounds[0]) / 2)

        cx, cy, radius = eventually(ring_bounds, lambda b: b and b[2] > min(w, h) * 0.43,
                                    'visible color wheel')
        def point(offset):
            return round(cx + offset[0] * radius), round(cy + offset[1] * radius)

        case.desktop.pointer(*point(start), target['screen'])
        case.desktop.pointer(*point(start), target['screen'], True)
        case.expect_state('Color selector', 'focused')
        case.desktop.pointer(*point(end), target['screen'])
        case.desktop.pointer(*point(end), target['screen'], False)

    case.key('4')
    case.expect_state('#EF4444', 'pressed')
    case.activate('Choose color', 'toggle button')
    case.expect_state('Choose color', 'pressed')
    case.expect_state('#EF4444', 'pressed', False)
    # Green is one third of a clockwise turn from red at twelve o'clock.
    pick_color((0, -0.9), (0.90 * 0.866, 0.90 * 0.5))
    pick_color((0, 0), (0.62 * 0.866, 0.62 * 0.5))
    case.key('escape')
    case.expect('Color selector', present=False)
    case.expect_state('Choose color', 'pressed')
    case.drag_image(target, (40, 30), (180, 130))
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')

    def stroke():
        data = case.desktop.call('clipboard', mime='image/png')
        return Image.open(io.BytesIO(base64.b64decode(data))).convert('RGB').getpixel((110, 30))

    green = stroke()
    assert green[1] > 225 and green[0] < 42 and green[2] < 42, green
    # The center mixes equal parts pure green, white and black. Recolor the existing rectangle.
    case.key('1')
    case.activate('Choose color', 'toggle button')
    pick_color((0.2, 0.1), (0, 0))
    case.checkpoint('color-selector')
    case.key('escape')
    case.expect('Color selector', present=False)
    case.key('ctrl+c')
    eventually(stroke, lambda rgb: 65 < rgb[0] < 110 and 145 < rgb[1] < 195 and 65 < rgb[2] < 110,
               'triangle changes the selected stroke to muted green')
    case.key('ctrl+z')
    case.key('ctrl+c')
    eventually(stroke, lambda rgb: rgb == green, 'undo restores the previous stroke color')


def test_I20_blur_previews_before_release_and_preserves_surrounding_pixels(case):
    original = case.sample_image('preview.png', (696, 288))
    case.launch('images', case.path('preview.png'))
    case.expect('preview.png', 'image')
    target = case.image_target(original)
    case.key('0')
    case.expect_state('Blur', 'pressed')
    case.activate('Copy Image', focus=True)
    case.expect_state('Copy Image', 'focused')
    x, y = target['x'], target['y']
    case.desktop.pointer(x + 185, y + 48, target['screen'], True)
    case.expect_state('preview.png', 'focused')
    case.desktop.pointer(x + 290, y + 124, target['screen'])

    def observe():
        path = case.artifacts / 'blur-preview.png'
        case.desktop.screenshot(path)
        with Image.open(path) as frame:
            frame = frame.convert('RGB')
            # Sample an untouched point in the changed tile, and one in a neighboring tile.
            assert frame.getpixel((x + 20, y + 25)) == original.getpixel((20, 25))[:3]
            assert frame.getpixel((x + 588, y + 80)) == original.getpixel((588, 80))[:3]
            return frame.getpixel((x + 231, y + 85))

    preview = eventually(observe, lambda rgb: 55 < rgb[0] < 180 and 90 < rgb[2] < 215,
                         'the sharp blue-orange edge blurs while the pointer is still down')
    case.desktop.pointer(x + 290, y + 124, target['screen'], False)
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    data = case.desktop.call('clipboard', mime='image/png')
    with Image.open(io.BytesIO(base64.b64decode(data))) as exported:
        assert preview == exported.convert('RGB').getpixel((231, 85))
    assert observe() == preview


def test_I21_shared_style_controls_custom_stroke_and_tool_defaults(case):
    original = case.sample_image('styles.png', (320, 200))
    case.launch('images', case.path('styles.png'))
    case.expect('styles.png', 'image')
    target = case.image_target(original)
    properties_y = case.expect('Properties', 'heading')[0]['bounds'][1]

    def bounds(name, role):
        return case.expect(name, role)[0]['bounds']

    case.key('4')
    case.expect('Tool style')
    expect_style_value(case, 'Stroke · Custom', '3 px')
    assert len(case.expect('Choose color', 'toggle button')) == 1
    case.expect('Color selector', present=False)
    color = bounds('Color', 'label')
    stroke = bounds('Stroke', 'label')
    fmt = bounds('Format', 'label')
    dimensions = bounds('Dimensions', 'label')
    tool = bounds('Tool style', 'heading')
    properties = bounds('Properties', 'heading')
    assert color[0] == stroke[0] == fmt[0] == dimensions[0] == tool[0] == properties[0]
    assert color[1] - tool[1] == fmt[1] - properties[1]
    assert stroke[1] - color[1] == dimensions[1] - fmt[1]
    picker = bounds('Choose color', 'toggle button')
    custom = bounds('Stroke · Custom', 'toggle button')
    first_swatch = bounds('#EF4444', 'toggle button')
    first_size = bounds('Stroke 2', 'toggle button')
    assert color[0] + color[2] <= first_swatch[0] < picker[0]
    assert stroke[0] + stroke[2] <= first_size[0] < custom[0]
    assert abs(color[1] + color[3] / 2 - (picker[1] + picker[3] / 2)) <= 1
    assert abs(stroke[1] + stroke[3] / 2 - (custom[1] + custom[3] / 2)) <= 1
    assert picker[3] == custom[3] and abs(picker[0] + picker[2] - custom[0] - custom[2]) <= 1
    case.expect('#EF4444', 'label', present=False)
    case.activate('#F59E0B', 'toggle button')
    case.expect_state('#F59E0B', 'pressed')
    case.expect_state('Choose color', 'pressed', False)
    expect_style_value(case, 'Choose color', '#F59E0B')
    case.activate('Stroke 6', 'toggle button')
    expect_style_value(case, 'Stroke · Custom', '3 px')
    case.drag_image(target, (40, 30), (180, 130))
    case.expect('1 selected')
    assert len(case.expect('Choose color', 'toggle button')) == 1
    assert case.expect('Properties', 'heading')[0]['bounds'][1] == properties_y
    expect_style_value(case, 'Choose color', '#F59E0B')
    expect_style_value(case, 'Stroke · Custom', '3 px')

    def enter_width(value):
        case.activate('Stroke · Custom', 'toggle button')
        case.expect_entry()
        case.type(value)

    case.activate('#3B82F6', 'toggle button')
    case.key('1')
    case.expect('Object style', 'heading')
    expect_style_value(case, 'Choose color', '#F59E0B')
    case.expect_state('Stroke 6', 'pressed')
    enter_width('9.5')
    case.key('enter')
    expect_style_value(case, 'Stroke · Custom', '10 px')
    case.expect('Stroke', 'entry', present=False)
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    exported = Image.open(io.BytesIO(base64.b64decode(case.desktop.call('clipboard', mime='image/png')))).convert('RGB')
    assert exported.getpixel((105, 30)) == (245, 158, 11)
    # Count covered samples across the stroke, including the antialiased subpixel edges.
    before = original.convert('RGB')
    coverage = sum(exported.getpixel((105, y)) != before.getpixel((105, y)) for y in range(18, 44))
    assert 9 <= coverage <= 11, coverage
    assert exported.getpixel((105, 37)) == original.convert('RGB').getpixel((105, 37))
    case.checkpoint('selection-style')

    enter_width('0')
    case.key('enter')
    case.expect('Stroke', 'entry')
    expect_style_value(case, 'Stroke · Custom', '10 px')
    case.key('escape')
    case.expect('Stroke', 'entry', present=False)
    expect_style_value(case, 'Stroke · Custom', '10 px')
    case.key('ctrl+z')
    expect_style_value(case, 'Stroke · Custom', '10 px')
    case.expect_state('Stroke 6', 'pressed')
    case.key('ctrl+shift+z')
    expect_style_value(case, 'Stroke · Custom', '10 px')

    case.key('0')
    case.expect('Tool style')
    case.expect('Blur Radius')
    expect_style_value(case, 'Blur Radius · Custom', '8 px')
    assert case.expect('Properties', 'heading')[0]['bounds'][1] == properties_y
    case.expect('Choose color', present=False)
    case.expect('Stroke · Custom', present=False)
    case.expect('Tool options', present=False)
    case.key('4')
    expect_style_value(case, 'Choose color', '#3B82F6')
    expect_style_value(case, 'Stroke · Custom', '3 px')
    case.activate('Stroke 2', 'toggle button')
    case.drag_image(target, (200, 50), (270, 110))
    case.key('ctrl+a')
    case.expect('2 selected')
    expect_style_value(case, 'Choose color', 'Mixed')
    expect_style_value(case, 'Stroke · Custom', '10 px')
    case.expect_state('Stroke 3', 'pressed', False)
    case.activate('Stroke 3', 'toggle button')
    case.expect_state('Stroke 3', 'pressed')
    expect_style_value(case, 'Stroke · Custom', '10 px')
    expect_style_value(case, 'Choose color', 'Mixed')


def test_I22_off_canvas_geometry_and_number_markers(case):
    original = case.sample_image('objects.png', (320, 200)).convert('RGB')
    case.launch('images', case.path('objects.png'))
    target = case.image_target(original)
    case.key('4')
    case.drag_image(target, (42, 30), (-30, 110))
    case.key('1')
    # Move the right edge back into the image, revealing the original left edge.
    case.drag_image(target, (42, 52), (112, 52))
    case.key('n')
    case.drag_image(target, (170, 65), (170, 65))
    case.drag_image(target, (230, 65), (230, 65))
    case.key('ctrl+z')
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    first = Image.open(io.BytesIO(base64.b64decode(case.desktop.call('clipboard', mime='image/png')))).convert('RGB')
    case.key('ctrl+shift+z')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    both = Image.open(io.BytesIO(case.desktop.read(case.path('objects.png')))).convert('RGB')
    assert first.getpixel((40, 65)) != original.getpixel((40, 65))
    assert first.getpixel((70, 65)) == original.getpixel((70, 65))
    assert first.getpixel((112, 65)) != original.getpixel((112, 65))
    assert first.crop((214, 49, 246, 81)).tobytes() == original.crop((214, 49, 246, 81)).tobytes()
    assert first.getpixel((170, 53)) == both.getpixel((230, 53)) == (239, 68, 68)
    glyph_one = both.crop((164, 58, 176, 72))
    glyph_two = both.crop((224, 58, 236, 72))
    assert ImageChops.difference(glyph_one, glyph_two).getbbox()
    case.checkpoint('number-markers')


def test_I23_text_font_changes_exported_glyphs(case):
    original = case.sample_image('font.png', (320, 200)).convert('RGB')
    case.launch('images', case.path('font.png'))
    target = case.image_target(original)
    case.key('8')
    custom = 'Font Size · Custom'
    case.activate(custom, 'toggle button')
    case.expect_entry()
    case.type('20')
    case.key('enter')
    case.expect_state(custom, 'pressed')
    case.expect_state('Font Size 20', 'pressed', False)
    case.activate('Font Size 30', 'toggle button')
    case.expect_state('Font Size 30', 'pressed')
    expect_style_value(case, custom, '20 px')
    case.activate('Font Size 14', 'toggle button')
    expect_style_value(case, custom, '20 px')
    case.activate('Font Size 20', 'toggle button')
    case.expect_state('Font Size 20', 'pressed')
    case.expect_state(custom, 'pressed', False)
    case.activate(custom, 'toggle button')
    case.expect_entry()
    case.key('enter')
    case.expect_state(custom, 'pressed')
    case.desktop.pointer(target['x'] + 65, target['y'] + 50, target['screen'], True)
    case.desktop.pointer(target['x'] + 65, target['y'] + 50, target['screen'], False)
    case.expect_entry()
    case.type('iiii WWW')
    case.activate('Add Text', 'button')
    case.expect('1 selected')
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    before = Image.open(io.BytesIO(base64.b64decode(case.desktop.call('clipboard', mime='image/png')))).convert('RGB')
    case.key('1')
    case.activate('Font family', 'button')
    case.expect_state('Search fonts', 'focused')
    case.type('DejaVu Sans Mono')
    case.expect('DejaVu Sans Mono')
    case.key('enter')
    case.expect('Search fonts', present=False)
    expect_style_value(case, 'Font family', 'DejaVu Sans Mono')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    after = Image.open(io.BytesIO(case.desktop.read(case.path('font.png')))).convert('RGB')
    assert ImageChops.difference(before, original).getbbox()
    assert ImageChops.difference(after, original).getbbox()
    glyph_changes = ImageChops.difference(after, before).getbbox()
    assert glyph_changes and glyph_changes[0] >= 65 and glyph_changes[1] >= 50
    case.checkpoint('font-family')


def test_I24_menu_opens_folder_as_gallery(case):
    case.sample_image('photo10.png')
    case.sample_image('photo2.png')
    case.launch('images', case.path('photo10.png'))
    case.expect('photo10.png', 'image')
    case.activate('Application Menu')
    case.activate('Open Folder')
    eventually(case.tree, lambda t: not any(n['role'] == 'frame' and 'active' in n['states'] for n in t),
               'system directory chooser has focus')
    case.key('enter')
    case.expect('photo2.png', 'image')
    case.key('right')
    case.expect('photo10.png', 'image')


def test_I25_number_marker_size_follows_presets_input_and_resize(case):
    original = case.sample_image('markers.png', (320, 200)).convert('RGB')
    case.launch('images', case.path('markers.png'))
    target = case.image_target(original)
    case.key('n')
    custom = 'Diameter · Custom'
    expect_style_value(case, custom, '32 px')
    case.activate('Diameter 48', 'toggle button')
    case.drag_image(target, (80, 75), (80, 75))
    case.expect_state('Diameter 48', 'pressed')
    expect_style_value(case, custom, '32 px')

    def set_diameter(value):
        case.activate(custom, 'toggle button')
        case.expect_state('Diameter', 'focused')
        case.type(value)
        case.key('enter')
        case.expect('Diameter', 'entry', present=False)
        expect_style_value(case, custom, f'{value} px')

    set_diameter('36')
    case.drag_image(target, (205, 75), (205, 75))
    expect_style_value(case, custom, '36 px')
    case.activate('Diameter 24', 'toggle button')
    case.key('ctrl+a')
    expect_style_value(case, custom, '32 px')
    case.expect_state('Diameter 24', 'pressed', False)
    set_diameter('40')
    expect_style_value(case, custom, '40 px')
    case.key('ctrl+z')
    expect_style_value(case, custom, '40 px')
    case.expect_state(custom, 'pressed', False)
    case.key('ctrl+shift+z')
    expect_style_value(case, custom, '40 px')

    case.key('1')
    case.drag_image(target, (20, 20), (20, 20))
    case.drag_image(target, (80, 75), (80, 75))
    case.expect('1 selected')
    case.drag_image(target, (100, 95), (120, 115))
    expect_style_value(case, custom, '60 px')
    case.checkpoint('marker-size')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    saved = Image.open(io.BytesIO(case.desktop.read(case.path('markers.png')))).convert('RGB')
    changed = ImageChops.difference(saved, original)
    left = changed.crop((0, 0, 160, 200)).getbbox()
    right = changed.crop((160, 0, 320, 200)).getbbox()
    assert left and 59 <= left[2] - left[0] <= 61 and 59 <= left[3] - left[1] <= 61
    assert right and 39 <= right[2] - right[0] <= 41 and 39 <= right[3] - right[1] <= 41

def test_I26_drawing_over_objects_creates_new_annotations(case):
    original = case.sample_image('overlap.png', (320, 200)).convert('RGB')
    case.launch('images', case.path('overlap.png'))
    target = case.image_target(original)
    case.key('4')
    case.drag_image(target, (50, 40), (140, 110))
    # Start at the selected rectangle's corner while the drawing tool remains active.
    case.drag_image(target, (140, 40), (245, 135))
    case.key('ctrl+a')
    case.expect('2 selected')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    saved = Image.open(io.BytesIO(case.desktop.read(case.path('overlap.png')))).convert('RGB')
    assert saved.getpixel((50, 75)) == saved.getpixel((245, 85)) == (239, 68, 68)
    assert saved.getpixel((90, 75)) == original.getpixel((90, 75))


def test_I27_settings_layout_matches_files(case):
    def bounds(name, role):
        # Native window decorations can offset the whole client by one pixel.
        left, top, _, _ = case.expect('Settings', 'landmark')[0]['bounds']
        x, y, width, height = case.expect(name, role)[0]['bounds']
        return x - left, y - top, width, height

    def maximize():
        if any(n['name'] == 'Maximize window' and 'showing' in n['states'] for n in case.tree()):
            case.activate('Maximize window', 'button')
        case.expect('Restore window', 'button')

    def appearance_controls():
        return {
            'sidebar': bounds('Settings', 'landmark'),
            'heading': bounds('Appearance', 'heading'),
            'source': bounds('Theme source', 'combo box'),
            'mode': bounds('Color mode', 'combo box'),
            'corners': bounds('Rounded window corners', 'toggle button'),
            'opacity': bounds('Main background opacity', 'slider'),
        }

    case.sample_image('layout.png')
    case.launch('files', case.work)
    maximize()
    case.key('ctrl+comma')
    files_first = bounds('General', 'button')
    files_heading = bounds('General', 'heading')
    case.checkpoint('files-general')
    case.activate('Appearance', 'button')
    files = appearance_controls()
    case.checkpoint('files-appearance')
    case.quit()

    case.launch('images', case.path('layout.png'))
    maximize()
    case.key('ctrl+comma')
    assert bounds('Appearance', 'button') == files_first
    assert appearance_controls() == files
    case.checkpoint('images-appearance')
    case.activate('Export & Privacy', 'button')
    assert bounds('Export & Privacy', 'heading')[:2] == files_heading[:2]
    case.checkpoint('images-export')


def test_I28_brush_freehand_stroke_and_style(case):
    original = case.sample_image('brush.png', (320, 200))
    case.launch('images', case.path('brush.png'))
    case.expect('brush.png', 'image')
    target = case.image_target(original)
    case.key('b')
    case.expect_state('Brush', 'pressed')
    expect_style_value(case, 'Stroke · Custom', '3 px')
    case.activate('#22C55E', 'toggle button')
    case.activate('Stroke 6', 'toggle button')
    case.activate('Copy Image', focus=True)
    case.expect_state('Copy Image', 'focused')
    x, y = target['x'], target['y']
    path = [(40, 40), (70, 95), (120, 140), (185, 105), (240, 55)]
    case.desktop.pointer(x + path[0][0], y + path[0][1], target['screen'])
    case.desktop.pointer(x + path[0][0], y + path[0][1], target['screen'], True)
    case.expect_state('brush.png', 'focused')
    for px, py in path[1:]:
        case.desktop.pointer(x + px, y + py, target['screen'])
    case.desktop.pointer(x + path[-1][0], y + path[-1][1], target['screen'], False)
    case.expect('1 selected')
    case.checkpoint('brush-stroke')
    # Single click without dragging stamps a round brush dot.
    case.desktop.pointer(x + 275, y + 155, target['screen'])
    case.desktop.pointer(x + 275, y + 155, target['screen'], True)
    case.desktop.pointer(x + 275, y + 155, target['screen'], False)
    case.key('ctrl+c')
    case.expect('Copied image to clipboard')
    with case.step('Verify clipboard contains both freehand stroke and click dot'):
        copied = Image.open(io.BytesIO(base64.b64decode(
            case.desktop.call('clipboard', mime='image/png')))).convert('RGB')
        changes = ImageChops.difference(copied, original.convert('RGB'))
        assert copied.getpixel((40, 40)) == (34, 197, 94)
        assert copied.getpixel((240, 55)) == (34, 197, 94)
        assert copied.getpixel((275, 155)) == (34, 197, 94)
        assert changes.crop((110, 120, 130, 145)).getbbox() is not None
        assert changes.crop((130, 30, 160, 55)).getbbox() is None
    case.key('ctrl+z')
    case.key('ctrl+s')
    case.expect(contains='Saved ', role='notification')
    with case.step('Verify undo removed only the dot before save'):
        saved = Image.open(io.BytesIO(case.desktop.read(case.path('brush.png')))).convert('RGB')
        assert saved.getpixel((275, 155)) == original.convert('RGB').getpixel((275, 155))
        assert saved.getpixel((40, 40)) == (34, 197, 94)
        assert saved.getpixel((240, 55)) == (34, 197, 94)
