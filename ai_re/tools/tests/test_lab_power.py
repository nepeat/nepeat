"""Exercise the HTTP contract without switching real hardware."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("lab_power", Path(__file__).parents[1] / "lab_power.py")
power = importlib.util.module_from_spec(spec)
spec.loader.exec_module(power)


class OutletTests(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self.state = "off"
        self.error = None
        self.stuck = False
        test = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def handle_request(self):
                data = json.loads(self.rfile.read(int(self.headers["Content-Length"]))) if self.command == "POST" else None
                test.calls.append((self.command, self.path, data, self.headers.get("Authorization")))
                if test.error:
                    self.send_response(test.error)
                    if test.error == 302:
                        self.send_header("Location", "/redirect-target")
                    self.end_headers()
                    self.wfile.write(b"private response body")
                    return
                if data is not None:
                    if not test.stuck:
                        test.state = self.path.rsplit("turn_", 1)[1]
                    response = []
                else:
                    response = {"entity_id": self.path.rsplit("/", 1)[1], "state": test.state}
                self.send_response(200)
                self.end_headers()
                self.wfile.write(json.dumps(response).encode())

            do_GET = handle_request
            do_POST = handle_request

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.env = patch.dict(os.environ, {
            "HA_BASE": f"http://127.0.0.1:{self.server.server_port}",
            "HA_KEY": "test-secret", "HA_ENTITY": "switch.test",
        }, clear=True)
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def run_cli(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = power.main(list(args))
        return code, out.getvalue(), err.getvalue()

    def test_status_is_read_only_and_authenticated(self):
        code, out, err = self.run_cli("status", "--json")
        self.assertEqual(code, 0, err)
        self.assertEqual(json.loads(out)["state"], "off")
        self.assertEqual(self.calls, [("GET", "/api/states/switch.test", None, "Bearer test-secret")])

    def test_on_off_target_and_verification(self):
        for action in ("on", "off"):
            self.calls.clear()
            code, out, err = self.run_cli(action, "--entity", "switch.override")
            self.assertEqual(code, 0, err)
            self.assertIn(f"switch.override: {action}", out)
            self.assertEqual([(c[0], c[1], c[2]) for c in self.calls], [
                ("GET", "/api/states/switch.override", None),
                ("POST", f"/api/services/switch/turn_{action}", {"entity_id": "switch.override"}),
                ("GET", "/api/states/switch.override", None),
            ])

    def test_http_errors_hide_private_details(self):
        for status in (401, 404, 500, 302):
            self.error = status
            code, out, err = self.run_cli("on")
            self.assertEqual(code, 1)
            self.assertIn(str(status), err)
            self.assertNotIn("test-secret", err)
            self.assertNotIn("private response", err)
        self.assertTrue(all(call[0] == "GET" for call in self.calls))
        self.assertEqual(len(self.calls), 4)

    def test_unavailable_prevents_switching(self):
        self.state = "unavailable"
        self.assertEqual(self.run_cli("status")[0], 0)
        self.assertEqual(self.run_cli("on")[0], 1)
        self.assertTrue(all(call[0] == "GET" for call in self.calls))

    def test_unconfirmed_switch_fails(self):
        self.stuck = True
        code, out, err = self.run_cli("on", "--wait", "0.01")
        self.assertEqual(code, 1)
        self.assertIn("did not report on", err)

    def test_missing_configuration(self):
        del os.environ["HA_KEY"]
        self.assertEqual(self.run_cli("status")[0], 1)
        self.assertEqual(self.calls, [])

    def test_invalid_target_fails_before_request(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            power.main(["off", "--entity", "switch.test/../../services"])
        self.assertEqual(self.calls, [])

    def test_default_entity(self):
        del os.environ["HA_ENTITY"]
        self.assertEqual(self.run_cli("status")[0], 0)
        self.assertEqual(self.calls[0][1], "/api/states/switch.server_room_lab_plug")


if __name__ == "__main__":
    unittest.main()
