package ui

import (
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

// Drives menu → modal end to end in dry-run mode, including outlet-state
// gating: an all-on device must not offer "Power on", all-off no "Power off".
func TestPowerModalFlow(t *testing.T) {
	slots := loadFixture[netbox.ElevationSlot](t, "elevation_front.json")
	devices := loadFixture[netbox.Device](t, "devices_mdf.json")
	racks := loadFixture[netbox.Rack](t, "racks.json")

	cfg, _ := config.Load("")
	cfg.PDUs["dma-pdu-01"] = config.PDU{Driver: "none"}
	app := NewApp(cfg, "MDF", true) // dry-run
	app.client = netbox.New("http://example.invalid", "x")

	step := func(msg tea.Msg) tea.Cmd {
		m, cmd := app.Update(msg)
		app = m.(*App)
		return cmd
	}
	step(tea.WindowSizeMsg{Width: 150, Height: 45})
	step(racksMsg{Racks: racks, Roles: nil})
	step(rackDataMsg{RackID: 1, Front: slots, Rear: slots, Devices: devices})

	// Select dreamflasher and inject its power-port detail (PSU1 → dma-pdu-01/output6).
	app.focus = focusElevation
	app.devCursor = indexOfBlock(t, app.rackData[1].blocks("front"), "dreamflasher")
	app.details[15] = &deviceDetail{
		powerPorts: []netbox.PowerPort{{
			Name: "PSU1",
			Endpoints: []netbox.Endpoint{{
				Name:   "output6",
				Device: netbox.Named{ID: 3, Name: "dma-pdu-01"},
			}},
		}},
	}

	// Enter opens the menu; items appear once outlet states resolve.
	step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.menu == nil {
		t.Fatal("menu did not open on enter")
	}
	if len(app.menu.items) != 0 {
		t.Fatal("items built before outlet states resolved")
	}
	if !strings.Contains(app.View(), "checking outlet states") {
		t.Error("menu view missing state-check spinner")
	}

	// Outlet reports ON → "Power on" is gated out.
	step(outletStateMsg{PDU: "dma-pdu-01", Outlet: 6, State: pdu.StateOn})
	view := app.View()
	if strings.Contains(view, "Power on") {
		t.Error("all-on device still offers Power on")
	}
	for _, want := range []string{"Power off", "Power cycle", "outlet 6"} {
		if !strings.Contains(view, want) {
			t.Errorf("gated menu missing %q", want)
		}
	}

	// Cursor 0 is Power off; enter opens the modal for it.
	cmd := step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.menu != nil || app.modal == nil {
		t.Fatal("modal did not open from menu")
	}
	if app.modal.Action != actionOff {
		t.Fatalf("modal action = %s, want power_off", app.modal.Action)
	}
	_ = cmd

	// y must NOT confirm power_off.
	step(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune{'y'}})
	if app.modal == nil {
		t.Fatal("y confirmed power_off — it must require the outlet number")
	}
	// Wrong outlet number is rejected.
	step(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune{'9'}})
	step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.modal == nil {
		t.Fatal("wrong outlet number was accepted")
	}
	// Correct outlet number confirms and yields the dry-run action.
	step(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune{'6'}})
	cmd = step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.modal != nil {
		t.Fatal("modal still open after correct confirmation")
	}
	if cmd == nil {
		t.Fatal("no action command returned")
	}
	res, ok := cmd().(actionResultMsg)
	if !ok {
		t.Fatal("action did not return actionResultMsg")
	}
	if res.Err != nil || !strings.Contains(res.Desc, "dry-run") || !strings.Contains(res.Desc, "outlet 6") {
		t.Errorf("unexpected action result %+v", res)
	}

	// Reopen with outlet OFF → "Power off" gated out, cursor 0 is Power on.
	step(tea.KeyMsg{Type: tea.KeyEnter})
	step(outletStateMsg{PDU: "dma-pdu-01", Outlet: 6, State: pdu.StateOff})
	if strings.Contains(app.View(), "Power off") {
		t.Error("all-off device still offers Power off")
	}
	step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.modal == nil || app.modal.Action != actionOn {
		t.Fatal("cursor 0 on all-off device should be Power on")
	}
	step(tea.KeyMsg{Type: tea.KeyEsc})
	if app.modal != nil {
		t.Fatal("esc did not close modal")
	}

	// Unknown state (query error) → nothing gated.
	step(tea.KeyMsg{Type: tea.KeyEnter})
	step(outletStateMsg{PDU: "dma-pdu-01", Outlet: 6, Err: errFake})
	view = app.View()
	for _, want := range []string{"Power on", "Power off", "Power cycle"} {
		if !strings.Contains(view, want) {
			t.Errorf("unknown-state menu missing %q", want)
		}
	}
	step(tea.KeyMsg{Type: tea.KeyEsc})

	// Race: enter while details are still in flight — the menu must open
	// automatically once the detail message lands.
	delete(app.details, 15)
	app.details[15] = &deviceDetail{loading: true}
	step(tea.KeyMsg{Type: tea.KeyEnter})
	if app.menu != nil {
		t.Fatal("menu opened before details loaded")
	}
	step(detailMsg{DeviceID: 15, PowerPorts: []netbox.PowerPort{{
		Name:      "PSU1",
		Endpoints: []netbox.Endpoint{{Name: "output6", Device: netbox.Named{ID: 3, Name: "dma-pdu-01"}}},
	}}})
	if app.menu == nil {
		t.Fatal("menu did not auto-open after pending detail load")
	}
	if app.statusLine != "" {
		t.Errorf("statusLine not cleared: %q", app.statusLine)
	}
}

// Mixed multi-PSU states must not gate anything.
func TestMenuRebuildMixedStates(t *testing.T) {
	m := &actionMenu{targets: []outletTarget{
		{PDU: "dma-pdu-01", Outlet: 1, State: pdu.StateOn, StateKnown: true, Resolved: true},
		{PDU: "dma-pdu-02", Outlet: 7, State: pdu.StateOff, StateKnown: true, Resolved: true},
	}}
	m.rebuild()
	if len(m.items) != 3 {
		t.Fatalf("mixed states: %d items, want all 3", len(m.items))
	}
}

type fakeErr struct{}

func (fakeErr) Error() string { return "unreachable" }

var errFake = fakeErr{}
