package ui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

func TestPDUView(t *testing.T) {
	racks := loadFixture[netbox.Rack](t, "racks.json")

	cfg, _ := config.Load("")
	cfg.PDUs["dma-pdu-01"] = config.PDU{Driver: "none"}
	app := NewApp(cfg, "", true)
	app.client = netbox.New("http://example.invalid", "x")

	step := func(msg tea.Msg) tea.Cmd {
		m, cmd := app.Update(msg)
		app = m.(*App)
		return cmd
	}
	step(tea.WindowSizeMsg{Width: 150, Height: 45})
	step(racksMsg{Racks: racks})

	// Left pane lists the PDU section.
	if !strings.Contains(app.render(), "⚡ dma-pdu-01") {
		t.Fatal("left pane missing PDUs section")
	}

	// Select the PDU entry (after the 4 racks).
	app.focus = focusRacks
	app.rackCursor = len(app.racks)
	if app.selectedPDUName() != "dma-pdu-01" {
		t.Fatalf("selectedPDUName = %q", app.selectedPDUName())
	}
	if cmd := app.selectPDU("dma-pdu-01"); cmd == nil {
		t.Fatal("selectPDU issued no load")
	}

	outlets := []netbox.PowerOutlet{
		{Name: "output1", Endpoints: []netbox.Endpoint{{Name: "PS-B", Device: netbox.Named{Name: "dma-core-a-1"}}}},
		{Name: "output2", Endpoints: []netbox.Endpoint{{Name: "Power", Device: netbox.Named{Name: "home-assistant-one"}}}},
		{Name: "output3"},
		{Name: "output4", Endpoints: []netbox.Endpoint{{Name: "PSU1", Device: netbox.Named{Name: "dma-con-sw-1"}}}},
		{Name: "output5", Description: "space heater (temporary)", MarkConnected: true},
		{Name: "output6", Description: "described but unconnected"},
	}
	cmd := step(pduViewMsg{PDU: "dma-pdu-01", Outlets: outlets, States: map[int]pdu.OutletState{
		1: pdu.StateOn, 2: pdu.StateOn, 3: pdu.StateOff, 4: pdu.StateOff, 5: pdu.StateOn, 6: pdu.StateOff,
	}})
	// The follow-up draw batch must include the free outlets too.
	if cmd == nil {
		t.Fatal("pduViewMsg scheduled no draw batch")
	}
	for _, o := range []int{3, 5} {
		if e := app.outletDraw[orKey("dma-pdu-01", o)]; e == nil || !e.loading {
			t.Errorf("free outlet %d not queued for readings", o)
		}
	}

	app.focus = focusElevation
	view := app.render()
	// mark_connected (output5) is NetBox-canon connected → not counted free;
	// a described-but-unconnected outlet (output6) still is.
	for _, want := range []string{"⚡ dma-pdu-01", "01 ╶─ dma-core-a-1 · PS-B", "03 ╶─ (free)", "05 ╶─ space heater (temporary)", "06 ╶─ described but unconnected", "3 on · 3 off · 2 free"} {
		if !strings.Contains(view, want) {
			t.Errorf("pdu view missing %q\n%s", want, view)
		}
	}

	// A NetBox-free outlet that is actually drawing power gets flagged.
	step(outletReadingsMsg{PDU: "dma-pdu-01", Requested: []int{3, 5}, ByOutlet: map[int]pdu.PowerReading{
		3: {Watts: 66.6, Amps: 0.55},
		5: {Watts: 800, Amps: 6.6},
	}})
	view = app.render()
	if !strings.Contains(view, "⚠ undocumented load") || !strings.Contains(view, "66.6 W") {
		t.Errorf("ghost load not flagged:\n%s", view)
	}
	if strings.Contains(view, "space heater (temporary) ⚠") {
		t.Error("described free outlet wrongly flagged as undocumented")
	}
	if !strings.Contains(view, "800.0 W") {
		t.Error("described free outlet missing its draw")
	}

	// Inline draw appears once the reading lands.
	app.outletDraw["dma-pdu-01/1"] = &outletReadingEntry{loading: true}
	step(outletReadingsMsg{PDU: "dma-pdu-01", Requested: []int{1}, ByOutlet: map[int]pdu.PowerReading{1: {Watts: 145.3, Amps: 1.2}}})
	view = app.render()
	if !strings.Contains(view, "145.3 W") {
		t.Errorf("pdu view missing inline draw:\n%s", view)
	}
	if !strings.Contains(view, "145.3 W · 1.20 A") {
		t.Error("info pane missing selected outlet draw")
	}

	// Cursor + outlet menu → modal for that outlet.
	step(tea.KeyPressMsg{Code: 'j', Text: "j"})
	step(tea.KeyPressMsg{Code: 'j', Text: "j"})
	step(tea.KeyPressMsg{Code: 'j', Text: "j"})
	if app.outletCursor != 3 {
		t.Fatalf("outletCursor = %d, want 3", app.outletCursor)
	}
	step(tea.KeyPressMsg{Code: tea.KeyEnter})
	if app.menu == nil {
		t.Fatal("outlet menu did not open")
	}
	// Outlet 4 is off → Power off gated out; cursor 0 = Power on.
	if strings.Contains(app.render(), "Power off") {
		t.Error("off outlet still offers Power off")
	}
	step(tea.KeyPressMsg{Code: tea.KeyEnter})
	if app.modal == nil || app.modal.Action != actionOn {
		t.Fatalf("modal = %+v, want power_on", app.modal)
	}
	if len(app.modal.Targets) != 1 || app.modal.Targets[0].Outlet != 4 {
		t.Fatalf("modal targets = %+v, want outlet 4", app.modal.Targets)
	}
	step(tea.KeyPressMsg{Code: tea.KeyEsc})

	// Free outlet also gets a menu (someone may want to power a new feed).
	app.outletCursor = 2
	step(tea.KeyPressMsg{Code: tea.KeyEnter})
	if app.menu == nil {
		t.Fatal("free-outlet menu did not open")
	}
	step(tea.KeyPressMsg{Code: tea.KeyEsc})

	// A successful power action schedules the settle refresh; the refresh
	// message re-polls state, draw, and the live PDU view.
	if cmd := step(actionResultMsg{Desc: "power_on dma-pdu-01 outlet 4", PDU: "dma-pdu-01", Outlet: 4}); cmd == nil {
		t.Fatal("action result scheduled no follow-ups")
	}
	if cmd := step(actionRefreshMsg{PDU: "dma-pdu-01", Outlet: 4}); cmd == nil {
		t.Fatal("action refresh issued no commands")
	}
	if e := app.outletDraw["dma-pdu-01/4"]; e == nil || !e.loading {
		t.Fatal("action refresh did not re-queue the outlet's draw")
	}
}
