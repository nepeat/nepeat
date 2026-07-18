package netbox

// Live smoke test against the real NetBox; runs only when NETBOX_TOKEN is set.

import (
	"context"
	"os"
	"testing"
	"time"
)

func TestLiveRackLoad(t *testing.T) {
	token := os.Getenv("NETBOX_TOKEN")
	if token == "" {
		t.Skip("NETBOX_TOKEN not set")
	}
	c := New("https://infra.dma.space", token)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	t0 := time.Now()
	front, err := c.Elevation(ctx, 1, "front")
	t.Logf("front: %d slots err=%v after %s", len(front), err, time.Since(t0))
	if err != nil {
		t.Fatal(err)
	}
	rear, err := c.Elevation(ctx, 1, "rear")
	t.Logf("rear: %d slots err=%v after %s", len(rear), err, time.Since(t0))
	if err != nil {
		t.Fatal(err)
	}
	devs, err := c.DevicesInRack(ctx, 1)
	t.Logf("devices: %d err=%v after %s", len(devs), err, time.Since(t0))
	if err != nil {
		t.Fatal(err)
	}
}
