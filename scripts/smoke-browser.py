#!/usr/bin/env python3
"""Real Chromium acceptance against the production bundle; no mocked renderer or simulation.

The bundle embeds the shared Rust zone simulation as a WASM local host, so these
tests drive real movement, jumps and zone entry/exit through the page.

Prerequisites: build web/ (bun run build), pip install playwright==1.57.0, then
python -m playwright install --with-deps chromium.
Run from the repository root: python scripts/smoke-browser.py
"""
from __future__ import annotations

import base64
import functools
import http.server
import json
import re
from pathlib import Path
import threading
import unittest

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts" / "browser"
ROSTER_KEY = "mmorpg.offline-roster.v1"
# The retired offline checkpoint key; nothing may write it any more.
LEGACY_CHECKPOINT_PREFIX = "mmorpg.offline-demo.v1."


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/mmorpg/"):
            self.path = self.path[len("/mmorpg"):]
        super().do_GET()

    def log_message(self, *_args):
        pass


class BrowserAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        dist = ROOT / "web" / "dist"
        if not (dist / "index.html").is_file():
            raise RuntimeError("Build the production web bundle before browser acceptance.")
        ARTIFACTS.mkdir(parents=True, exist_ok=True)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(dist)))
        cls.server_thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.server_thread.start()
        cls.url = f"http://127.0.0.1:{cls.server.server_port}/mmorpg/"
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
        cls.evidence = []

    @classmethod
    def tearDownClass(cls):
        (ARTIFACTS / "browser-evidence.json").write_text(json.dumps(cls.evidence, indent=2))
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.server_thread.join()

    def setUp(self):
        self.errors = []
        self.context = self.browser.new_context(viewport={"width": 1440, "height": 900}, reduced_motion="reduce")
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.context.add_init_script("""(() => {
          window.__calls = {reads: 0, writes: 0, draws: 0};
          for (const [name, counter] of [['getItem', 'reads'], ['setItem', 'writes']]) {
            const original = Storage.prototype[name];
            Storage.prototype[name] = function(...args) { window.__calls[counter]++; return original.apply(this, args); };
          }
          for (const Type of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
            if (!Type) continue;
            for (const name of ['drawArrays', 'drawElements', 'drawArraysInstanced', 'drawElementsInstanced']) {
              const original = Type.prototype[name];
              if (!original) continue;
              Type.prototype[name] = function(...args) { window.__calls.draws++; return original.apply(this, args); };
            }
          }
        })();""")

    def tearDown(self):
        self.page.screenshot(path=str(ARTIFACTS / f"{self._testMethodName}.png"))
        self.evidence.append({"test": self._testMethodName, "pageErrors": self.errors})
        self.context.close()
        self.assertEqual(self.errors, [], "Browser application raised an uncaught error")

    def open(self, query=""):
        self.page.goto(self.url + query)
        self.page.wait_for_function("window.__calls.draws > 0")
        expect(self.page.locator("#character-stage-fallback")).to_be_hidden()
        expect(self.page.get_by_role("slider", name="Character rotation")).to_be_visible()
        self.wait_for_zone()
        self.frames()

    def wait_for_zone(self):
        """The WASM local zone host loads asynchronously; entry is disabled until it is ready."""
        expect(self.enter_button()).to_be_enabled(timeout=20_000)
        expect(self.page.locator("#enter-world-note")).to_contain_text("Start ")

    def enter_button(self):
        return self.page.get_by_role("button", name=re.compile("^Enter World"))

    def enter_world(self):
        self.enter_button().click()
        expect(self.page.locator("#character-select")).to_be_hidden()
        expect(self.page.locator(".hud")).to_be_visible()
        self.page.locator("#world").focus()
        self.frames(4)

    def world_view(self):
        """The world canvas between the HUD panels; the camera follows the character."""
        return self.page.screenshot(clip={"x": 200, "y": 120, "width": 1040, "height": 560})

    def wait_for_view(self, expected, timeout_frames=240):
        for _ in range(timeout_frames):
            self.frames(2)
            if self.world_view() == expected:
                return True
        return False

    def legacy_checkpoints(self):
        return self.page.evaluate("prefix => Object.keys(localStorage).filter(key => key.startsWith(prefix))", LEGACY_CHECKPOINT_PREFIX)

    def frames(self, count=3):
        self.page.evaluate("""count => new Promise(resolve => {
          function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
          requestAnimationFrame(next);
        })""", count)

    def test_rotation_mouse_keyboard_reset_and_rendered_pixels(self):
        self.open()
        surface = self.page.get_by_role("slider", name="Character rotation")
        # Reach the front view the way the reset check below does (a mouse click, then
        # focus), so both screenshots share the same :focus-visible state.
        self.page.get_by_role("button", name="Reset view").click()
        surface.focus()
        box = surface.bounding_box()
        clip = {"x": box["x"] + 5, "y": box["y"] + 5, "width": box["width"] - 10, "height": box["height"] - 10}
        self.frames()
        front = self.page.screenshot(clip=clip)
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-front.png"))
        x, y = box["x"] + box["width"] / 3, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + box["width"] / 4, y, steps=8)
        self.page.mouse.up()
        expect(surface).to_have_attribute("aria-valuenow", "90")
        self.frames()
        self.assertNotEqual(self.page.screenshot(clip=clip), front, "Changing yaw must rotate the rendered 3D model, not only ARIA state")
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-rotated.png"))
        surface.press("ArrowRight")
        expect(surface).to_have_attribute("aria-valuenow", "105")
        surface.press("Home")
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.get_by_role("button", name="Rotate character left").click()
        expect(surface).to_have_attribute("aria-valuenow", "345")
        self.page.get_by_role("button", name="Reset view").click()
        surface.focus()
        self.frames()
        self.assertEqual(self.page.screenshot(clip=clip), front, "Reset restores the rendered front view")
        reads = self.page.evaluate("window.__calls.reads")
        writes = self.page.evaluate("window.__calls.writes")
        self.frames(15)
        self.assertEqual(self.page.evaluate("window.__calls.reads"), reads)
        self.assertEqual(self.page.evaluate("window.__calls.writes"), writes)

    def test_pointer_cancel_capture_loss_and_control_isolation(self):
        self.open()
        surface = self.page.locator("#preview-surface")
        surface.evaluate("el => el.addEventListener('pointerdown', e => window.__pointerId = e.pointerId)")
        box = surface.bounding_box()
        x, y = box["x"] + 30, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 80, y)
        angle = surface.get_attribute("aria-valuenow")
        self.assertNotEqual(angle, "0")
        surface.dispatch_event("pointercancel", {"pointerId": 99})
        expect(surface).to_have_attribute("aria-valuenow", angle)
        surface.evaluate("el => el.releasePointerCapture(window.__pointerId)")
        # Capture changes are processed before the next pointer event, not on rAF.
        # https://www.w3.org/TR/pointerevents3/#process-pending-pointer-capture
        self.page.mouse.move(x + 81, y)
        self.frames()
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.mouse.up()
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 80, y)
        pointer_id = self.page.evaluate("window.__pointerId")
        surface.dispatch_event("pointercancel", {"pointerId": pointer_id})
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.mouse.up()
        self.page.locator('[data-hat-style="ranger-cap"]').press("Enter")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(surface).to_have_attribute("aria-valuenow", "0")
        self.page.get_by_role("button", name="Reset view").press("Enter")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(surface).to_have_attribute("aria-valuenow", "0")

    def test_enter_world_move_jump_orbit_and_leave(self):
        self.open()
        self.assertEqual(self.page.get_by_role("button", name=re.compile("^(Save|Load|Export|Import) (game|save)$")).count(), 0,
                         "No control may claim to save world progress the demo cannot persist")
        self.assertNotRegex(self.page.locator("body").text_content(), re.compile(r"saved (facing|position)", re.IGNORECASE),
                            "No text may claim world state is saved; the demo cannot persist it")
        self.page.locator('[data-hat-style="ironcrest-helm"]').click()
        writes = self.page.evaluate("window.__calls.writes")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        expect(self.page.locator("#objective")).to_contain_text("Explore Greyhaven Vale")
        expect(self.page.locator("#objective")).to_contain_text("Greyhaven Outpost")
        self.frames(6)
        spawn = self.world_view()
        self.page.screenshot(path=str(ARTIFACTS / "world-spawn.png"))
        self.frames(6)
        self.assertEqual(self.world_view(), spawn, "An idle character in a static zone renders identically")

        # W runs forward along the camera heading through the WASM zone; the camera follows.
        self.page.keyboard.down("KeyW")
        self.frames(24)
        self.page.keyboard.up("KeyW")
        self.frames(4)
        moved = self.world_view()
        self.assertNotEqual(moved, spawn, "Holding W must move the character and its follow camera")
        self.page.screenshot(path=str(ARTIFACTS / "world-moved.png"))

        # Space jumps: body and camera rise, then physics lands the unit where it took off.
        self.page.keyboard.press("Space")
        self.frames(3)
        self.assertNotEqual(self.world_view(), moved, "A grounded jump must lift the character")
        self.page.screenshot(path=str(ARTIFACTS / "world-jump.png"))
        self.assertTrue(self.wait_for_view(moved), "The character lands back on the ground it jumped from")

        # Dragging the canvas orbits the camera; the wheel zooms.
        box = self.page.locator("#world").bounding_box()
        x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 180, y - 40, steps=6)
        self.page.mouse.up()
        self.page.mouse.wheel(0, -240)
        self.frames(4)
        self.assertNotEqual(self.world_view(), moved, "Orbit and zoom must change the view")
        self.page.screenshot(path=str(ARTIFACTS / "world-orbit.png"))
        self.assertEqual(self.page.evaluate("window.__calls.writes"), writes, "Playing never writes browser storage")

        # Leaving removes the unit from the local zone; re-entry spawns a fresh character at the spawn.
        self.page.get_by_role("button", name="Characters", exact=True).click()
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.enter_button()).to_be_enabled()
        self.enter_world()
        self.assertTrue(self.wait_for_view(spawn, timeout_frames=20),
                        "Re-entering must show a new character alone at the spawn with a reset camera")
        self.page.locator("#world").focus()
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-select")).to_be_visible()
        self.assertEqual(self.legacy_checkpoints(), [])

    def test_rejected_projections_fail_closed_without_freezing_the_page(self):
        # Byte 2 of a snapshot header is its schema version. While the flag is set the
        # strict decoder sees an unknown version, as after a WASM/decoder version skew.
        self.context.add_init_script("""(() => {
          window.__rejectSnapshots = false;
          const original = DataView.prototype.getUint16;
          DataView.prototype.getUint16 = function(offset, ...rest) {
            return window.__rejectSnapshots && offset === 2 ? 0xffff : original.call(this, offset, ...rest);
          };
        })();""")
        self.open()
        note = self.page.locator("#enter-world-note")

        # A refused entry keeps the page on selection, says why, and keeps rendering.
        self.page.evaluate("window.__rejectSnapshots = true")
        self.enter_button().click()
        expect(note).to_contain_text("Could not enter the world: Unsupported snapshot version")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.page.locator(".hud")).to_be_hidden()
        self.assert_rendering()

        # The refused join left nothing joined, so entry works once projections decode.
        self.page.evaluate("window.__rejectSnapshots = false")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")

        # A projection rejected while playing leaves the world instead of stopping the loop.
        self.page.evaluate("window.__rejectSnapshots = true")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(self.page.locator(".hud")).to_be_hidden()
        expect(note).to_contain_text("Left the world after an error: Unsupported snapshot version")
        self.page.evaluate("window.__rejectSnapshots = false")
        self.assert_rendering()
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        self.page.keyboard.press("Escape")
        expect(note).to_contain_text("Start ")

    def colour_stats(self, png: bytes):
        """Distinct RGB colours in a screenshot and the share of pixels that differ from the page
        background, counted in the page (no image library needed)."""
        return self.page.evaluate("""async data => {
          const image = new Image();
          image.src = 'data:image/png;base64,' + data;
          await image.decode();
          const canvas = document.createElement('canvas');
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext('2d');
          context.drawImage(image, 0, 0);
          const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
          const background = getComputedStyle(document.body).backgroundColor.match(/\\d+/g).map(Number);
          const colours = new Set();
          let samples = 0;
          let covered = 0;
          for (let i = 0; i < pixels.length; i += 4 * 5) {
            const rgb = [pixels[i], pixels[i + 1], pixels[i + 2]];
            colours.add((rgb[0] << 16) | (rgb[1] << 8) | rgb[2]);
            samples += 1;
            if (rgb.some((channel, index) => Math.abs(channel - background[index]) > 3)) covered += 1;
          }
          return {distinct: colours.size, covered: covered / samples};
        }""", base64.b64encode(png).decode())

    def canvas_colours(self, hide_canvas=False):
        """Colour statistics of what the renderer drew in the world clip. The canvas is
        transparent over the CSS sky, and the minimap, HUD and F3 overlay sit on top of it;
        with those hidden, only rendered pixels differ from the plain page background."""
        layers = "#sky, #debug-overlay, [data-world-ui]" + (", #world" if hide_canvas else "")
        self.page.evaluate("layers => document.querySelectorAll(layers).forEach(e => { e.style.visibility = 'hidden'; })", layers)
        try:
            return self.colour_stats(self.world_view())
        finally:
            self.page.evaluate("layers => document.querySelectorAll(layers).forEach(e => { e.style.visibility = ''; })", layers)

    def debug_stats(self):
        return self.page.evaluate("window.__valeDebug.stats()")

    def test_vale_viewpoints_overlay_minimap_and_mouse_look(self):
        """Debug-only camera viewpoints (never the simulation) show the subzones; F3 reports scene work."""
        self.open("?debug")
        self.enter_world()
        self.frames(6)
        spawn = self.debug_stats()["self"]
        overlay = self.page.get_by_role("region", name="Debug statistics")
        expect(overlay).to_be_hidden()
        self.page.keyboard.press("F3")
        expect(overlay).to_be_visible()
        minimap = self.page.get_by_role("complementary", name="Minimap")
        expect(minimap).to_be_visible()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        # Control: without the canvas the measurement sees only the plain page background.
        self.assertEqual(self.canvas_colours(hide_canvas=True), {"distinct": 1, "covered": 0})
        viewpoints = {}
        for name in ["hub", "woods", "hollow"]:
            self.page.evaluate("name => window.__valeDebug.flyTo(name)", name)
            self.frames(8)
            self.page.screenshot(path=str(ARTIFACTS / f"viewpoint-{name}.png"))
            colours = self.canvas_colours()
            self.assertGreater(colours["distinct"], 400, f"The {name} viewpoint must render a varied scene, not a uniform canvas")
            self.assertGreater(colours["covered"], 0.5, f"The {name} viewpoint must draw most of the view, not leave the canvas empty")
            viewpoints[name] = {"canvasColours": colours, "frame": self.debug_stats()["frame"]}
        self.frames(12)
        stats = self.debug_stats()
        overlay_stats = json.loads(overlay.get_attribute("data-stats"))
        self.assertGreater(overlay_stats["nodes"], overlay_stats["staticNodes"])
        self.assertLessEqual(overlay_stats["staticNodes"], 600, "Static batching keeps draw calls bounded")
        self.assertGreater(overlay_stats["fps"], 0)
        (ARTIFACTS / "scene-stats.json").write_text(json.dumps({
            "note": "fps comes from headless Chromium with SwiftShader software rendering; it is not a hardware measurement",
            "overlay": overlay_stats,
            "buildMs": stats["buildMs"],
            "scene": stats["scene"],
            "viewpoints": viewpoints,
        }, indent=2))
        # The viewpoints moved only the camera: the character never left its spawn.
        self.assertEqual(stats["self"], spawn)
        self.page.evaluate("window.__valeDebug.follow()")

        # Minimap zoom buttons step through their range; clicking them leaves the keyboard
        # with the world, so movement, jumps and F3 keep working without clicking the canvas.
        zoom_out = self.page.get_by_role("button", name="Zoom minimap out")
        for _ in range(4):
            if zoom_out.is_enabled():
                zoom_out.click()
        expect(zoom_out).to_be_disabled()
        north_up = self.page.get_by_role("button", name="Keep north up")
        north_up.click()
        expect(north_up).to_have_attribute("aria-pressed", "true")
        self.page.keyboard.down("KeyW")
        self.frames(12)
        self.page.keyboard.up("KeyW")
        self.frames(4)
        ran = self.debug_stats()["self"]
        self.assertNotEqual((ran["x"], ran["z"]), (spawn["x"], spawn["z"]), "W runs right after a minimap click")
        self.page.keyboard.press("Space")
        self.frames(2)
        expect(north_up).to_have_attribute("aria-pressed", "true")
        self.page.keyboard.press("F3")
        expect(overlay).to_be_hidden()
        self.page.keyboard.press("F3")
        expect(overlay).to_be_visible()

        # A left drag looks around without turning the character; a right drag turns it.
        box = self.page.locator("#world").bounding_box()
        x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
        self.page.mouse.move(x, y)
        self.page.mouse.down()
        self.page.mouse.move(x + 200, y, steps=5)
        self.page.mouse.up()
        self.frames(8)
        self.assertEqual(self.debug_stats()["self"]["facing"], ran["facing"], "Left-drag orbit leaves the facing alone")
        self.page.mouse.move(x, y)
        self.page.mouse.down(button="right")
        self.page.mouse.move(x + 200, y, steps=5)
        self.page.mouse.up(button="right")
        self.assertTrue(self.wait_for_facing_change(ran["facing"]), "Right-drag mouse-look turns the character")
        self.page.screenshot(path=str(ARTIFACTS / "world-mouse-look.png"))
        self.page.keyboard.press("F3")
        expect(overlay).to_be_hidden()

    def wait_for_facing_change(self, facing, timeout_frames=60):
        for _ in range(timeout_frames):
            self.frames(2)
            if abs(self.debug_stats()["self"]["facing"] - facing) > 1e-3:
                return True
        return False

    def assert_rendering(self):
        draws = self.page.evaluate("window.__calls.draws")
        self.frames(3)
        self.assertGreater(self.page.evaluate("window.__calls.draws"), draws, "The render loop must keep running")

    def test_storage_denial_keeps_the_world_playable(self):
        self.context.add_init_script("Object.defineProperty(window, 'localStorage', {get() { throw new DOMException('Storage denied', 'SecurityError'); }});")
        self.open()
        expect(self.page.locator("#roster-status")).to_contain_text("could not be read")
        self.page.locator(".appearance-storage summary").click()
        self.page.get_by_role("button", name="Save appearance").click()
        expect(self.page.locator("#save-status")).to_contain_text("storage is unavailable")
        self.enter_world()
        expect(self.page.locator("#area-name")).to_have_text("Greyhaven Outpost")
        self.page.keyboard.down("KeyD")
        self.frames(10)
        self.page.keyboard.up("KeyD")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        expect(self.page.locator("#character-select")).to_be_visible()

    def test_mobile_touch_rotation_scroll_and_layout(self):
        self.context.close()
        self.context = self.browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True, device_scale_factor=1)
        self.page = self.context.new_page()
        self.page.on("pageerror", lambda error: self.errors.append(str(error)))
        self.page.goto(self.url)
        expect(self.page.locator("#character-stage-fallback")).to_be_hidden()
        surface = self.page.locator("#preview-surface")
        surface.scroll_into_view_if_needed()
        self.frames(10)
        box = surface.bounding_box()
        session = self.context.new_cdp_session(self.page)
        x, y = box["x"] + box["width"] / 3, box["y"] + box["height"] / 2
        session.send("Input.dispatchTouchEvent", {"type": "touchStart", "touchPoints": [{"x": x, "y": y, "id": 1}]})
        session.send("Input.dispatchTouchEvent", {"type": "touchMove", "touchPoints": [{"x": x + 70, "y": y, "id": 1}]})
        session.send("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
        self.assertNotEqual(surface.get_attribute("aria-valuenow"), "0", "A real touch drag must turn the character")
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile.png"))
        overflow = self.page.locator("#character-select").evaluate("el => el.scrollWidth > el.clientWidth + 1")
        self.assertFalse(overflow, "Mobile selection must not scroll horizontally")
        self.assert_selection_layout()
        self.page.locator(".selection-actions").scroll_into_view_if_needed()
        self.frames()
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile-resume.png"))
        session.detach()

    def test_character_creation_classes_sex_and_roster_persistence(self):
        self.open()
        self.page.get_by_role("button", name="Create character", exact=True).click()
        expect(self.page.get_by_role("heading", name="Create character")).to_be_visible()
        class_radios = self.page.get_by_role("group", name="Class").get_by_role("radio")
        self.assertEqual(class_radios.count(), 3)
        expect(self.page.get_by_role("radio", name=re.compile("^Warden"))).to_be_checked()
        expect(self.page.get_by_role("radio", name="Male", exact=True)).to_be_checked()

        self.page.get_by_label("Name").fill("Lyra Vale")
        self.page.get_by_role("radio", name=re.compile("^Ranger")).check()
        self.page.get_by_role("radio", name="Female").check()
        expect(self.page.locator("#character-subtitle")).to_contain_text("Female Human Ranger")
        expect(self.page.locator("#equipment-list")).to_contain_text("Ashwood Longbow")
        self.frames()
        self.page.screenshot(path=str(ARTIFACTS / "character-creation-female-ranger.png"))
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()

        roster_buttons = self.page.locator("#character-roster")
        expect(roster_buttons.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        roster = self.page.evaluate("key => JSON.parse(localStorage.getItem(key))", ROSTER_KEY)
        self.assertEqual(roster["characters"], [{
            "id": "local-1", "name": "Lyra Vale", "classId": "ranger", "sex": "female",
        }])

        self.enter_world()
        self.frames(8)
        self.page.screenshot(path=str(ARTIFACTS / "character-ranger-world.png"))
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        self.assertEqual(self.legacy_checkpoints(), [], "Entering the world must not write any progress save")
        roster_buttons.get_by_role("button", name=re.compile("Aelric Stormward")).click()
        expect(self.page.locator("#character-name")).to_have_text("Aelric Stormward")
        roster_buttons.get_by_role("button", name=re.compile("Lyra Vale")).click()

        self.page.reload()
        self.page.wait_for_function("window.__calls.draws > 0")
        roster_buttons = self.page.locator("#character-roster")
        expect(roster_buttons.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        self.page.get_by_role("button", name="Create character", exact=True).click()
        self.page.get_by_label("Name").fill("Dorian Voss")
        self.page.get_by_role("radio", name=re.compile("^Arcanist")).check()
        expect(self.page.get_by_role("radio", name="Male", exact=True)).to_be_checked()
        expect(self.page.locator("#character-subtitle")).to_contain_text("Male Human Arcanist")
        expect(self.page.locator("#equipment-list")).to_contain_text("Emberglass Staff")
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()
        expect(self.page.locator("#character-roster").get_by_role("button", name=re.compile("Dorian Voss"))).to_be_visible()
        self.page.screenshot(path=str(ARTIFACTS / "character-creation-roster.png"))

    def assert_selection_layout(self):
        selectors = [".selection-brand", "#preview-surface", ".turntable-controls", ".roster-panel", ".selection-actions", ".character-details"]
        boxes = {selector: self.page.locator(selector).bounding_box() for selector in selectors}
        (ARTIFACTS / f"{self._testMethodName}-layout.json").write_text(json.dumps(boxes, indent=2))
        for i, name in enumerate(selectors):
            a = boxes[name]
            for other in selectors[i + 1:]:
                b = boxes[other]
                overlap = min(a["x"] + a["width"], b["x"] + b["width"]) > max(a["x"], b["x"]) + 1 and min(a["y"] + a["height"], b["y"] + b["height"]) > max(a["y"], b["y"]) + 1
                self.assertFalse(overlap, f"Selection panels overlap: {name} {a}, {other} {b}")
        self.assertFalse(self.page.locator("#character-select").evaluate("el => el.scrollWidth > el.clientWidth + 1"))

    def test_selection_layout_at_narrow_short_and_tablet_sizes(self):
        self.open()
        for width, height in [(320, 568), (844, 390), (900, 700), (1280, 600)]:
            with self.subTest(width=width, height=height):
                self.page.set_viewport_size({"width": width, "height": height})
                self.frames()
                self.assert_selection_layout()
                self.page.screenshot(path=str(ARTIFACTS / f"selection-{width}x{height}.png"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
