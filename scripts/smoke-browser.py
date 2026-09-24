#!/usr/bin/env python3
"""Real Chromium acceptance against the production bundle; no mocked renderer.

Prerequisites: build web/, pip install playwright==1.57.0, then
python -m playwright install --with-deps chromium.
Run from the repository root: python scripts/smoke-browser.py
"""
from __future__ import annotations

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
KEY = "mmorpg.offline-demo.v1.aelric-stormward"
ROSTER_KEY = "mmorpg.offline-roster.v1"


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

    def open(self):
        self.page.goto(self.url)
        self.page.wait_for_function("window.__calls.draws > 0")
        expect(self.page.locator("#character-stage-fallback")).to_be_hidden()
        expect(self.page.get_by_role("slider", name="Character rotation")).to_be_visible()
        self.frames()

    def frames(self, count=3):
        self.page.evaluate("""count => new Promise(resolve => {
          function next() { if (--count <= 0) resolve(); else requestAnimationFrame(next); }
          requestAnimationFrame(next);
        })""", count)

    def export(self):
        with self.page.expect_download() as info:
            self.page.get_by_role("button", name="Export save", exact=True).filter(visible=True).click()
        return json.loads(Path(info.value.path()).read_text())

    def import_raw(self, raw: str):
        self.page.locator("[data-game-save-controls]:visible input[type=file]").set_input_files({
            "name": "save.json", "mimeType": "application/json", "buffer": raw.encode(),
        })

    def save(self):
        self.page.get_by_role("button", name="Save game", exact=True).filter(visible=True).click()
        expect(self.page.locator("[data-game-save-controls]:visible .game-save-status")).to_contain_text("Game saved")

    def checkpoint(self):
        return self.page.evaluate("key => localStorage.getItem(key)", KEY)

    def test_rotation_mouse_keyboard_reset_and_rendered_pixels(self):
        self.open()
        surface = self.page.get_by_role("slider", name="Character rotation")
        expect(self.page.get_by_role("button", name="Load game", exact=True).filter(visible=True)).to_be_disabled()
        before_save = self.export()
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
        self.assertEqual(self.export(), before_save, "Inspection cannot mutate gameplay, tick, or facing")
        self.assertIsNone(self.checkpoint(), "Inspect/export must not create a local save")
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
        self.page.get_by_role("button", name="Save game", exact=True).filter(visible=True).press("Enter")
        expect(self.page.locator("#character-select")).to_be_visible()
        expect(surface).to_have_attribute("aria-valuenow", "0")

    def test_save_reload_resume_import_and_paused_selection(self):
        self.open()
        self.page.locator('[data-hat-style="ironcrest-helm"]').click()
        self.page.get_by_role("button", name="Enter World").click()
        # Exercise actual movement; the fixture import below then reaches the waystone deterministically.
        self.page.keyboard.down("KeyD")
        self.frames(15)
        self.page.keyboard.up("KeyD")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        moved = self.export()
        self.assertGreater(moved["progress"]["position"]["x"], -5.5)
        self.frames(15)
        self.assertEqual(self.export(), moved, "Character selection must pause the simulation")
        moved["progress"].update({"position": {"x": 4.8, "z": -3.5}, "waystoneActive": False})
        self.import_raw(json.dumps(moved))
        expect(self.page.locator("#character-select")).to_be_hidden()
        self.page.keyboard.press("KeyE")
        expect(self.page.locator("#objective")).to_contain_text("Waystone activated")
        self.save()
        saved = self.checkpoint()
        self.page.keyboard.press("Escape")  # Save control owns keys; focus the world first.
        self.page.locator("#world").focus()
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#enter-world-label")).to_have_text("Resume exploration")
        expect(self.page.locator("#character-select .game-save-summary")).to_contain_text("Waystone active")
        self.assertEqual(self.checkpoint(), saved)
        self.page.reload()
        self.page.wait_for_function("window.__calls.draws > 0")
        expect(self.page.locator("#character-select .game-save-summary")).to_contain_text("Ironcrest Helm")
        self.page.get_by_role("button", name="Load game", exact=True).filter(visible=True).click()
        expect(self.page.locator("#objective")).to_contain_text("Waystone activated")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        restored = self.export()
        for field in ["position", "facing", "waystoneActive", "appearance"]:
            self.assertEqual(restored["progress"][field], json.loads(saved)["progress"][field])
        self.assertEqual(self.checkpoint(), saved)
        self.page.screenshot(path=str(ARTIFACTS / "saved-character-selection.png"))
        self.import_raw("{")
        expect(self.page.locator("#character-select .game-save-status")).to_contain_text("not valid JSON")
        self.assertEqual(self.export(), restored)
        self.assertEqual(self.checkpoint(), saved)

    def test_navigation_fences_delayed_import(self):
        self.open()
        imported = self.export()
        imported["progress"]["waystoneActive"] = True
        self.page.get_by_role("button", name="Enter World").click()
        self.page.evaluate("""() => {
          const original = File.prototype.text;
          File.prototype.text = function() { return new Promise(resolve => {
            window.__finishImport = async () => resolve(await original.call(this));
          }); };
        }""")
        self.import_raw(json.dumps(imported))
        expect(self.page.locator(".hud .game-save-status")).to_contain_text("Reading")
        self.page.get_by_role("button", name="Characters", exact=True).click()
        self.page.evaluate("() => window.__finishImport()")
        self.frames()
        expect(self.page.locator("#character-select")).to_be_visible()
        self.assertFalse(self.export()["progress"]["waystoneActive"], "Navigation alone must retire the pending import")
        self.assertIsNone(self.checkpoint())

    def test_storage_denial_keeps_file_backups_working(self):
        self.context.add_init_script("Object.defineProperty(window, 'localStorage', {get() { throw new DOMException('Storage denied', 'SecurityError'); }});")
        self.open()
        expect(self.page.locator("#character-select .game-save-summary")).to_contain_text("unavailable")
        expect(self.page.get_by_role("button", name="Load game", exact=True).filter(visible=True)).to_be_disabled()
        before = self.export()
        self.page.get_by_role("button", name="Save game", exact=True).filter(visible=True).click()
        expect(self.page.locator("#character-select .game-save-status")).to_contain_text("Storage denied")
        self.assertEqual(self.export(), before)
        self.import_raw(json.dumps(before))
        expect(self.page.locator("#character-select")).to_be_hidden()
        expect(self.page.locator(".hud .game-save-status")).to_contain_text("Imported game restored")

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
        self.page.get_by_role("button", name="Save game", exact=True).filter(visible=True).scroll_into_view_if_needed()
        self.save()
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile-saves.png"))
        expect(self.page.locator("#character-select .game-save-summary")).to_contain_text("Local checkpoint")
        self.assert_selection_layout()
        self.page.locator(".selection-actions").scroll_into_view_if_needed()
        self.frames()
        self.page.screenshot(path=str(ARTIFACTS / "character-selection-mobile-resume.png"))
        session.detach()

    def test_character_creation_classes_sex_roster_persistence_and_save_isolation(self):
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

        expect(self.page.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        roster = self.page.evaluate("key => JSON.parse(localStorage.getItem(key))", ROSTER_KEY)
        self.assertEqual(roster["characters"], [{
            "id": "local-1", "name": "Lyra Vale", "classId": "ranger", "sex": "female",
        }])

        self.page.get_by_role("button", name="Enter World").click()
        self.frames(8)
        self.page.screenshot(path=str(ARTIFACTS / "character-ranger-world.png"))
        self.page.locator("#world").focus()
        self.page.keyboard.press("Escape")
        expect(self.page.locator("#character-name")).to_have_text("Lyra Vale")
        self.save()
        self.assertIsNotNone(self.page.evaluate("() => localStorage.getItem('mmorpg.offline-demo.v1.local-1')"))
        self.assertIsNone(self.checkpoint(), "A created character must not write the built-in character save slot")
        self.page.get_by_role("button", name=re.compile("Aelric Stormward")).click()
        expect(self.page.get_by_role("button", name="Load game", exact=True).filter(visible=True)).to_be_disabled()
        self.page.get_by_role("button", name=re.compile("Lyra Vale")).click()
        expect(self.page.get_by_role("button", name="Load game", exact=True).filter(visible=True)).to_be_enabled()

        self.page.reload()
        self.page.wait_for_function("window.__calls.draws > 0")
        expect(self.page.get_by_role("button", name=re.compile("Lyra Vale"))).to_be_visible()
        self.page.get_by_role("button", name="Create character", exact=True).click()
        self.page.get_by_label("Name").fill("Dorian Voss")
        self.page.get_by_role("radio", name=re.compile("^Arcanist")).check()
        expect(self.page.get_by_role("radio", name="Male", exact=True)).to_be_checked()
        expect(self.page.locator("#character-subtitle")).to_contain_text("Male Human Arcanist")
        expect(self.page.locator("#equipment-list")).to_contain_text("Emberglass Staff")
        self.page.get_by_role("button", name="Create character", exact=True).filter(visible=True).click()
        expect(self.page.get_by_role("button", name=re.compile("Dorian Voss"))).to_be_visible()
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
