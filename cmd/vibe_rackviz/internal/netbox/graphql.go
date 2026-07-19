package netbox

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"sort"
	"strconv"
	"strings"
)

// GraphQL one-shot queries: what took several REST round-trips (rack devices
// + elevation faces, interface/power detail, racks + roles) collapses into a
// single POST each. The rack elevation is computed locally from device
// positions and u_height, since /racks/{id}/elevation/ is REST-only.

func (c *Client) gql(ctx context.Context, query string, out any) error {
	body, err := json.Marshal(map[string]string{"query": query})
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.base+"/graphql/", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Token "+c.token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("graphql: %s: %s", resp.Status, strings.TrimSpace(string(b)))
	}
	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}
	return decodeGQL(raw, out)
}

func decodeGQL(raw []byte, out any) error {
	var envelope struct {
		Data   json.RawMessage `json:"data"`
		Errors []struct {
			Message string `json:"message"`
		} `json:"errors"`
	}
	if err := json.Unmarshal(raw, &envelope); err != nil {
		return fmt.Errorf("graphql: bad response: %w", err)
	}
	if len(envelope.Errors) > 0 {
		return fmt.Errorf("graphql: %s", envelope.Errors[0].Message)
	}
	return json.Unmarshal(envelope.Data, out)
}

// gqlID and gqlNum absorb NetBox's string-typed GraphQL scalars.
func gqlID(s string) int {
	n, _ := strconv.Atoi(s)
	return n
}

func gqlNum(s string) float64 {
	f, _ := strconv.ParseFloat(s, 64)
	return f
}

func titleCase(s string) string {
	if s == "" {
		return s
	}
	return strings.ToUpper(s[:1]) + s[1:]
}

const bootstrapQuery = `query {
  rack_list { id name u_height desc_units site { name } devices { id } }
  device_role_list { id name color }
}`

// Bootstrap fetches racks and device roles in one query.
func (c *Client) Bootstrap(ctx context.Context) ([]Rack, []DeviceRole, error) {
	var data struct {
		Racks []struct {
			ID        string `json:"id"`
			Name      string `json:"name"`
			UHeight   int    `json:"u_height"`
			DescUnits bool   `json:"desc_units"`
			Site      struct {
				Name string `json:"name"`
			} `json:"site"`
			Devices []struct {
				ID string `json:"id"`
			} `json:"devices"`
		} `json:"rack_list"`
		Roles []struct {
			ID    string `json:"id"`
			Name  string `json:"name"`
			Color string `json:"color"`
		} `json:"device_role_list"`
	}
	if err := c.gql(ctx, bootstrapQuery, &data); err != nil {
		return nil, nil, err
	}
	racks := make([]Rack, len(data.Racks))
	for i, r := range data.Racks {
		racks[i] = Rack{
			ID:          gqlID(r.ID),
			Name:        r.Name,
			UHeight:     r.UHeight,
			DescUnits:   r.DescUnits,
			Site:        Named{Name: r.Site.Name},
			DeviceCount: len(r.Devices),
		}
	}
	roles := make([]DeviceRole, len(data.Roles))
	for i, r := range data.Roles {
		roles[i] = DeviceRole{ID: gqlID(r.ID), Name: r.Name, Color: r.Color}
	}
	return racks, roles, nil
}

const rackDevicesQuery = `query {
  device_list(filters: {rack_id: %d}) {
    id name position face serial description status
    device_type { model u_height is_full_depth manufacturer { name } }
    role { name }
    parent_bay { name device { id name } }
    primary_ip4 { address }
  }
}`

type gqlDevice struct {
	ID          string  `json:"id"`
	Name        string  `json:"name"`
	Position    *string `json:"position"`
	Face        *string `json:"face"`
	Serial      string  `json:"serial"`
	Description string  `json:"description"`
	Status      string  `json:"status"`
	DeviceType  struct {
		Model        string `json:"model"`
		UHeight      string `json:"u_height"`
		IsFullDepth  bool   `json:"is_full_depth"`
		Manufacturer struct {
			Name string `json:"name"`
		} `json:"manufacturer"`
	} `json:"device_type"`
	Role struct {
		Name string `json:"name"`
	} `json:"role"`
	ParentBay *struct {
		Name   string `json:"name"`
		Device struct {
			ID   string `json:"id"`
			Name string `json:"name"`
		} `json:"device"`
	} `json:"parent_bay"`
	PrimaryIP4 *struct {
		Address string `json:"address"`
	} `json:"primary_ip4"`
}

func (g gqlDevice) toDevice() Device {
	d := Device{
		ID:          gqlID(g.ID),
		Name:        g.Name,
		Serial:      g.Serial,
		Description: g.Description,
		Status:      valueLabel{Value: g.Status, Label: titleCase(g.Status)},
		Role:        Named{Name: g.Role.Name},
		DeviceType: DeviceType{
			Model:        g.DeviceType.Model,
			UHeight:      gqlNum(g.DeviceType.UHeight),
			IsFullDepth:  g.DeviceType.IsFullDepth,
			Manufacturer: Named{Name: g.DeviceType.Manufacturer.Name},
		},
	}
	if g.Position != nil {
		p := gqlNum(*g.Position)
		if !math.IsNaN(p) {
			d.Position = &p
		}
	}
	if g.Face != nil {
		d.Face = &valueLabel{Value: *g.Face, Label: titleCase(*g.Face)}
	}
	if g.ParentBay != nil {
		p := &ParentRef{ID: gqlID(g.ParentBay.Device.ID), Name: g.ParentBay.Device.Name}
		p.DeviceBay.Name = g.ParentBay.Name
		d.Parent = p
	}
	if g.PrimaryIP4 != nil {
		d.PrimaryIP = &struct {
			Address string `json:"address"`
		}{Address: g.PrimaryIP4.Address}
	}
	return d
}

// ParseRackDevices decodes a raw rack-devices GraphQL response (exported for
// test fixtures).
func ParseRackDevices(raw []byte) ([]Device, error) {
	var data struct {
		Devices []gqlDevice `json:"device_list"`
	}
	if err := decodeGQL(raw, &data); err != nil {
		return nil, err
	}
	devices := make([]Device, len(data.Devices))
	for i, g := range data.Devices {
		devices[i] = g.toDevice()
	}
	return devices, nil
}

// RackDevices fetches every device in a rack — with the unit heights and
// depth flags needed to build the elevation locally — in one query.
func (c *Client) RackDevices(ctx context.Context, rackID int) ([]Device, error) {
	var data struct {
		Devices []gqlDevice `json:"device_list"`
	}
	if err := c.gql(ctx, fmt.Sprintf(rackDevicesQuery, rackID), &data); err != nil {
		return nil, err
	}
	devices := make([]Device, len(data.Devices))
	for i, g := range data.Devices {
		devices[i] = g.toDevice()
	}
	return devices, nil
}

const deviceDetailQuery = `query {
  device_list(filters: {id: {exact: %d}}) {
    interfaces { name cable { id } connected_endpoints { ... on InterfaceType { name device { name } } } }
    powerports { name connected_endpoints { ... on PowerOutletType { name device { name } } } }
    poweroutlets { name connected_endpoints { ... on PowerPortType { name device { name } } } }
  }
}`

// naturalLess compares strings chunk-wise, treating digit runs as numbers,
// so "Te1/0/2" sorts before "Te1/0/10".
func naturalLess(a, b string) bool {
	for a != "" && b != "" {
		ca, ra, na := nextChunk(a)
		cb, rb, nb := nextChunk(b)
		if na && nb {
			ta, tb := strings.TrimLeft(ca, "0"), strings.TrimLeft(cb, "0")
			if len(ta) != len(tb) {
				return len(ta) < len(tb)
			}
			if ta != tb {
				return ta < tb
			}
		} else if ca != cb {
			return ca < cb
		}
		a, b = ra, rb
	}
	return len(a) < len(b)
}

// nextChunk splits off the leading run of digits or non-digits.
func nextChunk(s string) (chunk, rest string, digits bool) {
	digits = s[0] >= '0' && s[0] <= '9'
	i := 1
	for i < len(s) && (s[i] >= '0' && s[i] <= '9') == digits {
		i++
	}
	return s[:i], s[i:], digits
}

type gqlEndpoint struct {
	Name   string `json:"name"`
	Device struct {
		Name string `json:"name"`
	} `json:"device"`
}

func (e gqlEndpoint) toEndpoint() Endpoint {
	return Endpoint{Name: e.Name, Device: Named{Name: e.Device.Name}}
}

// DeviceDetail fetches cabled interfaces, power ports, and (for PDUs) power
// outlets in one query.
func (c *Client) DeviceDetail(ctx context.Context, deviceID int) ([]Interface, []PowerPort, []PowerOutlet, error) {
	var data struct {
		Devices []struct {
			Interfaces []struct {
				Name  string `json:"name"`
				Cable *struct {
					ID string `json:"id"`
				} `json:"cable"`
				Endpoints []gqlEndpoint `json:"connected_endpoints"`
			} `json:"interfaces"`
			PowerPorts []struct {
				Name      string        `json:"name"`
				Endpoints []gqlEndpoint `json:"connected_endpoints"`
			} `json:"powerports"`
			PowerOutlets []struct {
				Name      string        `json:"name"`
				Endpoints []gqlEndpoint `json:"connected_endpoints"`
			} `json:"poweroutlets"`
		} `json:"device_list"`
	}
	if err := c.gql(ctx, fmt.Sprintf(deviceDetailQuery, deviceID), &data); err != nil {
		return nil, nil, nil, err
	}
	if len(data.Devices) == 0 {
		return nil, nil, nil, fmt.Errorf("graphql: device %d not found", deviceID)
	}
	d := data.Devices[0]
	var ifaces []Interface
	for _, i := range d.Interfaces {
		if i.Cable == nil {
			continue // mimic the REST cabled=true filter
		}
		iface := Interface{Name: i.Name}
		for _, e := range i.Endpoints {
			iface.Endpoints = append(iface.Endpoints, e.toEndpoint())
		}
		ifaces = append(ifaces, iface)
	}
	// GraphQL returns interfaces in creation order; sort them like NetBox's
	// natural name ordering (Te1/0/2 before Te1/0/10).
	sort.SliceStable(ifaces, func(i, j int) bool { return naturalLess(ifaces[i].Name, ifaces[j].Name) })
	ports := make([]PowerPort, len(d.PowerPorts))
	for i, p := range d.PowerPorts {
		ports[i] = PowerPort{Name: p.Name}
		for _, e := range p.Endpoints {
			ports[i].Endpoints = append(ports[i].Endpoints, e.toEndpoint())
		}
	}
	outlets := make([]PowerOutlet, len(d.PowerOutlets))
	for i, o := range d.PowerOutlets {
		outlets[i] = PowerOutlet{Name: o.Name}
		for _, e := range o.Endpoints {
			outlets[i].Endpoints = append(outlets[i].Endpoints, e.toEndpoint())
		}
	}
	return ifaces, ports, outlets, nil
}
