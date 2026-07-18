package netbox

import (
	"context"
	"os"
	"testing"
	"time"
)

func TestParseRackDevices(t *testing.T) {
	raw, err := os.ReadFile("../../testdata/gql_rack_devices.json")
	if err != nil {
		t.Fatal(err)
	}
	devices, err := ParseRackDevices(raw)
	if err != nil {
		t.Fatal(err)
	}
	if len(devices) != 17 {
		t.Fatalf("devices = %d, want 17", len(devices))
	}
	byName := map[string]Device{}
	for _, d := range devices {
		byName[d.Name] = d
	}
	df := byName["dreamflasher"]
	if df.ID != 15 || df.Position == nil || *df.Position != 32 || df.DeviceType.UHeight != 3 {
		t.Errorf("dreamflasher = id %d pos %v u %v, want 15/32/3", df.ID, df.Position, df.DeviceType.UHeight)
	}
	if df.Status.Label != "Active" {
		t.Errorf("status label = %q, want Active", df.Status.Label)
	}
	core := byName["dma-core-a-1"]
	if !core.DeviceType.IsFullDepth || core.FaceValue() != "rear" {
		t.Errorf("dma-core-a-1 full=%v face=%q, want true/rear", core.DeviceType.IsFullDepth, core.FaceValue())
	}
	ha := byName["home-assistant-one"]
	if ha.Parent == nil || ha.Parent.Name != "u39-nuc-shelf" || ha.BayName() != "left" {
		t.Errorf("home-assistant-one parent = %+v, want u39-nuc-shelf/left", ha.Parent)
	}
	if ha.Position != nil {
		t.Errorf("bay child should have nil position, got %v", *ha.Position)
	}
}

// Live GraphQL smoke; runs only when NETBOX_TOKEN is set.
func TestLiveGraphQL(t *testing.T) {
	token := os.Getenv("NETBOX_TOKEN")
	if token == "" {
		t.Skip("NETBOX_TOKEN not set")
	}
	c := New("https://infra.dma.space", token)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	t0 := time.Now()
	racks, roles, err := c.Bootstrap(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("bootstrap: %d racks %d roles in %s", len(racks), len(roles), time.Since(t0))
	t1 := time.Now()
	devices, err := c.RackDevices(ctx, 1)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("rack devices: %d in %s", len(devices), time.Since(t1))
	t2 := time.Now()
	ifaces, ports, _, err := c.DeviceDetail(ctx, 15)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("detail: %d cabled ifaces %d ports in %s", len(ifaces), len(ports), time.Since(t2))
	if len(devices) == 0 || len(racks) == 0 {
		t.Fatal("empty results")
	}
}
