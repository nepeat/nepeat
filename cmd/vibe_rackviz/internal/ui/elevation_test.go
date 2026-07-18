package ui

import (
	"encoding/json"
	"os"
	"testing"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
)

func loadFixture[T any](t *testing.T, name string) []T {
	t.Helper()
	raw, err := os.ReadFile("../../testdata/" + name)
	if err != nil {
		t.Fatal(err)
	}
	var page struct {
		Results []T `json:"results"`
	}
	if err := json.Unmarshal(raw, &page); err != nil {
		t.Fatal(err)
	}
	return page.Results
}

// loadGQLDevices loads the captured GraphQL rack-devices response for MDF.
func loadGQLDevices(t *testing.T) []netbox.Device {
	t.Helper()
	raw, err := os.ReadFile("../../testdata/gql_rack_devices.json")
	if err != nil {
		t.Fatal(err)
	}
	devices, err := netbox.ParseRackDevices(raw)
	if err != nil {
		t.Fatal(err)
	}
	return devices
}

func TestBuildRowsAndBlocks(t *testing.T) {
	devices := loadGQLDevices(t)

	rows := buildRows(devices, 40, false, "front")
	if len(rows) != 40 {
		t.Fatalf("rows = %d, want 40", len(rows))
	}
	for i := 1; i < len(rows); i++ {
		if rows[i].U >= rows[i-1].U {
			t.Fatalf("rows not descending: %d then %d", rows[i-1].U, rows[i].U)
		}
	}

	byU := map[int]int{}
	for _, r := range rows {
		byU[r.U] = r.DeviceID
	}
	if byU[39] != 41 {
		t.Errorf("U39 occupant = %d, want 41 (u39-nuc-shelf)", byU[39])
	}
	if byU[40] != 0 {
		t.Errorf("U40 should be empty, got %d", byU[40])
	}
	// dreamflasher is 3U at position 32 → occupies U32..U34.
	for u := 32; u <= 34; u++ {
		if byU[u] != 15 {
			t.Errorf("U%d occupant = %d, want 15 (dreamflasher)", u, byU[u])
		}
	}
	// dma-core-a-1 is rear-mounted but full-depth → shows on the front too.
	if byU[37] != 21 {
		t.Errorf("U37 occupant = %d, want 21 (dma-core-a-1, full depth)", byU[37])
	}
	// dma-ckg2 is rear-mounted, NOT full depth → absent from the front...
	if byU[36] == 31 {
		t.Error("dma-ckg2 (rear, half-depth) must not appear on the front face")
	}
	// ...but present on the rear.
	rearByU := map[int]int{}
	for _, r := range buildRows(devices, 40, false, "rear") {
		rearByU[r.U] = r.DeviceID
	}
	if rearByU[34] != 31 {
		t.Errorf("rear U34 occupant = %d, want 31 (dma-ckg2)", rearByU[34])
	}

	blocks := buildBlocks(rows, devices)
	var racked, bays, zeroU int
	blockFor := map[string]block{}
	for _, b := range blocks {
		switch {
		case b.TopU != 0:
			racked++
		case b.Bay != "":
			bays++
		default:
			zeroU++
		}
		blockFor[b.Device.Name] = b
	}
	if bays != 2 {
		t.Errorf("bay blocks = %d, want 2", bays)
	}
	if zeroU != 3 {
		t.Errorf("zero-U blocks = %d, want 3", zeroU)
	}
	if b := blockFor["u39-nuc-shelf"]; b.TopU == 0 {
		t.Errorf("u39-nuc-shelf should be racked, got %+v", b)
	}
	if b := blockFor["dreamflasher"]; b.Rows != 3 || b.TopU != 34 {
		t.Errorf("dreamflasher block = top %d rows %d, want top 34 rows 3", b.TopU, b.Rows)
	}
	if b := blockFor["home-assistant-one"]; b.Bay != "left" {
		t.Errorf("home-assistant-one bay = %q, want left", b.Bay)
	}
	if b := blockFor["ks4"]; b.Bay != "right" {
		t.Errorf("ks4 bay = %q, want right", b.Bay)
	}
	if b, ok := blockFor["dma-pdu-01"]; !ok || b.TopU != 0 || b.Bay != "" {
		t.Errorf("dma-pdu-01 should be a bare 0U block, got %+v", b)
	}

	kids := childrenByParent(blocks)
	if len(kids[41]) != 2 {
		t.Errorf("shelf (id 41) children = %d, want 2", len(kids[41]))
	}
}
