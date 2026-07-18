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
		// One GraphQL query (racks+roles) alongside the REST status probe.
		g, gctx := errgroup.WithContext(ctx)
		g.Go(func() (err error) { status, err = client.Status(gctx); return })
		g.Go(func() (err error) { racks, roles, err = client.Bootstrap(gctx); return })
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
		devices, err := client.RackDevices(ctx, rackID)
		if err != nil {
			return rackDataMsg{RackID: rackID, Err: err}
		}
		return rackDataMsg{RackID: rackID, Devices: devices}
	}
}

func loadDetailCmd(client *netbox.Client, deviceID int) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		ifaces, ports, outlets, err := client.DeviceDetail(ctx, deviceID)
		if err != nil {
			return detailMsg{DeviceID: deviceID, Err: err}
		}
		return detailMsg{DeviceID: deviceID, Interfaces: ifaces, PowerPorts: ports, Outlets: outlets}
	}
}
