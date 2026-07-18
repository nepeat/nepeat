package pdu

import (
	"context"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/gosnmp/gosnmp"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/proxy"
)

// Raritan PX3 driver speaking PDU2-MIB (enterprise 13742.6) over SNMP v2c.
//
// OID layout (standalone unit → pduId 1, inlet 1):
//
//	switching op (rw): .1.3.6.1.4.1.13742.6.4.1.2.1.2.1.<outlet>   off(0) on(1) cycle(2)
//	outlet state:      .1.3.6.1.4.1.13742.6.5.4.3.1.3.1.<outlet>.14 (onOff sensor: on(7) off(8))
//	inlet pole meas:   .1.3.6.1.4.1.13742.6.5.2.4.1.4.1.1.<pole>.<sensor>
//	inlet meas:        .1.3.6.1.4.1.13742.6.5.2.3.1.4.1.1.<sensor>
//	pole digits:       .1.3.6.1.4.1.13742.6.3.3.6.1.7.1.1.<pole>.<sensor>
//	inlet digits:      .1.3.6.1.4.1.13742.6.3.3.4.1.7.1.1.<sensor>
//
// Sensor types: rmsCurrent(1), activePower(5). Raw values scale by 10^-digits.
const (
	oidSwitchingOp     = ".1.3.6.1.4.1.13742.6.4.1.2.1.2.1"
	oidOutletState     = ".1.3.6.1.4.1.13742.6.5.4.3.1.3.1"
	oidInletPoleValues = ".1.3.6.1.4.1.13742.6.5.2.4.1.4.1.1"
	oidInletValues     = ".1.3.6.1.4.1.13742.6.5.2.3.1.4.1.1"
	oidPoleDigits      = ".1.3.6.1.4.1.13742.6.3.3.6.1.7.1.1"
	oidInletDigits     = ".1.3.6.1.4.1.13742.6.3.3.4.1.7.1.1"
	oidOutletValues    = ".1.3.6.1.4.1.13742.6.5.4.3.1.4.1"
	oidOutletDigits    = ".1.3.6.1.4.1.13742.6.3.5.4.1.7.1"

	sensorOnOff      = 14
	sensorRMSCurrent = 1
	sensorActivePow  = 5

	opOff   = 0
	opOn    = 1
	opCycle = 2

	stateSensorOn  = 7
	stateSensorOff = 8
)

type raritan struct {
	name      string
	host      string
	community string
}

func newRaritan(name, host, community string) Controller {
	return &raritan{name: name, host: host, community: community}
}

func (r *raritan) Name() string { return r.name }
func (r *raritan) Caps() Caps   { return CapSwitch | CapMeter }

// session opens an SNMP connection to the PDU, relayed through a SOCKS5
// proxy when $PROXY is one (gosnmp is pointed at a local UDP forwarder).
// The returned func closes the connection and any forwarder.
func (r *raritan) session(ctx context.Context) (*gosnmp.GoSNMP, func(), error) {
	target, port := r.host, uint16(161)
	cleanup := func() {}
	if u, err := proxy.FromEnv(); err != nil {
		return nil, nil, err
	} else if proxy.IsSOCKS(u) {
		fwd, err := proxy.DialUDPVia(u, r.host, 161)
		if err != nil {
			return nil, nil, fmt.Errorf("%s: %w", r.name, err)
		}
		target, port = "127.0.0.1", fwd.LocalPort()
		cleanup = fwd.Close
	}
	g := &gosnmp.GoSNMP{
		Target:    target,
		Port:      port,
		Community: r.community,
		Version:   gosnmp.Version2c,
		Timeout:   4 * time.Second,
		Retries:   1,
		Context:   ctx,
	}
	if err := g.Connect(); err != nil {
		cleanup()
		return nil, nil, fmt.Errorf("%s: snmp connect: %w", r.name, err)
	}
	return g, func() { g.Conn.Close(); cleanup() }, nil
}

func (r *raritan) OutletState(ctx context.Context, outlet int) (OutletState, error) {
	g, done, err := r.session(ctx)
	if err != nil {
		return StateUnknown, err
	}
	defer done()
	oid := fmt.Sprintf("%s.%d.%d", oidOutletState, outlet, sensorOnOff)
	res, err := g.Get([]string{oid})
	if err != nil {
		return StateUnknown, fmt.Errorf("%s outlet %d state: %w", r.name, outlet, err)
	}
	if len(res.Variables) == 0 {
		return StateUnknown, fmt.Errorf("%s outlet %d: empty response", r.name, outlet)
	}
	switch gosnmp.ToBigInt(res.Variables[0].Value).Int64() {
	case stateSensorOn:
		return StateOn, nil
	case stateSensorOff:
		return StateOff, nil
	}
	return StateUnknown, nil
}

// OutletStates walks the onOff sensor column for every outlet in one sweep
// (the SNMP walk is a single operation, so the narrowing hint is ignored).
func (r *raritan) OutletStates(ctx context.Context, _ []int) (map[int]OutletState, error) {
	g, done, err := r.session(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	raw, err := walkInts(g, oidOutletState)
	if err != nil {
		return nil, fmt.Errorf("%s outlet states: %w", r.name, err)
	}
	out := map[int]OutletState{}
	for key, v := range raw {
		outlet, sensor, ok := splitIndex2(key)
		if !ok || sensor != sensorOnOff {
			continue
		}
		switch v {
		case stateSensorOn:
			out[outlet] = StateOn
		case stateSensorOff:
			out[outlet] = StateOff
		default:
			out[outlet] = StateUnknown
		}
	}
	return out, nil
}

func (r *raritan) switchOp(ctx context.Context, outlet, op int) error {
	g, done, err := r.session(ctx)
	if err != nil {
		return err
	}
	defer done()
	oid := fmt.Sprintf("%s.%d", oidSwitchingOp, outlet)
	_, err = g.Set([]gosnmp.SnmpPDU{{Name: oid, Type: gosnmp.Integer, Value: op}})
	if err != nil {
		return fmt.Errorf("%s outlet %d op %d: %w", r.name, outlet, op, err)
	}
	return nil
}

func (r *raritan) PowerOn(ctx context.Context, outlet int) error {
	return r.switchOp(ctx, outlet, opOn)
}

func (r *raritan) PowerOff(ctx context.Context, outlet int) error {
	return r.switchOp(ctx, outlet, opOff)
}

func (r *raritan) PowerCycle(ctx context.Context, outlet int) error {
	return r.switchOp(ctx, outlet, opCycle)
}

// OutletReading fetches one outlet's rmsCurrent + activePower with their
// decimal-digit scaling.
func (r *raritan) OutletReading(ctx context.Context, outlet int) (PowerReading, error) {
	g, done, err := r.session(ctx)
	if err != nil {
		return PowerReading{}, err
	}
	defer done()
	oids := []string{
		fmt.Sprintf("%s.%d.%d", oidOutletValues, outlet, sensorRMSCurrent),
		fmt.Sprintf("%s.%d.%d", oidOutletValues, outlet, sensorActivePow),
		fmt.Sprintf("%s.%d.%d", oidOutletDigits, outlet, sensorRMSCurrent),
		fmt.Sprintf("%s.%d.%d", oidOutletDigits, outlet, sensorActivePow),
	}
	res, err := g.Get(oids)
	if err != nil {
		return PowerReading{}, fmt.Errorf("%s outlet %d reading: %w", r.name, outlet, err)
	}
	if len(res.Variables) != 4 {
		return PowerReading{}, fmt.Errorf("%s outlet %d reading: short response", r.name, outlet)
	}
	v := func(i int) int64 { return gosnmp.ToBigInt(res.Variables[i].Value).Int64() }
	return PowerReading{
		Label: fmt.Sprintf("outlet %d", outlet),
		Amps:  float64(v(0)) / math.Pow10(int(v(2))),
		Watts: float64(v(1)) / math.Pow10(int(v(3))),
	}, nil
}

// OutletReadings fetches many outlets' draw over one SNMP session.
func (r *raritan) OutletReadings(ctx context.Context, outlets []int) (map[int]PowerReading, error) {
	g, done, err := r.session(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	out := map[int]PowerReading{}
	for _, outlet := range outlets {
		oids := []string{
			fmt.Sprintf("%s.%d.%d", oidOutletValues, outlet, sensorRMSCurrent),
			fmt.Sprintf("%s.%d.%d", oidOutletValues, outlet, sensorActivePow),
			fmt.Sprintf("%s.%d.%d", oidOutletDigits, outlet, sensorRMSCurrent),
			fmt.Sprintf("%s.%d.%d", oidOutletDigits, outlet, sensorActivePow),
		}
		res, err := g.Get(oids)
		if err != nil || len(res.Variables) != 4 {
			continue
		}
		v := func(i int) int64 { return gosnmp.ToBigInt(res.Variables[i].Value).Int64() }
		out[outlet] = PowerReading{
			Label: fmt.Sprintf("outlet %d", outlet),
			Amps:  float64(v(0)) / math.Pow10(int(v(2))),
			Watts: float64(v(1)) / math.Pow10(int(v(3))),
		}
	}
	return out, nil
}

// Readings walks per-pole current+power on inlet 1 plus the inlet totals.
func (r *raritan) Readings(ctx context.Context) ([]PowerReading, error) {
	g, done, err := r.session(ctx)
	if err != nil {
		return nil, err
	}
	defer done()

	poleValues, err := walkInts(g, oidInletPoleValues)
	if err != nil {
		return nil, fmt.Errorf("%s pole values: %w", r.name, err)
	}
	poleDigits, _ := walkInts(g, oidPoleDigits)
	inletValues, err := walkInts(g, oidInletValues)
	if err != nil {
		return nil, fmt.Errorf("%s inlet values: %w", r.name, err)
	}
	inletDigits, _ := walkInts(g, oidInletDigits)

	scale := func(raw, digits int64) float64 {
		return float64(raw) / math.Pow10(int(digits))
	}

	// poleValues keys are "<pole>.<sensorType>".
	poles := map[int]*PowerReading{}
	for key, raw := range poleValues {
		pole, sensor, ok := splitIndex2(key)
		if !ok {
			continue
		}
		pr := poles[pole]
		if pr == nil {
			pr = &PowerReading{Label: fmt.Sprintf("L%d", pole)}
			poles[pole] = pr
		}
		switch sensor {
		case sensorRMSCurrent:
			pr.Amps = scale(raw, poleDigits[key])
		case sensorActivePow:
			pr.Watts = scale(raw, poleDigits[key])
		}
	}

	var out []PowerReading
	var poleNums []int
	for p := range poles {
		poleNums = append(poleNums, p)
	}
	sort.Ints(poleNums)
	for _, p := range poleNums {
		out = append(out, *poles[p])
	}

	total := PowerReading{Label: "Total"}
	for key, raw := range inletValues {
		sensor, err := strconv.Atoi(key)
		if err != nil {
			continue
		}
		switch sensor {
		case sensorRMSCurrent:
			total.Amps = scale(raw, inletDigits[key])
		case sensorActivePow:
			total.Watts = scale(raw, inletDigits[key])
		}
	}
	return append(out, total), nil
}

// walkInts bulk-walks base and returns {index-suffix: integer value}.
func walkInts(g *gosnmp.GoSNMP, base string) (map[string]int64, error) {
	out := map[string]int64{}
	err := g.BulkWalk(base, func(pdu gosnmp.SnmpPDU) error {
		suffix := strings.TrimPrefix(strings.TrimPrefix(pdu.Name, base), ".")
		out[suffix] = gosnmp.ToBigInt(pdu.Value).Int64()
		return nil
	})
	return out, err
}

func splitIndex2(key string) (int, int, bool) {
	parts := strings.Split(key, ".")
	if len(parts) != 2 {
		return 0, 0, false
	}
	a, err1 := strconv.Atoi(parts[0])
	b, err2 := strconv.Atoi(parts[1])
	if err1 != nil || err2 != nil {
		return 0, 0, false
	}
	return a, b, true
}
