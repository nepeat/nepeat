package ui

import (
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

func TestMouseInteractions(t *testing.T) {
	slots := loadFixture[netbox.ElevationSlot](t, "elevation_front.json")
	devices := loadFixture[netbox.Device](t, "devices_mdf.json")
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
	click := func(x, y int) tea.Cmd {
		app.render() // clicks resolve against the last rendered frame
		return step(tea.MouseClickMsg{X: x, Y: y, Button: tea.MouseLeft})
	}
	step(tea.WindowSizeMsg{Width: 150, Height: 45})
	step(racksMsg{Racks: racks})
	step(rackDataMsg{RackID: 1, Front: slots, Rear: slots, Devices: devices})

	// Click the first rack line (left pane, content row 0 → y=3).
	click(3, 3)
	if app.focus != focusRacks || app.rackCursor != 0 {
		t.Fatalf("rack click: focus=%v cursor=%d, want racks/0", app.focus, app.rackCursor)
	}

	// Back to MDF (list shows rack0 + its detail line first → MDF at y=7).
	click(3, 7)
	if app.racks[app.rackCursor].Name != "MDF" {
		t.Fatalf("rack click: selected %q, want MDF", app.racks[app.rackCursor].Name)
	}

	// Click dreamflasher's block: rows are U40..U1, U34 is row index 6 → y=9.
	click(30, 9)
	if app.focus != focusElevation {
		t.Fatalf("elevation click: focus=%v", app.focus)
	}
	d := app.selectedDevice()
	if d == nil || d.Name != "dreamflasher" {
		t.Fatalf("elevation click selected %v, want dreamflasher", d)
	}

	// Second click on the selected device opens the action menu.
	app.details[15] = &deviceDetail{
		powerPorts: []netbox.PowerPort{{
			Name: "PSU1",
			Endpoints: []netbox.Endpoint{{
				Name:   "output6",
				Device: netbox.Named{ID: 3, Name: "dma-pdu-01"},
			}},
		}},
	}
	click(30, 9)
	if app.menu == nil {
		t.Fatal("second click did not open the menu")
	}
	step(outletStateMsg{PDU: "dma-pdu-01", Outlet: 6, State: pdu.StateOn})

	// Click the "Power cycle" item inside the popup.
	app.render()
	itemLine := -1
	for i, owner := range app.hit.menuLines {
		if owner == 1 {
			itemLine = i
		}
	}
	if itemLine < 0 {
		t.Fatal("no menu line maps to item 1")
	}
	click(app.hit.overlayX+4, 1+app.hit.overlayY+2+itemLine)
	if app.menu != nil || app.modal == nil {
		t.Fatal("menu item click did not open the modal")
	}
	if app.modal.Action != actionCycle {
		t.Fatalf("modal action = %s, want power_cycle", app.modal.Action)
	}

	// Click outside the modal closes it.
	click(1, 1+app.hit.overlayY+1)
	if app.modal != nil {
		t.Fatal("outside click did not close the modal")
	}

	// Wheel over the rack list moves the rack cursor and focuses the pane.
	app.render()
	step(tea.MouseWheelMsg{X: 3, Y: 5, Button: tea.MouseWheelUp})
	if app.focus != focusRacks {
		t.Fatalf("wheel focus = %v, want racks", app.focus)
	}
	if app.racks[app.rackCursor].Name != "Member Rack" {
		t.Fatalf("wheel selected %q, want Member Rack", app.racks[app.rackCursor].Name)
	}
}
