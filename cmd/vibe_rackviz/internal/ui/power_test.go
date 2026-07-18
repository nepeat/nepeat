package ui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

func TestJoinOutletStates(t *testing.T) {
	outlets := []netbox.PowerOutlet{
		{Name: "output1", Endpoints: []netbox.Endpoint{{Device: netbox.Named{Name: "core"}}}},
		{Name: "output2", Endpoints: []netbox.Endpoint{{Device: netbox.Named{Name: "dual-psu"}}}},
		{Name: "output3", Endpoints: []netbox.Endpoint{{Device: netbox.Named{Name: "dual-psu"}}}},
		{Name: "output4", Endpoints: []netbox.Endpoint{{Device: netbox.Named{Name: "dark"}}}},
		{Name: "output5"}, // free outlet
	}
	states := map[int]pdu.OutletState{
		1: pdu.StateOff,
		2: pdu.StateOn,
		3: pdu.StateOff, // dual-psu: one on + one off → on
		4: pdu.StateOff,
	}
	got := joinOutletStates(outlets, states, `(\d+)$`)
	want := map[string]pdu.OutletState{
		"core":     pdu.StateOff,
		"dual-psu": pdu.StateOn,
		"dark":     pdu.StateOff,
	}
	if len(got) != len(want) {
		t.Fatalf("joined = %v, want %v", got, want)
	}
	for k, v := range want {
		if got[k] != v {
			t.Errorf("%s = %v, want %v", k, got[k], v)
		}
	}
}

func TestPowerStatesAndToast(t *testing.T) {
	devices := loadGQLDevices(t)
	racks := loadFixture[netbox.Rack](t, "racks.json")

	cfg, _ := config.Load("")
	cfg.PDUs["dma-pdu-01"] = config.PDU{Driver: "none"}
	app := NewApp(cfg, "MDF", true)
	app.client = netbox.New("http://example.invalid", "x")

	step := func(msg tea.Msg) tea.Cmd {
		m, cmd := app.Update(msg)
		app = m.(*App)
		return cmd
	}
	step(tea.WindowSizeMsg{Width: 150, Height: 45})
	step(racksMsg{Racks: racks})
	// Rack load must schedule a power sweep for the configured PDU.
	if cmd := step(rackDataMsg{RackID: 1, Devices: devices}); cmd == nil {
		t.Fatal("rack load returned no commands (expected power sweep)")
	}

	// Power states arrive → deviceState reflects them.
	step(powerStatesMsg{PDU: "dma-pdu-01", ByDevice: map[string]pdu.OutletState{
		"dreamflasher": pdu.StateOn,
		"dma-con-sw-1": pdu.StateOff,
	}})
	if st := app.deviceState("dreamflasher"); st != pdu.StateOn {
		t.Errorf("dreamflasher = %v, want on", st)
	}
	if st := app.deviceState("dma-con-sw-1"); st != pdu.StateOff {
		t.Errorf("dma-con-sw-1 = %v, want off", st)
	}
	if st := app.deviceState("Ramiel"); st != pdu.StateUnknown {
		t.Errorf("Ramiel = %v, want unknown", st)
	}

	// Successful action → toast set, clear tick scheduled, later cleared.
	cmd := step(actionResultMsg{Desc: "power_on dma-pdu-01 outlet 11 (dma-con-sw-1)", PDU: "dma-pdu-01"})
	if app.toast == "" {
		t.Fatal("toast not set after successful action")
	}
	if cmd == nil {
		t.Fatal("no toast-expiry command scheduled")
	}
	if !strings.Contains(app.render(), "power_on dma-pdu-01") {
		t.Error("footer missing toast")
	}
	step(toastClearMsg{gen: app.toastGen})
	if app.toast != "" {
		t.Errorf("toast not cleared by tick: %q", app.toast)
	}

	// A stale clear (older gen) must not wipe a newer toast.
	step(actionResultMsg{Desc: "second action", PDU: "dma-pdu-01"})
	step(toastClearMsg{gen: app.toastGen - 1})
	if app.toast == "" {
		t.Error("stale toastClearMsg cleared a newer toast")
	}
	// Any keypress clears it.
	step(tea.KeyPressMsg{Code: 'j', Text: "j"})
	if app.toast != "" {
		t.Error("keypress did not clear toast")
	}

	// Selecting a dual-PSU device with loaded detail fires per-outlet draw
	// readings; the info pane shows W/A per outlet and a summed Total line.
	// (The toast keypress above walked the left cursor into the PDU section —
	// return to MDF first.)
	app.rackCursor = 3
	app.focus = focusElevation
	app.devCursor = indexOfBlock(t, app.rackData[1].blocks("front"), "dreamflasher")
	app.details[15] = &deviceDetail{
		powerPorts: []netbox.PowerPort{
			{
				Name: "PSU1",
				Endpoints: []netbox.Endpoint{{
					Name:   "output6",
					Device: netbox.Named{ID: 3, Name: "dma-pdu-01"},
				}},
			},
			{
				Name: "PSU2",
				Endpoints: []netbox.Endpoint{{
					Name:   "output7",
					Device: netbox.Named{ID: 3, Name: "dma-pdu-01"},
				}},
			},
		},
	}
	if cmd := app.selectDevice(); cmd == nil {
		t.Fatal("selectDevice fired no outlet-draw commands")
	}
	if e := app.outletDraw["dma-pdu-01/6"]; e == nil || !e.loading {
		t.Fatal("no loading entry for dma-pdu-01/6")
	}
	if !strings.Contains(app.render(), "measuring…") {
		t.Error("info pane missing measuring placeholder")
	}
	step(outletReadingMsg{PDU: "dma-pdu-01", Outlet: 6, Reading: pdu.PowerReading{Watts: 45.2, Amps: 0.38}})
	view := app.render()
	if !strings.Contains(view, "45.2 W") || !strings.Contains(view, "0.38 A") {
		t.Errorf("info pane missing outlet draw, view:\n%s", view)
	}
	// Second outlet still measuring → Total is marked partial.
	if !strings.Contains(view, "Total 45.2 W · 0.38 A …") {
		t.Errorf("info pane missing partial total, view:\n%s", view)
	}
	step(outletReadingMsg{PDU: "dma-pdu-01", Outlet: 7, Reading: pdu.PowerReading{Watts: 23.1, Amps: 0.21}})
	view = app.render()
	if !strings.Contains(view, "Total 68.3 W · 0.59 A") || strings.Contains(view, "0.59 A …") {
		t.Errorf("info pane missing final total, view:\n%s", view)
	}
}
