package netbox

import (
	"context"
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

// PowerOutlets lists a PDU's outlets with their connected endpoints (used by
// the power sweep; everything else moved to the GraphQL one-shots).
func (c *Client) PowerOutlets(ctx context.Context, pduDeviceID int) ([]PowerOutlet, error) {
	q := url.Values{"device_id": {strconv.Itoa(pduDeviceID)}}
	return paginate[PowerOutlet](ctx, c, "/api/dcim/power-outlets/", q)
}
