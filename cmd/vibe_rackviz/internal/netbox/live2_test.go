package netbox

import (
	"context"
	"os"
	"testing"
	"time"
)

func TestLiveDeviceDetail(t *testing.T) {
	token := os.Getenv("NETBOX_TOKEN")
	if token == "" {
		t.Skip("NETBOX_TOKEN not set")
	}
	c := New("https://infra.dma.space", token)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	for _, id := range []int{21, 41} { // dma-core-a-1, u39-nuc-shelf
		ifaces, err := c.CabledInterfaces(ctx, id)
		if err != nil {
			t.Fatalf("interfaces device %d: %v", id, err)
		}
		ports, err := c.PowerPorts(ctx, id)
		if err != nil {
			t.Fatalf("power ports device %d: %v", id, err)
		}
		t.Logf("device %d: %d cabled interfaces, %d power ports", id, len(ifaces), len(ports))
	}
}
