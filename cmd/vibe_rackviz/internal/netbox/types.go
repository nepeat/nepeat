package netbox

// Minimal types matching the NetBox 4.5 REST shapes this tool consumes.
// Nested serializers in list responses are brief: device_type lacks u_height
// and role lacks color, so device spans are derived from elevation slots and
// colors from the device-roles endpoint.

type Named struct {
	ID   int    `json:"id"`
	Name string `json:"name"`
}

type valueLabel struct {
	Value string `json:"value"`
	Label string `json:"label"`
}

type Rack struct {
	ID          int    `json:"id"`
	Name        string `json:"name"`
	Site        Named  `json:"site"`
	UHeight     int    `json:"u_height"`
	DescUnits   bool   `json:"desc_units"`
	DeviceCount int    `json:"device_count"`
}

type Device struct {
	ID          int         `json:"id"`
	Name        string      `json:"name"`
	Role        Named       `json:"role"`
	DeviceType  DeviceType  `json:"device_type"`
	Position    *float64    `json:"position"`
	Face        *valueLabel `json:"face"`
	Status      valueLabel  `json:"status"`
	Serial      string      `json:"serial"`
	Description string      `json:"description"`
	PrimaryIP   *struct {
		Address string `json:"address"`
	} `json:"primary_ip"`
	// Parent is set for child devices installed in a device bay.
	Parent *ParentRef `json:"parent_device"`
}

type ParentRef struct {
	ID        int    `json:"id"`
	Name      string `json:"name"`
	DeviceBay struct {
		Name string `json:"name"`
	} `json:"device_bay"`
}

func (d Device) BayName() string {
	if d.Parent == nil {
		return ""
	}
	return d.Parent.DeviceBay.Name
}

func (d Device) FaceValue() string {
	if d.Face == nil {
		return ""
	}
	return d.Face.Value
}

type DeviceType struct {
	ID           int    `json:"id"`
	Model        string `json:"model"`
	Manufacturer Named  `json:"manufacturer"`
	// Populated by the GraphQL path only (REST brief serializers omit them).
	UHeight     float64 `json:"u_height"`
	IsFullDepth bool    `json:"is_full_depth"`
}

type DeviceRole struct {
	ID    int    `json:"id"`
	Name  string `json:"name"`
	Color string `json:"color"`
}

// Endpoint is one entry of connected_endpoints on an interface/power-port.
type Endpoint struct {
	ID     int    `json:"id"`
	Name   string `json:"name"`
	Device Named  `json:"device"`
}

type Interface struct {
	ID                 int        `json:"id"`
	Name               string     `json:"name"`
	Endpoints          []Endpoint `json:"connected_endpoints"`
	EndpointsReachable *bool      `json:"connected_endpoints_reachable"`
}

type PowerPort struct {
	ID        int        `json:"id"`
	Name      string     `json:"name"`
	Endpoints []Endpoint `json:"connected_endpoints"`
}

type PowerOutlet struct {
	ID          int         `json:"id"`
	Name        string      `json:"name"`
	Device      Named       `json:"device"`
	Type        *valueLabel `json:"type"`
	Description string      `json:"description"`
	// MarkConnected is NetBox's "treat as if a cable is connected" flag —
	// the connected canon for outlets feeding unmodeled loads.
	MarkConnected bool       `json:"mark_connected"`
	Endpoints     []Endpoint `json:"connected_endpoints"`
}

// IsConnected reports NetBox's canonical connected state: a real cable or
// the mark_connected flag.
func (o PowerOutlet) IsConnected() bool {
	return len(o.Endpoints) > 0 || o.MarkConnected
}

type Status struct {
	NetBoxVersion string `json:"netbox-version"`
}
