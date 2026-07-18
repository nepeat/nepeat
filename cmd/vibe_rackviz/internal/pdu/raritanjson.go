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

type raritanJSON struct {
	name string
	base string // https://host
	user string
	pass string
	http *http.Client
}

func newRaritanJSON(name, host, user, pass string, tlsVerify bool) Controller {
	base := host
	if !strings.Contains(base, "://") {
		base = "https://" + base
	}
	return &raritanJSON{
		name: name,
		base: strings.TrimRight(base, "/"),
		user: user,
		pass: pass,
		http: &http.Client{
			Timeout:   15 * time.Second,
			Transport: proxy.HTTPTransport(!tlsVerify),
		},
	}
}

func (r *raritanJSON) Name() string { return r.name }
func (r *raritanJSON) Caps() Caps   { return CapSwitch | CapMeter }

// call performs one JSON-RPC request against rid and decodes result._ret_
// into out (out may be nil for methods without a useful return).
func (r *raritanJSON) call(ctx context.Context, rid, method string, params, out any) error {
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
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, r.base+rid, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.SetBasicAuth(r.user, r.pass)
	req.Header.Set("Content-Type", "application/json")
	resp, err := r.http.Do(req)
	if err != nil {
		return fmt.Errorf("%s: %s %s: %w", r.name, method, rid, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 256))
		return fmt.Errorf("%s: %s %s: %s: %s", r.name, method, rid, resp.Status, strings.TrimSpace(string(b)))
	}
	var envelope struct {
		Result json.RawMessage `json:"result"`
		Error  *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&envelope); err != nil {
		return fmt.Errorf("%s: %s %s: bad response: %w", r.name, method, rid, err)
	}
	if envelope.Error != nil {
		return fmt.Errorf("%s: %s %s: rpc error %d: %s", r.name, method, rid, envelope.Error.Code, envelope.Error.Message)
	}
	if out == nil {
		return nil
	}
	var ret struct {
		Ret json.RawMessage `json:"_ret_"`
	}
	if err := json.Unmarshal(envelope.Result, &ret); err != nil {
		return fmt.Errorf("%s: %s %s: bad result: %w", r.name, method, rid, err)
	}
	if ret.Ret == nil {
		return nil
	}
	return json.Unmarshal(ret.Ret, out)
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

// OutletStates fetches every outlet's state: getOutlets for the outlet count
// (the returned rids are opaque on real firmware — /tfwopaque/… — so only the
// count is used), then direct getState calls with bounded concurrency.
// Validated against PX3 Xerus 3.x, which has no /bulk endpoint.
func (r *raritanJSON) OutletStates(ctx context.Context) (map[int]OutletState, error) {
	var outlets []struct {
		Rid string `json:"rid"`
	}
	if err := r.call(ctx, "/model/pdu/0", "getOutlets", nil, &outlets); err != nil {
		return nil, err
	}
	n := len(outlets)
	if n == 0 {
		return map[int]OutletState{}, nil
	}
	states := make([]OutletState, n)
	errs := make([]error, n)
	sem := make(chan struct{}, 8)
	var wg sync.WaitGroup
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			states[i], errs[i] = r.OutletState(ctx, i+1)
		}(i)
	}
	wg.Wait()
	out := map[int]OutletState{}
	failed := 0
	for i := 0; i < n; i++ {
		if errs[i] != nil {
			failed++
			continue
		}
		out[i+1] = states[i]
	}
	if failed == n {
		return nil, fmt.Errorf("%s: all %d outlet state queries failed: %w", r.name, n, errs[0])
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
