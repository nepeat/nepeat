package pdu

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"golang.org/x/sync/errgroup"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/proxy"
)

// Raritan PX2/PX3 (Xerus) JSON-RPC driver over HTTPS. Everything is TCP, so
// it works through PROXY/HTTPS_PROXY — including plain `ssh -D` — unlike the
// SNMP driver, whose UDP needs a SOCKS proxy with UDP ASSOCIATE.
//
// Object model (Raritan JSON-RPC SDK): POST {"jsonrpc":"2.0","method":M,
// "params":P,"id":N} to https://<pdu><rid>; results come back wrapped in
// result._ret_. Outlet rids are 0-based: NetBox "output6" → /model/pdu/0/outlet/5.
//
//	/model/pdu/0/outlet/<n>: getState, setPowerState({"pstate":0|1}), cyclePowerState
//	/model/pdu/0/inlet/0:    getSensors (total sensors), getPoles (per-leg)
//	sensor rid:              getReading → {valid, value}
const (
	psOff = 0
	psOn  = 1
)

type outletSensorRefs struct {
	current string
	power   string
}

type raritanJSON struct {
	name string
	base string // https://host
	user string
	pass string
	http *http.Client

	mu         sync.Mutex
	sensorRids map[int]outletSensorRefs
}

func newRaritanJSON(name, host, user, pass string, tlsVerify bool) Controller {
	base := host
	if !strings.Contains(base, "://") {
		base = "https://" + base
	}
	return &raritanJSON{
		name:       name,
		base:       strings.TrimRight(base, "/"),
		user:       user,
		pass:       pass,
		sensorRids: map[int]outletSensorRefs{},
		http: &http.Client{
			Timeout:   15 * time.Second,
			Transport: proxy.HTTPTransport(!tlsVerify),
		},
	}
}

func (r *raritanJSON) Name() string { return r.name }
func (r *raritanJSON) Caps() Caps   { return CapSwitch | CapMeter }

// callResult performs one JSON-RPC request against rid and returns the raw
// result object.
func (r *raritanJSON) callResult(ctx context.Context, rid, method string, params any) (json.RawMessage, error) {
	if params == nil {
		params = struct{}{}
	}
	body, err := json.Marshal(map[string]any{
		"jsonrpc": "2.0",
		"method":  method,
		"params":  params,
		"id":      1,
	})
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, r.base+rid, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.SetBasicAuth(r.user, r.pass)
	req.Header.Set("Content-Type", "application/json")
	resp, err := r.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("%s: %s %s: %w", r.name, method, rid, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 256))
		return nil, fmt.Errorf("%s: %s %s: %s: %s", r.name, method, rid, resp.Status, strings.TrimSpace(string(b)))
	}
	var envelope struct {
		Result json.RawMessage `json:"result"`
		Error  *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&envelope); err != nil {
		return nil, fmt.Errorf("%s: %s %s: bad response: %w", r.name, method, rid, err)
	}
	if envelope.Error != nil {
		return nil, fmt.Errorf("%s: %s %s: rpc error %d: %s", r.name, method, rid, envelope.Error.Code, envelope.Error.Message)
	}
	return envelope.Result, nil
}

// call performs one JSON-RPC request against rid and decodes result._ret_
// into out (out may be nil for methods without a useful return).
func (r *raritanJSON) call(ctx context.Context, rid, method string, params, out any) error {
	result, err := r.callResult(ctx, rid, method, params)
	if err != nil {
		return err
	}
	if out == nil {
		return nil
	}
	var ret struct {
		Ret json.RawMessage `json:"_ret_"`
	}
	if err := json.Unmarshal(result, &ret); err != nil {
		return fmt.Errorf("%s: %s %s: bad result: %w", r.name, method, rid, err)
	}
	if ret.Ret == nil {
		return nil
	}
	return json.Unmarshal(ret.Ret, out)
}

// bulkItem is one batched JSON-RPC call for performBulk.
type bulkItem struct {
	Rid  string         `json:"rid"`
	JSON map[string]any `json:"json"`
}

func bulkCall(rid, method string, id int) bulkItem {
	return bulkItem{Rid: rid, JSON: map[string]any{
		"jsonrpc": "2.0", "method": method, "params": nil, "id": id,
	}}
}

// performBulk executes many JSON-RPC calls in a single round trip (the stock
// web UI's batching endpoint: POST /bulk with a performBulk envelope).
// Returns one raw _ret_ per item, nil for items that individually failed.
func (r *raritanJSON) performBulk(ctx context.Context, items []bulkItem) ([]json.RawMessage, error) {
	result, err := r.callResult(ctx, "/bulk", "performBulk", map[string]any{"requests": items})
	if err != nil {
		return nil, err
	}
	var out struct {
		Responses []struct {
			JSON struct {
				Result struct {
					Ret json.RawMessage `json:"_ret_"`
				} `json:"result"`
				Error *struct {
					Message string `json:"message"`
				} `json:"error"`
			} `json:"json"`
			Statcode int `json:"statcode"`
		} `json:"responses"`
	}
	if err := json.Unmarshal(result, &out); err != nil {
		return nil, fmt.Errorf("%s: performBulk: bad result: %w", r.name, err)
	}
	if len(out.Responses) != len(items) {
		return nil, fmt.Errorf("%s: performBulk: %d responses for %d requests", r.name, len(out.Responses), len(items))
	}
	rets := make([]json.RawMessage, len(items))
	for i, rsp := range out.Responses {
		if rsp.JSON.Error != nil || (rsp.Statcode != 0 && rsp.Statcode != http.StatusOK) {
			continue
		}
		rets[i] = rsp.JSON.Result.Ret
	}
	return rets, nil
}

// outletRid maps the 1-based NetBox outlet number to the 0-based rid.
func outletRid(outlet int) string {
	return fmt.Sprintf("/model/pdu/0/outlet/%d", outlet-1)
}

func (r *raritanJSON) OutletState(ctx context.Context, outlet int) (OutletState, error) {
	var st struct {
		Available  bool `json:"available"`
		PowerState int  `json:"powerState"`
	}
	if err := r.call(ctx, outletRid(outlet), "getState", nil, &st); err != nil {
		return StateUnknown, err
	}
	switch st.PowerState {
	case psOn:
		return StateOn, nil
	case psOff:
		return StateOff, nil
	}
	return StateUnknown, nil
}

// OutletStates fetches outlet states — one performBulk round trip, with a
// bounded-concurrency fan-out fallback for firmware without /bulk. When the
// narrowing hint is empty, getOutlets supplies the outlet count.
func (r *raritanJSON) OutletStates(ctx context.Context, outlets []int) (map[int]OutletState, error) {
	if len(outlets) == 0 {
		var refs []struct {
			Rid string `json:"rid"`
		}
		if err := r.call(ctx, "/model/pdu/0", "getOutlets", nil, &refs); err != nil {
			return nil, err
		}
		for i := range refs {
			outlets = append(outlets, i+1)
		}
	}
	if len(outlets) == 0 {
		return map[int]OutletState{}, nil
	}

	items := make([]bulkItem, len(outlets))
	for i, outlet := range outlets {
		items[i] = bulkCall(outletRid(outlet), "getState", i+1)
	}
	rets, err := r.performBulk(ctx, items)
	if err != nil {
		return r.outletStatesFanout(ctx, outlets)
	}
	out := map[int]OutletState{}
	for i, ret := range rets {
		if ret == nil {
			continue
		}
		var st struct {
			PowerState int `json:"powerState"`
		}
		if json.Unmarshal(ret, &st) != nil {
			continue
		}
		switch st.PowerState {
		case psOn:
			out[outlets[i]] = StateOn
		case psOff:
			out[outlets[i]] = StateOff
		default:
			out[outlets[i]] = StateUnknown
		}
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("%s: bulk getState returned no usable states", r.name)
	}
	return out, nil
}

func (r *raritanJSON) outletStatesFanout(ctx context.Context, outlets []int) (map[int]OutletState, error) {
	states := make([]OutletState, len(outlets))
	errs := make([]error, len(outlets))
	sem := make(chan struct{}, 4)
	var wg sync.WaitGroup
	for i, outlet := range outlets {
		wg.Add(1)
		go func(i, outlet int) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			states[i], errs[i] = r.OutletState(ctx, outlet)
		}(i, outlet)
	}
	wg.Wait()
	out := map[int]OutletState{}
	failed := 0
	for i, outlet := range outlets {
		if errs[i] != nil {
			failed++
			continue
		}
		out[outlet] = states[i]
	}
	if failed == len(outlets) {
		return nil, fmt.Errorf("%s: all %d outlet state queries failed: %w", r.name, len(outlets), errs[0])
	}
	return out, nil
}

// ensureSensorRids resolves (and caches) each outlet's current/activePower
// sensor rids — they're stable, so getSensors only ever runs once per outlet.
func (r *raritanJSON) ensureSensorRids(ctx context.Context, outlets []int) (map[int]outletSensorRefs, error) {
	r.mu.Lock()
	var missing []int
	for _, o := range outlets {
		if _, ok := r.sensorRids[o]; !ok {
			missing = append(missing, o)
		}
	}
	r.mu.Unlock()

	if len(missing) > 0 {
		items := make([]bulkItem, len(missing))
		for i, o := range missing {
			items[i] = bulkCall(outletRid(o), "getSensors", i+1)
		}
		rets, err := r.performBulk(ctx, items)
		if err != nil {
			return nil, err
		}
		r.mu.Lock()
		for i, ret := range rets {
			if ret == nil {
				continue
			}
			var sensors struct {
				Current     *sensorRef `json:"current"`
				RMSCurrent  *sensorRef `json:"rmsCurrent"`
				ActivePower *sensorRef `json:"activePower"`
			}
			if json.Unmarshal(ret, &sensors) != nil {
				continue
			}
			refs := outletSensorRefs{}
			if sensors.Current != nil {
				refs.current = sensors.Current.Rid
			} else if sensors.RMSCurrent != nil {
				refs.current = sensors.RMSCurrent.Rid
			}
			if sensors.ActivePower != nil {
				refs.power = sensors.ActivePower.Rid
			}
			r.sensorRids[missing[i]] = refs
		}
		r.mu.Unlock()
	}

	out := map[int]outletSensorRefs{}
	r.mu.Lock()
	for _, o := range outlets {
		if refs, ok := r.sensorRids[o]; ok {
			out[o] = refs
		}
	}
	r.mu.Unlock()
	return out, nil
}

// OutletReadings fetches live W/A for many outlets: one bulk to discover any
// uncached sensor rids, one bulk for all the readings.
func (r *raritanJSON) OutletReadings(ctx context.Context, outlets []int) (map[int]PowerReading, error) {
	if len(outlets) == 0 {
		return map[int]PowerReading{}, nil
	}
	refs, err := r.ensureSensorRids(ctx, outlets)
	if err != nil {
		return r.outletReadingsFanout(ctx, outlets)
	}
	type slot struct {
		outlet int
		watts  bool
	}
	var items []bulkItem
	var slots []slot
	for _, o := range outlets {
		ref, ok := refs[o]
		if !ok {
			continue
		}
		if ref.current != "" {
			items = append(items, bulkCall(ref.current, "getReading", len(items)+1))
			slots = append(slots, slot{outlet: o})
		}
		if ref.power != "" {
			items = append(items, bulkCall(ref.power, "getReading", len(items)+1))
			slots = append(slots, slot{outlet: o, watts: true})
		}
	}
	if len(items) == 0 {
		return map[int]PowerReading{}, nil
	}
	rets, err := r.performBulk(ctx, items)
	if err != nil {
		return r.outletReadingsFanout(ctx, outlets)
	}
	out := map[int]PowerReading{}
	for i, ret := range rets {
		if ret == nil {
			continue
		}
		var rd struct {
			Valid bool    `json:"valid"`
			Value float64 `json:"value"`
		}
		if json.Unmarshal(ret, &rd) != nil || !rd.Valid {
			continue
		}
		pr := out[slots[i].outlet]
		pr.Label = fmt.Sprintf("outlet %d", slots[i].outlet)
		if slots[i].watts {
			pr.Watts = rd.Value
		} else {
			pr.Amps = rd.Value
		}
		out[slots[i].outlet] = pr
	}
	return out, nil
}

func (r *raritanJSON) outletReadingsFanout(ctx context.Context, outlets []int) (map[int]PowerReading, error) {
	out := map[int]PowerReading{}
	var firstErr error
	for _, o := range outlets {
		pr, err := r.OutletReading(ctx, o)
		if err != nil {
			if firstErr == nil {
				firstErr = err
			}
			continue
		}
		out[o] = pr
	}
	if len(out) == 0 && firstErr != nil {
		return nil, firstErr
	}
	return out, nil
}

func (r *raritanJSON) PowerOn(ctx context.Context, outlet int) error {
	return r.call(ctx, outletRid(outlet), "setPowerState", map[string]int{"pstate": psOn}, nil)
}

func (r *raritanJSON) PowerOff(ctx context.Context, outlet int) error {
	return r.call(ctx, outletRid(outlet), "setPowerState", map[string]int{"pstate": psOff}, nil)
}

func (r *raritanJSON) PowerCycle(ctx context.Context, outlet int) error {
	return r.call(ctx, outletRid(outlet), "cyclePowerState", nil, nil)
}

// sensorRef is an object reference to a sensors.NumericSensor.
type sensorRef struct {
	Rid string `json:"rid"`
}

func (r *raritanJSON) reading(ctx context.Context, ref *sensorRef) (float64, bool) {
	if ref == nil || ref.Rid == "" {
		return 0, false
	}
	var rd struct {
		Valid bool    `json:"valid"`
		Value float64 `json:"value"`
	}
	if err := r.call(ctx, ref.Rid, "getReading", nil, &rd); err != nil || !rd.Valid {
		return 0, false
	}
	return rd.Value, true
}

// OutletReading fetches one outlet's live draw via its sensor objects
// (getSensors on the outlet, then getReading on current/activePower).
func (r *raritanJSON) OutletReading(ctx context.Context, outlet int) (PowerReading, error) {
	var sensors struct {
		Current     *sensorRef `json:"current"`
		RMSCurrent  *sensorRef `json:"rmsCurrent"`
		ActivePower *sensorRef `json:"activePower"`
	}
	if err := r.call(ctx, outletRid(outlet), "getSensors", nil, &sensors); err != nil {
		return PowerReading{}, err
	}
	pr := PowerReading{Label: fmt.Sprintf("outlet %d", outlet)}
	cur := sensors.Current
	if cur == nil {
		cur = sensors.RMSCurrent
	}
	r.readAll(ctx, []readJob{
		{ref: cur, dst: &pr.Amps},
		{ref: sensors.ActivePower, dst: &pr.Watts},
	})
	return pr, nil
}

// Readings returns per-pole current/power (when the PDU exposes poles) plus
// the inlet totals. The sensor discovery calls run in parallel, then every
// getReading fires in one bounded wave.
func (r *raritanJSON) Readings(ctx context.Context) ([]PowerReading, error) {
	const inletRid = "/model/pdu/0/inlet/0"

	var poles []struct {
		Current     *sensorRef `json:"current"`
		ActivePower *sensorRef `json:"activePower"`
	}
	var sensors struct {
		Current     *sensorRef `json:"current"`
		RMSCurrent  *sensorRef `json:"rmsCurrent"`
		ActivePower *sensorRef `json:"activePower"`
	}
	g, gctx := errgroup.WithContext(ctx)
	g.Go(func() error {
		// Not all models expose poles — non-fatal.
		_ = r.call(gctx, inletRid, "getPoles", nil, &poles)
		return nil
	})
	g.Go(func() error { return r.call(gctx, inletRid, "getSensors", nil, &sensors) })
	if err := g.Wait(); err != nil {
		return nil, err
	}

	readings := make([]PowerReading, len(poles)+1)
	var jobs []readJob
	for i, p := range poles {
		readings[i].Label = fmt.Sprintf("L%d", i+1)
		jobs = append(jobs,
			readJob{ref: p.Current, dst: &readings[i].Amps},
			readJob{ref: p.ActivePower, dst: &readings[i].Watts})
	}
	total := &readings[len(poles)]
	total.Label = "Total"
	cur := sensors.Current
	if cur == nil {
		cur = sensors.RMSCurrent
	}
	jobs = append(jobs,
		readJob{ref: cur, dst: &total.Amps},
		readJob{ref: sensors.ActivePower, dst: &total.Watts})
	r.readAll(ctx, jobs)

	out := make([]PowerReading, 0, len(readings))
	for i, pr := range readings {
		if i < len(poles) && pr.Amps == 0 && pr.Watts == 0 {
			continue
		}
		out = append(out, pr)
	}
	return out, nil
}

// readJob is one getReading call whose value lands in dst.
type readJob struct {
	ref *sensorRef
	dst *float64
}

// readAll runs every sensor read concurrently (bounded); failed or invalid
// readings simply leave their destination at zero.
func (r *raritanJSON) readAll(ctx context.Context, jobs []readJob) {
	sem := make(chan struct{}, 8)
	var wg sync.WaitGroup
	for _, j := range jobs {
		if j.ref == nil || j.ref.Rid == "" {
			continue
		}
		wg.Add(1)
		go func(j readJob) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			if v, ok := r.reading(ctx, j.ref); ok {
				*j.dst = v
			}
		}(j)
	}
	wg.Wait()
}
