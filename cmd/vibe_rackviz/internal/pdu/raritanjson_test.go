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
	bulkCalls  int
	lastAuth   string
}

// exec dispatches one JSON-RPC call; ok=false means unknown method/rid.
func (f *fakePX3) exec(path, method string, params json.RawMessage) (any, bool) {
	readings := map[string]float64{
		"/sensor/inlet-current": 11.5,
		"/sensor/inlet-power":   2640,
		"/sensor/l1-current":    5.2,
		"/sensor/l1-power":      1200,
		"/sensor/l2-current":    6.3,
		"/sensor/l2-power":      1440,
		"/sensor/out5-current":  0.38,
		"/sensor/out5-power":    45.2,
		"/sensor/out0-current":  1.0,
		"/sensor/out0-power":    120,
	}
	if path == "/model/pdu/0" && method == "getOutlets" {
		refs := make([]map[string]string, 8)
		for i := range refs {
			refs[i] = map[string]string{"rid": fmt.Sprintf("/model/pdu/0/outlet/%d", i)}
		}
		return refs, true
	}
	if v, ok := readings[path]; ok && method == "getReading" {
		return map[string]any{"valid": true, "value": v}, true
	}
	// getState/getSensors work on outlets 0..7; outlet 5 (0-based) mirrors
	// the switchable state, the rest are on.
	if strings.HasPrefix(path, "/model/pdu/0/outlet/") {
		idx, err := strconv.Atoi(strings.TrimPrefix(path, "/model/pdu/0/outlet/"))
		if err == nil && idx >= 0 && idx < 8 {
			switch method {
			case "getState":
				st := 1
				if idx == 5 {
					f.mu.Lock()
					st = f.powerState
					f.mu.Unlock()
				}
				return map[string]any{"available": true, "powerState": st}, true
			case "getSensors":
				// Only outlets 0 and 5 expose sensors in the fake.
				if idx == 0 || idx == 5 {
					return map[string]any{
						"current":     map[string]string{"rid": fmt.Sprintf("/sensor/out%d-current", idx)},
						"activePower": map[string]string{"rid": fmt.Sprintf("/sensor/out%d-power", idx)},
					}, true
				}
				return map[string]any{"current": nil, "activePower": nil}, true
			}
		}
	}
	switch path + " " + method {
	case "/model/pdu/0/outlet/5 setPowerState":
		var p struct {
			Pstate int `json:"pstate"`
		}
		json.Unmarshal(params, &p)
		f.mu.Lock()
		f.powerState = p.Pstate
		f.mu.Unlock()
		return nil, true
	case "/model/pdu/0/outlet/5 cyclePowerState":
		f.mu.Lock()
		f.cycles++
		f.mu.Unlock()
		return nil, true
	case "/model/pdu/0/inlet/0 getSensors":
		return map[string]any{
			"current":     map[string]string{"rid": "/sensor/inlet-current"},
			"activePower": map[string]string{"rid": "/sensor/inlet-power"},
		}, true
	case "/model/pdu/0/inlet/0 getPoles":
		return []map[string]any{
			{"current": map[string]string{"rid": "/sensor/l1-current"}, "activePower": map[string]string{"rid": "/sensor/l1-power"}},
			{"current": map[string]string{"rid": "/sensor/l2-current"}, "activePower": map[string]string{"rid": "/sensor/l2-power"}},
		}, true
	}
	return nil, false
}

func (f *fakePX3) handler() http.HandlerFunc {
	envelope := func(ret any) map[string]any {
		return map[string]any{"jsonrpc": "2.0", "id": 1, "result": map[string]any{"_ret_": ret}}
	}
	rpcError := func(method, path string) map[string]any {
		return map[string]any{
			"jsonrpc": "2.0", "id": 1,
			"error": map[string]any{"code": -32601, "message": "no such method " + method + " on " + path},
		}
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
		if r.URL.Path == "/bulk" && req.Method == "performBulk" {
			f.mu.Lock()
			f.bulkCalls++
			f.mu.Unlock()
			var p struct {
				Requests []struct {
					Rid  string `json:"rid"`
					JSON struct {
						Method string          `json:"method"`
						Params json.RawMessage `json:"params"`
					} `json:"json"`
				} `json:"requests"`
			}
			json.Unmarshal(req.Params, &p)
			responses := make([]map[string]any, len(p.Requests))
			for i, br := range p.Requests {
				var inner map[string]any
				if ret, ok := f.exec(br.Rid, br.JSON.Method, br.JSON.Params); ok {
					inner = envelope(ret)
				} else {
					inner = rpcError(br.JSON.Method, br.Rid)
				}
				responses[i] = map[string]any{"json": inner, "statcode": 200}
			}
			json.NewEncoder(w).Encode(map[string]any{
				"jsonrpc": "2.0", "id": 1,
				"result": map[string]any{"responses": responses},
			})
			return
		}
		if ret, ok := f.exec(r.URL.Path, req.Method, req.Params); ok {
			json.NewEncoder(w).Encode(envelope(ret))
			return
		}
		json.NewEncoder(w).Encode(rpcError(req.Method, r.URL.Path))
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
	states, err := c.OutletStates(ctx, nil)
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

	// Narrowed sweep queries only the requested outlets.
	narrowed, err := c.OutletStates(ctx, []int{6, 2})
	if err != nil {
		t.Fatal(err)
	}
	if len(narrowed) != 2 || narrowed[6] != StateOff || narrowed[2] != StateOn {
		t.Errorf("narrowed states = %v, want {6:off 2:on}", narrowed)
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

	// Bulk readings: sensor discovery + all reads via performBulk.
	fake.mu.Lock()
	before := fake.bulkCalls
	fake.mu.Unlock()
	brs, err := c.OutletReadings(ctx, []int{1, 6})
	if err != nil {
		t.Fatal(err)
	}
	if brs[6].Watts != 45.2 || brs[1].Watts != 120 {
		t.Errorf("bulk readings = %+v", brs)
	}
	fake.mu.Lock()
	used := fake.bulkCalls - before
	fake.mu.Unlock()
	if used == 0 {
		t.Error("OutletReadings did not use performBulk")
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
