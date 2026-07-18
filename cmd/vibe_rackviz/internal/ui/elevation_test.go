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

func TestBuildRowsAndBlocks(t *testing.T) {
	slots := loadFixture[netbox.ElevationSlot](t, "elevation_front.json")
	devices := loadFixture[netbox.Device](t, "devices_mdf.json")

	rows := buildRows(slots)
	if len(rows) == 0 {
		t.Fatal("no rows built")
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
	// MDF front: 12 positioned devices, 2 bay children, 3 bare 0U devices.
	if bays != 2 {
		t.Errorf("bay blocks = %d, want 2", bays)
	}
	if zeroU != 3 {
		t.Errorf("zero-U blocks = %d, want 3", zeroU)
	}
	if b := blockFor["u39-nuc-shelf"]; b.TopU != 39 || b.Rows != 1 {
		t.Errorf("u39-nuc-shelf block = top %d rows %d, want top 39 rows 1", b.TopU, b.Rows)
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
