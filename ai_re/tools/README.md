# RE toolkit helpers

`lab-power` reads or switches a Home Assistant power outlet. Enter the nix
devshell (reload direnv after pulling changes) to put it on PATH. The helper
uses Python's standard library; no pip install is needed.

Keep `HA_BASE` (the server URL, without `/api`) and `HA_KEY` (a long-lived access
token) in your local environment: put them in the untracked `.envrc.local`,
which `.envrc` sources through direnv. Plain
`nix develop` does not load `.envrc` exports; use direnv or export them yourself.
Never commit tokens or paste them into CLI arguments.

```sh
lab-power status
lab-power off
lab-power on
lab-power status --json
lab-power status --entity switch.another_outlet
lab-power on --entity switch.another_outlet
```

The default is `switch.server_room_lab_plug`. Set `HA_ENTITY` to change the
default, or use `--entity` for a single command (takes precedence over the
environment). Entity IDs must be in the `switch` domain.

`status` only reads. `on` and `off` first check the entity, call the switch
service, then poll until HA reports the requested state. Failure returns a
nonzero exit code; `unknown` and `unavailable` are shown as-is by `status`.
`--timeout` controls each request's timeout (default 10 seconds); `--wait`
controls the polling window (default 5 seconds, plus any in-flight request).
HA's reported state is not an independent measurement of physical mains power.

API reference: [Home Assistant REST API](https://developers.home-assistant.io/docs/api/rest/).

Run the isolated API tests without touching hardware:

```sh
python3 -m unittest discover -s tools/tests -v
```
