package pdu

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"
)

// fakePX3 speaks just enough Raritan JSON-RPC for the driver: outlet 6
// (rid index 5) with switchable state, inlet totals, and two poles.
type fakePX3 struct {
	mu         sync.Mutex
	powerState int
	cycles     int
	lastAuth   string
}

func (f *fakePX3) handler() http.HandlerFunc {
	rpc := func(w http.ResponseWriter, ret any) {
		json.NewEncoder(w).Encode(map[string]any{
			"jsonrpc": "2.0",
			"id":      1,
			"result":  map[string]any{"_ret_": ret},
		})
	}
	readings := map[string]float64{
		"/sensor/inlet-current": 11.5,
		"/sensor/inlet-power":   2640,
		"/sensor/l1-current":    5.2,
		"/sensor/l1-power":      1200,
		"/sensor/l2-current":    6.3,
		"/sensor/l2-power":      1440,
		"/sensor/out5-current":  0.38,
		"/sensor/out5-power":    45.2,
	}
	return func(w http.ResponseWriter, r *http.Request) {
		user, pass, _ := r.BasicAuth()
		f.mu.Lock()
		f.lastAuth = user + ":" + pass
		f.mu.Unlock()
		var req struct {
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if r.URL.Path == "/model/pdu/0" && req.Method == "getOutlets" {
			refs := make([]map[string]string, 8)
			for i := range refs {
				refs[i] = map[string]string{"rid": fmt.Sprintf("/model/pdu/0/outlet/%d", i)}
			}
			rpc(w, refs)
			return
		}
		if v, ok := readings[r.URL.Path]; ok && req.Method == "getReading" {
			rpc(w, map[string]any{"valid": true, "value": v})
			return
		}
		// getState works on outlets 0..7; outlet 5 (0-based) mirrors the
		// switchable state, the rest are on. Out-of-range falls through to
		// the rpc-error default like real firmware.
		if req.Method == "getState" && strings.HasPrefix(r.URL.Path, "/model/pdu/0/outlet/") {
			idx, err := strconv.Atoi(strings.TrimPrefix(r.URL.Path, "/model/pdu/0/outlet/"))
			if err == nil && idx >= 0 && idx < 8 {
				st := 1
				if idx == 5 {
					f.mu.Lock()
					st = f.powerState
					f.mu.Unlock()
				}
				rpc(w, map[string]any{"available": true, "powerState": st})
				return
			}
		}
		switch r.URL.Path + " " + req.Method {
		case "/model/pdu/0/outlet/5 setPowerState":
			var p struct {
				Pstate int `json:"pstate"`
			}
			json.Unmarshal(req.Params, &p)
			f.mu.Lock()
			f.powerState = p.Pstate
			f.mu.Unlock()
			rpc(w, nil)
		case "/model/pdu/0/outlet/5 cyclePowerState":
			f.mu.Lock()
			f.cycles++
			f.mu.Unlock()
			rpc(w, nil)
		case "/model/pdu/0/outlet/5 getSensors":
			rpc(w, map[string]any{
				"current":     map[string]string{"rid": "/sensor/out5-current"},
				"activePower": map[string]string{"rid": "/sensor/out5-power"},
			})
		case "/model/pdu/0/inlet/0 getSensors":
			rpc(w, map[string]any{
				"current":     map[string]string{"rid": "/sensor/inlet-current"},
				"activePower": map[string]string{"rid": "/sensor/inlet-power"},
			})
		case "/model/pdu/0/inlet/0 getPoles":
			rpc(w, []map[string]any{
				{"current": map[string]string{"rid": "/sensor/l1-current"}, "activePower": map[string]string{"rid": "/sensor/l1-power"}},
				{"current": map[string]string{"rid": "/sensor/l2-current"}, "activePower": map[string]string{"rid": "/sensor/l2-power"}},
			})
		default:
			json.NewEncoder(w).Encode(map[string]any{
				"jsonrpc": "2.0", "id": 1,
				"error": map[string]any{"code": -32601, "message": "no such method " + req.Method + " on " + r.URL.Path},
			})
		}
	}
}

func TestRaritanJSONDriver(t *testing.T) {
	fake := &fakePX3{powerState: psOn}
	srv := httptest.NewTLSServer(fake.handler())
	defer srv.Close()

	c := newRaritanJSON("dma-pdu-01", srv.URL, "admin", "kitty", false)
	ctx := context.Background()

	if c.Caps() != CapSwitch|CapMeter {
		t.Fatalf("caps = %v", c.Caps())
	}

	st, err := c.OutletState(ctx, 6)
	if err != nil {
		t.Fatal(err)
	}
	if st != StateOn {
		t.Errorf("state = %v, want on", st)
	}
	if fake.lastAuth != "admin:kitty" {
		t.Errorf("basic auth = %q", fake.lastAuth)
	}

	if err := c.PowerOff(ctx, 6); err != nil {
		t.Fatal(err)
	}
	if st, _ := c.OutletState(ctx, 6); st != StateOff {
		t.Errorf("state after off = %v, want off", st)
	}
	if err := c.PowerOn(ctx, 6); err != nil {
		t.Fatal(err)
	}
	if st, _ := c.OutletState(ctx, 6); st != StateOn {
		t.Errorf("state after on = %v, want on", st)
	}
	if err := c.PowerCycle(ctx, 6); err != nil {
		t.Fatal(err)
	}
	if fake.cycles != 1 {
		t.Errorf("cycles = %d, want 1", fake.cycles)
	}

	// Wrong outlet → rpc error surfaces.
	if _, err := c.OutletState(ctx, 25); err == nil {
		t.Error("outlet 25 should error")
	}

	// Bulk state sweep: outlet 6 mirrors f.powerState, others on.
	fake.mu.Lock()
	fake.powerState = psOff
	fake.mu.Unlock()
	states, err := c.OutletStates(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if len(states) != 8 {
		t.Fatalf("states = %d entries, want 8", len(states))
	}
	if states[6] != StateOff {
		t.Errorf("outlet 6 = %v, want off", states[6])
	}
	if states[1] != StateOn {
		t.Errorf("outlet 1 = %v, want on", states[1])
	}
	fake.mu.Lock()
	fake.powerState = psOn
	fake.mu.Unlock()

	// Per-outlet draw.
	or, err := c.OutletReading(ctx, 6)
	if err != nil {
		t.Fatal(err)
	}
	if or.Watts != 45.2 || or.Amps != 0.38 {
		t.Errorf("outlet reading = %+v, want 45.2W/0.38A", or)
	}

	rds, err := c.Readings(ctx)
	if err != nil {
		t.Fatal(err)
	}
	want := []PowerReading{
		{Label: "L1", Watts: 1200, Amps: 5.2},
		{Label: "L2", Watts: 1440, Amps: 6.3},
		{Label: "Total", Watts: 2640, Amps: 11.5},
	}
	if len(rds) != len(want) {
		t.Fatalf("readings = %+v, want %+v", rds, want)
	}
	for i := range want {
		if rds[i] != want[i] {
			t.Errorf("reading[%d] = %+v, want %+v", i, rds[i], want[i])
		}
	}
}
