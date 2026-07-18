package netbox

import (
	"context"
	"fmt"
	"net/url"
	"strconv"
)

func (c *Client) Status(ctx context.Context) (*Status, error) {
	var s Status
	if err := c.get(ctx, "/api/status/", nil, &s); err != nil {
		return nil, err
	}
	return &s, nil
}

func (c *Client) ListRacks(ctx context.Context) ([]Rack, error) {
	return paginate[Rack](ctx, c, "/api/dcim/racks/", nil)
}

func (c *Client) ListRoles(ctx context.Context) ([]DeviceRole, error) {
	return paginate[DeviceRole](ctx, c, "/api/dcim/device-roles/", nil)
}

// Elevation returns the U-slot array for one face, ordered top-down as NetBox
// renders it. Slots step by 0.5 U.
func (c *Client) Elevation(ctx context.Context, rackID int, face string) ([]ElevationSlot, error) {
	q := url.Values{"face": {face}, "render": {"json"}}
	return paginate[ElevationSlot](ctx, c, fmt.Sprintf("/api/dcim/racks/%d/elevation/", rackID), q)
}

func (c *Client) DevicesInRack(ctx context.Context, rackID int) ([]Device, error) {
	q := url.Values{"rack_id": {strconv.Itoa(rackID)}}
	return paginate[Device](ctx, c, "/api/dcim/devices/", q)
}

func (c *Client) CabledInterfaces(ctx context.Context, deviceID int) ([]Interface, error) {
	q := url.Values{"device_id": {strconv.Itoa(deviceID)}, "cabled": {"true"}}
	return paginate[Interface](ctx, c, "/api/dcim/interfaces/", q)
}

func (c *Client) PowerPorts(ctx context.Context, deviceID int) ([]PowerPort, error) {
	q := url.Values{"device_id": {strconv.Itoa(deviceID)}}
	return paginate[PowerPort](ctx, c, "/api/dcim/power-ports/", q)
}

func (c *Client) PowerOutlets(ctx context.Context, pduDeviceID int) ([]PowerOutlet, error) {
	q := url.Values{"device_id": {strconv.Itoa(pduDeviceID)}}
	return paginate[PowerOutlet](ctx, c, "/api/dcim/power-outlets/", q)
}
