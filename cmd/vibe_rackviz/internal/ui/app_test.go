package ui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
)

func TestViewAfterRackLoad(t *testing.T) {
	devices := loadGQLDevices(t)
	racks := loadFixture[netbox.Rack](t, "racks.json")

	cfg, err := config.Load("")
	if err != nil {
		t.Fatal(err)
	}
	app := NewApp(cfg, "MDF", false)
	app.client = netbox.New("http://example.invalid", "x")

	step := func(msg tea.Msg) {
		m, _ := app.Update(msg)
		app = m.(*App)
	}
	step(tea.WindowSizeMsg{Width: 150, Height: 45})
	step(racksMsg{Version: "4.5.9", Racks: racks, Roles: []netbox.DeviceRole{{Name: "server", Color: "9e9e9e"}}})
	step(rackDataMsg{RackID: 1, Devices: devices})

	view := app.render()
	for _, want := range []string{"u39-nuc-shelf", "dreamflasher", "RACKS", "MDF FRONT"} {
		if !strings.Contains(view, want) {
			t.Errorf("view missing %q", want)
		}
	}
	if strings.Contains(view, "loading rack") {
		t.Error("view still shows loading")
	}

	// Cursor to the last block (a bare 0U device) — the bays + 0U sections
	// must scroll into view.
	app.focus = focusElevation
	rd := app.rackData[1]
	app.devCursor = len(rd.blocks("front")) - 1
	view = app.render()
	for _, want := range []string{"── bays", "u39-nuc-shelf/left", "── 0U", "dma-pdu-01"} {
		if !strings.Contains(view, want) {
			t.Errorf("scrolled view missing %q", want)
		}
	}

	// Left/right arrows move pane focus, clamped at the ends.
	app.focus = focusRacks
	step(tea.KeyPressMsg{Code: tea.KeyLeft})
	if app.focus != focusRacks {
		t.Errorf("left from racks should clamp, got %v", app.focus)
	}
	step(tea.KeyPressMsg{Code: tea.KeyRight})
	step(tea.KeyPressMsg{Code: tea.KeyRight})
	if app.focus != focusInfo {
		t.Errorf("two rights should reach info pane, got %v", app.focus)
	}
	step(tea.KeyPressMsg{Code: tea.KeyRight})
	if app.focus != focusInfo {
		t.Errorf("right from info should clamp, got %v", app.focus)
	}
	step(tea.KeyPressMsg{Code: tea.KeyLeft})
	if app.focus != focusElevation {
		t.Errorf("left from info should reach elevation, got %v", app.focus)
	}

	// Key handling: tab focuses elevation, j moves, f flips to rear.
	app.devCursor = 0
	app.focus = focusRacks
	step(tea.KeyPressMsg{Code: tea.KeyTab})
	if app.focus != focusElevation {
		t.Fatalf("focus after tab = %v, want elevation", app.focus)
	}
	step(tea.KeyPressMsg{Code: 'j', Text: "j"})
	if app.devCursor != 1 {
		t.Errorf("devCursor after j = %d, want 1", app.devCursor)
	}
	step(tea.KeyPressMsg{Code: 'f', Text: "f"})
	if app.face != "rear" {
		t.Errorf("face after f = %q, want rear", app.face)
	}
	if !strings.Contains(app.render(), "MDF REAR") {
		t.Error("rear view missing MDF REAR title")
	}

	// Device detail message populates the info panel.
	ifaces := loadFixture[netbox.Interface](t, "ifaces_dreamflasher.json")
	app.details[15] = &deviceDetail{loading: true}
	step(detailMsg{DeviceID: 15, Interfaces: ifaces, PowerPorts: []netbox.PowerPort{}})
	app.face = "front"
	app.devCursor = indexOfBlock(t, app.rackData[1].blocks("front"), "dreamflasher")
	view = app.render()
	for _, want := range []string{"Network", "BMC → dma-core-a-1 Gi1/0/13", "eth0 → dma-core-a-1 Gi1/0/1"} {
		if !strings.Contains(view, want) {
			t.Errorf("detail view missing %q", want)
		}
	}
}

func indexOfBlock(t *testing.T, blocks []block, name string) int {
	t.Helper()
	for i, b := range blocks {
		if b.Device.Name == name {
			return i
		}
	}
	t.Fatalf("block %q not found", name)
	return -1
}
