#!/usr/bin/env python3
"""Read and switch a Home Assistant lab outlet using environment credentials.

The plug number is required on every call so nothing is switched by default.
"""

import argparse
import http.client
import json
import math
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


# Plug number -> HA entity. Which device is on each plug is recorded in
# tools/README.md.
PLUGS = {
    "1": "switch.server_room_lab_plug",
    "2": "switch.server_room_lab_plug_2",
}


class PowerError(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    # Never forward the bearer token to a redirect destination.
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class HomeAssistant:
    def __init__(self, base, key, timeout):
        try:
            parsed = urllib.parse.urlsplit(base)
        except ValueError:
            raise PowerError("HA_BASE must be a valid http(s) URL") from None
        if (parsed.scheme not in ("http", "https") or not parsed.hostname
                or parsed.username is not None or parsed.password is not None
                or parsed.query or parsed.fragment):
            raise PowerError("HA_BASE must be an http(s) URL without credentials, query, or fragment")
        if not key.strip() or "\n" in key or "\r" in key:
            raise PowerError("HA_KEY must contain a valid access token")
        self.base = base.rstrip("/")
        self.key = key
        self.timeout = timeout
        self.opener = urllib.request.build_opener(NoRedirect)

    def request(self, path, data=None):
        request = urllib.request.Request(
            self.base + "/api/" + path,
            data=json.dumps(data).encode() if data is not None else None,
            headers={"Authorization": "Bearer " + self.key,
                     "Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=self.timeout) as response:
                return json.load(response)
        except urllib.error.HTTPError as exc:
            hints = {401: "check HA_KEY", 403: "access denied", 404: "entity or endpoint not found"}
            raise PowerError(f"Home Assistant HTTP {exc.code}: "
                             + hints.get(exc.code, "request failed; check HA_BASE and server")) from None
        except (OSError, ValueError, http.client.HTTPException):
            # Raw exceptions/response bodies can contain credentials or private URLs.
            raise PowerError("Home Assistant connection failed or returned invalid JSON") from None

    def state(self, entity):
        result = self.request("states/" + entity)
        if not isinstance(result, dict) or not isinstance(result.get("state"), str):
            raise PowerError("Home Assistant returned an invalid entity state")
        return result


def positive_seconds(value):
    seconds = float(value)
    if not math.isfinite(seconds) or seconds <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return seconds


def main(argv=None):
    parser = argparse.ArgumentParser(prog="lab-power", description=__doc__)
    parser.add_argument("plug", choices=sorted(PLUGS), help="lab plug number (required)")
    parser.add_argument("action", choices=("status", "on", "off"))
    parser.add_argument("--json", action="store_true", help="print the full entity state as JSON")
    parser.add_argument("--timeout", type=positive_seconds, default=10,
                        help="timeout per HTTP request in seconds (default: 10)")
    parser.add_argument("--wait", type=positive_seconds, default=5,
                        help="seconds to wait for a requested state (default: 5)")
    args = parser.parse_args(argv)
    args.entity = PLUGS[args.plug]
    try:
        base, key = os.environ.get("HA_BASE"), os.environ.get("HA_KEY")
        if not base or not key:
            raise PowerError("set HA_BASE and HA_KEY in your environment (loaded by direnv)")
        client = HomeAssistant(base, key, args.timeout)
        state = client.state(args.entity)
        if args.action != "status":
            if state["state"] in ("unavailable", "unknown"):
                raise PowerError(f"{args.entity} is {state['state']}; cannot confirm control")
            client.request("services/switch/turn_" + args.action, {"entity_id": args.entity})
            deadline = time.monotonic() + args.wait
            while True:
                state = client.state(args.entity)
                if state["state"] == args.action:
                    break
                if time.monotonic() >= deadline:
                    raise PowerError(f"service accepted but {args.entity} did not report {args.action}; "
                                     f"last state: {state['state']}")
                time.sleep(min(0.25, max(0, deadline - time.monotonic())))
        print(json.dumps(state) if args.json else f"plug {args.plug} ({args.entity}): {state['state']}")
        return 0
    except PowerError as exc:
        print(f"lab-power: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
