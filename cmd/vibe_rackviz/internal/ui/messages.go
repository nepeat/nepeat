package ui

import (
	"context"
	"time"

	tea "charm.land/bubbletea/v2"
	"golang.org/x/sync/errgroup"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/secrets"
)

const apiTimeout = 20 * time.Second

type tokenMsg struct {
	Token string
	Err   error
}

type racksMsg struct {
	Version string
	Racks   []netbox.Rack
	Roles   []netbox.DeviceRole
	Err     error
}

type rackDataMsg struct {
	RackID  int
	Front   []netbox.ElevationSlot
	Rear    []netbox.ElevationSlot
	Devices []netbox.Device
	Err     error
}

type detailMsg struct {
	DeviceID   int
	Interfaces []netbox.Interface
	PowerPorts []netbox.PowerPort
	Outlets    []netbox.PowerOutlet // populated when the device is a PDU
	Err        error
}

func fetchTokenCmd(tokenOpRef string) tea.Cmd {
	return func() tea.Msg {
		token, err := secrets.NetBoxToken(tokenOpRef)
		return tokenMsg{Token: token, Err: err}
	}
}

func loadRacksCmd(client *netbox.Client) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		var (
			status *netbox.Status
			racks  []netbox.Rack
			roles  []netbox.DeviceRole
		)
		g, gctx := errgroup.WithContext(ctx)
		g.Go(func() (err error) { status, err = client.Status(gctx); return })
		g.Go(func() (err error) { racks, err = client.ListRacks(gctx); return })
		g.Go(func() (err error) { roles, err = client.ListRoles(gctx); return })
		if err := g.Wait(); err != nil {
			return racksMsg{Err: err}
		}
		return racksMsg{Version: status.NetBoxVersion, Racks: racks, Roles: roles}
	}
}

func loadRackCmd(client *netbox.Client, rackID int) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		var (
			front, rear []netbox.ElevationSlot
			devices     []netbox.Device
		)
		g, gctx := errgroup.WithContext(ctx)
		g.Go(func() (err error) { front, err = client.Elevation(gctx, rackID, "front"); return })
		g.Go(func() (err error) { rear, err = client.Elevation(gctx, rackID, "rear"); return })
		g.Go(func() (err error) { devices, err = client.DevicesInRack(gctx, rackID); return })
		if err := g.Wait(); err != nil {
			return rackDataMsg{RackID: rackID, Err: err}
		}
		return rackDataMsg{RackID: rackID, Front: front, Rear: rear, Devices: devices}
	}
}

func loadDetailCmd(client *netbox.Client, deviceID int, isPDU bool) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		var (
			ifaces  []netbox.Interface
			ports   []netbox.PowerPort
			outlets []netbox.PowerOutlet
		)
		g, gctx := errgroup.WithContext(ctx)
		g.Go(func() (err error) { ifaces, err = client.CabledInterfaces(gctx, deviceID); return })
		g.Go(func() (err error) { ports, err = client.PowerPorts(gctx, deviceID); return })
		if isPDU {
			g.Go(func() (err error) { outlets, err = client.PowerOutlets(gctx, deviceID); return })
		}
		if err := g.Wait(); err != nil {
			return detailMsg{DeviceID: deviceID, Err: err}
		}
		return detailMsg{DeviceID: deviceID, Interfaces: ifaces, PowerPorts: ports, Outlets: outlets}
	}
}
