package ui

import (
	"context"
	"time"

	tea "charm.land/bubbletea/v2"

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
		status, err := client.Status(ctx)
		if err != nil {
			return racksMsg{Err: err}
		}
		racks, err := client.ListRacks(ctx)
		if err != nil {
			return racksMsg{Err: err}
		}
		roles, err := client.ListRoles(ctx)
		if err != nil {
			return racksMsg{Err: err}
		}
		return racksMsg{Version: status.NetBoxVersion, Racks: racks, Roles: roles}
	}
}

func loadRackCmd(client *netbox.Client, rackID int) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		front, err := client.Elevation(ctx, rackID, "front")
		if err != nil {
			return rackDataMsg{RackID: rackID, Err: err}
		}
		rear, err := client.Elevation(ctx, rackID, "rear")
		if err != nil {
			return rackDataMsg{RackID: rackID, Err: err}
		}
		devices, err := client.DevicesInRack(ctx, rackID)
		if err != nil {
			return rackDataMsg{RackID: rackID, Err: err}
		}
		return rackDataMsg{RackID: rackID, Front: front, Rear: rear, Devices: devices}
	}
}

func loadDetailCmd(client *netbox.Client, deviceID int, isPDU bool) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), apiTimeout)
		defer cancel()
		ifaces, err := client.CabledInterfaces(ctx, deviceID)
		if err != nil {
			return detailMsg{DeviceID: deviceID, Err: err}
		}
		ports, err := client.PowerPorts(ctx, deviceID)
		if err != nil {
			return detailMsg{DeviceID: deviceID, Err: err}
		}
		var outlets []netbox.PowerOutlet
		if isPDU {
			outlets, err = client.PowerOutlets(ctx, deviceID)
			if err != nil {
				return detailMsg{DeviceID: deviceID, Err: err}
			}
		}
		return detailMsg{DeviceID: deviceID, Interfaces: ifaces, PowerPorts: ports, Outlets: outlets}
	}
}
