package ui

import (
	"log"
	"os"
	"strings"
	"sync"
	"time"

	"charm.land/bubbles/v2/spinner"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

type focusArea int

const (
	focusRacks focusArea = iota
	focusElevation
	focusInfo
)

type rackState struct {
	loading    bool
	front      []netbox.ElevationSlot
	rear       []netbox.ElevationSlot
	devices    []netbox.Device
	rowsCache  map[string][]row
	blockCache map[string][]block
}

func (r *rackState) rows(face string) []row {
	if r.rowsCache == nil {
		r.rowsCache = map[string][]row{}
	}
	if _, ok := r.rowsCache[face]; !ok {
		slots := r.front
		if face == "rear" {
			slots = r.rear
		}
		r.rowsCache[face] = buildRows(slots)
	}
	return r.rowsCache[face]
}

func (r *rackState) blocks(face string) []block {
	if r.blockCache == nil {
		r.blockCache = map[string][]block{}
	}
	if _, ok := r.blockCache[face]; !ok {
		r.blockCache[face] = buildBlocks(r.rows(face), r.devices)
	}
	return r.blockCache[face]
}

type deviceDetail struct {
	loading    bool
	interfaces []netbox.Interface
	powerPorts []netbox.PowerPort
	outlets    []netbox.PowerOutlet
}

type readingsEntry struct {
	loading bool
	at      time.Time
	data    []pdu.PowerReading
	err     string
}

type outletReadingEntry struct {
	loading bool
	at      time.Time
	watts   float64
	amps    float64
	err     string
}

type App struct {
	cfg      *config.Config
	client   *netbox.Client
	dryRun   bool
	jumpRack string

	width, height int
	focus         focusArea
	spinner       spinner.Model
	statusLine    string
	errMsg        string

	version    string
	racks      []netbox.Rack
	roleColors map[string]string
	rackCursor int

	rackData  map[int]*rackState
	face      string
	devCursor int

	details map[int]*deviceDetail

	ctrlMu      sync.Mutex
	controllers map[string]pdu.Controller
	readings    map[string]*readingsEntry
	powerByPDU  map[string]map[string]pdu.OutletState // pdu → device → state
	outletDraw  map[string]*outletReadingEntry        // "pdu/outlet" → live W/A
	modal       *modal
	menu        *actionMenu
	pendingMenu bool // enter pressed while details were still loading

	toast    string
	toastGen int

	hit hitState // mouse hit-test geometry, captured during render
}

const loadingDetailsStatus = "loading device details…"

func NewApp(cfg *config.Config, jumpRack string, dryRun bool) *App {
	sp := spinner.New()
	sp.Spinner = spinner.Dot
	return &App{
		cfg:         cfg,
		dryRun:      dryRun,
		jumpRack:    jumpRack,
		spinner:     sp,
		face:        "front",
		rackData:    map[int]*rackState{},
		details:     map[int]*deviceDetail{},
		roleColors:  map[string]string{},
		controllers: map[string]pdu.Controller{},
		readings:    map[string]*readingsEntry{},
		powerByPDU:  map[string]map[string]pdu.OutletState{},
		outletDraw:  map[string]*outletReadingEntry{},
		statusLine:  "fetching token from 1Password…",
	}
}

func (a *App) Init() tea.Cmd {
	return tea.Batch(a.spinner.Tick, fetchTokenCmd(a.cfg.NetBox.TokenOpRef))
}

func (a *App) currentRackID() int {
	if len(a.racks) == 0 {
		return 0
	}
	return a.racks[a.rackCursor].ID
}

func (a *App) currentRack() *netbox.Rack {
	if len(a.racks) == 0 {
		return nil
	}
	return &a.racks[a.rackCursor]
}

// selectedDevice returns the device under the elevation cursor, if any.
func (a *App) selectedDevice() *netbox.Device {
	rd := a.rackData[a.currentRackID()]
	if rd == nil || rd.loading {
		return nil
	}
	blocks := rd.blocks(a.face)
	if a.devCursor < 0 || a.devCursor >= len(blocks) {
		return nil
	}
	return blocks[a.devCursor].Device
}

func (a *App) isPDU(d *netbox.Device) bool {
	if d == nil {
		return false
	}
	_, configured := a.cfg.PDUs[d.Name]
	return configured || strings.EqualFold(d.Role.Name, "power")
}

func (a *App) selectRack() tea.Cmd {
	id := a.currentRackID()
	if id == 0 {
		return nil
	}
	a.devCursor = 0
	if rd, ok := a.rackData[id]; ok && !rd.loading {
		return nil // cached
	}
	a.rackData[id] = &rackState{loading: true}
	return loadRackCmd(a.client, id)
}

func (a *App) selectDevice() tea.Cmd {
	d := a.selectedDevice()
	if d == nil {
		return nil
	}
	var cmds []tea.Cmd
	if det, ok := a.details[d.ID]; !ok {
		a.details[d.ID] = &deviceDetail{loading: true}
		cmds = append(cmds, loadDetailCmd(a.client, d.ID, a.isPDU(d)))
	} else if !det.loading {
		cmds = append(cmds, a.outletDrawCmds(det)...)
	}
	if cmd := a.maybeLoadReadings(d); cmd != nil {
		cmds = append(cmds, cmd)
	}
	if len(cmds) == 0 {
		return nil
	}
	return tea.Batch(cmds...)
}

// outletDrawCmds refreshes stale per-outlet W/A readings for the outlets
// feeding the selected device.
func (a *App) outletDrawCmds(det *deviceDetail) []tea.Cmd {
	var cmds []tea.Cmd
	for _, t := range a.buildTargets(det) {
		key := orKey(t.PDU, t.Outlet)
		e := a.outletDraw[key]
		if e != nil && (e.loading || time.Since(e.at) < readingsRefresh) {
			continue
		}
		a.outletDraw[key] = &outletReadingEntry{loading: true}
		cmds = append(cmds, a.outletReadingCmd(t.PDU, t.Outlet))
	}
	return cmds
}

// deviceState combines a device's power state across all configured PDUs.
func (a *App) deviceState(name string) pdu.OutletState {
	st := pdu.StateUnknown
	seen := false
	for _, byDev := range a.powerByPDU {
		if s, ok := byDev[name]; ok {
			if !seen {
				st, seen = s, true
			} else {
				st = combineStates(st, s)
			}
		}
	}
	return st
}

// powerSweepCmds refreshes outlet states for every configured PDU in the
// given device list.
func (a *App) powerSweepCmds(devices []netbox.Device) []tea.Cmd {
	var cmds []tea.Cmd
	for _, d := range devices {
		if _, ok := a.cfg.PDUs[d.Name]; ok {
			cmds = append(cmds, a.loadPowerStatesCmd(d.Name, d.ID))
		}
	}
	return cmds
}

// maybeLoadReadings kicks off a power poll when the selected device is a
// configured PDU and the cached readings are missing or stale.
func (a *App) maybeLoadReadings(d *netbox.Device) tea.Cmd {
	if d == nil {
		return nil
	}
	if _, configured := a.cfg.PDUs[d.Name]; !configured {
		return nil
	}
	re := a.readings[d.Name]
	if re != nil && (re.loading || time.Since(re.at) < readingsRefresh) {
		return nil
	}
	a.readings[d.Name] = &readingsEntry{loading: true}
	return a.loadReadingsCmd(d.Name)
}

func (a *App) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		a.width, a.height = msg.Width, msg.Height
		return a, nil

	case spinner.TickMsg:
		var cmd tea.Cmd
		a.spinner, cmd = a.spinner.Update(msg)
		return a, cmd

	case tokenMsg:
		if msg.Err != nil {
			a.errMsg = msg.Err.Error()
			a.statusLine = ""
			return a, nil
		}
		a.client = netbox.New(a.cfg.NetBox.URL, msg.Token)
		a.statusLine = "loading racks…"
		return a, loadRacksCmd(a.client)

	case racksMsg:
		a.statusLine = ""
		if msg.Err != nil {
			a.errMsg = msg.Err.Error()
			return a, nil
		}
		a.version = msg.Version
		a.racks = msg.Racks
		for _, role := range msg.Roles {
			a.roleColors[role.Name] = role.Color
		}
		if a.jumpRack != "" {
			for i, r := range a.racks {
				if strings.EqualFold(r.Name, a.jumpRack) {
					a.rackCursor = i
					break
				}
			}
		}
		return a, a.selectRack()

	case rackDataMsg:
		rd := a.rackData[msg.RackID]
		if rd == nil {
			return a, nil
		}
		if msg.Err != nil {
			delete(a.rackData, msg.RackID)
			a.errMsg = msg.Err.Error()
			return a, nil
		}
		*rd = rackState{front: msg.Front, rear: msg.Rear, devices: msg.Devices}
		cmds := a.powerSweepCmds(msg.Devices)
		if msg.RackID == a.currentRackID() {
			if cmd := a.selectDevice(); cmd != nil {
				cmds = append(cmds, cmd)
			}
		}
		return a, tea.Batch(cmds...)

	case detailMsg:
		if os.Getenv("RACKVIZ_DEBUG") != "" {
			log.Printf("detailMsg dev=%d err=%v ifaces=%d ports=%d", msg.DeviceID, msg.Err, len(msg.Interfaces), len(msg.PowerPorts))
		}
		det := a.details[msg.DeviceID]
		if det == nil {
			return a, nil
		}
		if a.statusLine == loadingDetailsStatus {
			a.statusLine = ""
		}
		if msg.Err != nil {
			delete(a.details, msg.DeviceID)
			a.pendingMenu = false
			a.errMsg = msg.Err.Error()
			return a, nil
		}
		*det = deviceDetail{interfaces: msg.Interfaces, powerPorts: msg.PowerPorts, outlets: msg.Outlets}
		var cmds []tea.Cmd
		if d := a.selectedDevice(); d != nil && d.ID == msg.DeviceID {
			cmds = a.outletDrawCmds(det)
			if a.pendingMenu {
				a.pendingMenu = false
				if cmd := a.openMenu(); cmd != nil {
					cmds = append(cmds, cmd)
				}
			}
		}
		return a, tea.Batch(cmds...)

	case readingsMsg:
		re := a.readings[msg.PDU]
		if re == nil {
			return a, nil
		}
		if msg.Err != nil {
			*re = readingsEntry{at: time.Now(), err: msg.Err.Error()}
		} else {
			*re = readingsEntry{at: time.Now(), data: msg.Readings}
		}
		// Keep polling while this PDU stays selected.
		if d := a.selectedDevice(); d != nil && d.Name == msg.PDU {
			return a, readingsTickCmd(msg.PDU)
		}
		return a, nil

	case readingsTickMsg:
		if d := a.selectedDevice(); d != nil && d.Name == msg.PDU {
			if re := a.readings[msg.PDU]; re != nil && !re.loading {
				re.loading = true
				return a, a.loadReadingsCmd(msg.PDU)
			}
		}
		return a, nil

	case outletStateMsg:
		if a.modal != nil && msg.Err == nil {
			for i := range a.modal.Targets {
				t := &a.modal.Targets[i]
				if t.PDU == msg.PDU && t.Outlet == msg.Outlet {
					t.State = msg.State
					t.StateKnown = true
				}
			}
		}
		if a.menu != nil {
			a.menu.applyState(msg)
		}
		return a, nil

	case powerStatesMsg:
		if os.Getenv("RACKVIZ_DEBUG") != "" {
			log.Printf("powerStatesMsg pdu=%s devices=%d err=%v", msg.PDU, len(msg.ByDevice), msg.Err)
		}
		if msg.Err != nil {
			delete(a.powerByPDU, msg.PDU)
			return a, nil
		}
		a.powerByPDU[msg.PDU] = msg.ByDevice
		return a, nil

	case actionResultMsg:
		a.statusLine = ""
		if msg.Err != nil {
			a.errMsg = msg.Desc + ": " + msg.Err.Error()
			return a, nil
		}
		a.errMsg = ""
		a.toast = "✓ " + msg.Desc
		a.toastGen++
		gen := a.toastGen
		cmds := []tea.Cmd{tea.Tick(5*time.Second, func(time.Time) tea.Msg {
			return toastClearMsg{gen: gen}
		})}
		// Re-sweep the PDU so the elevation colors reflect the new state.
		if rd := a.rackData[a.currentRackID()]; rd != nil && msg.PDU != "" {
			for _, d := range rd.devices {
				if d.Name == msg.PDU {
					cmds = append(cmds, a.loadPowerStatesCmd(d.Name, d.ID))
					break
				}
			}
		}
		return a, tea.Batch(cmds...)

	case outletReadingMsg:
		e := a.outletDraw[orKey(msg.PDU, msg.Outlet)]
		if e == nil {
			return a, nil
		}
		if msg.Err != nil {
			*e = outletReadingEntry{at: time.Now(), err: msg.Err.Error()}
		} else {
			*e = outletReadingEntry{at: time.Now(), watts: msg.Reading.Watts, amps: msg.Reading.Amps}
		}
		return a, nil

	case toastClearMsg:
		if msg.gen == a.toastGen {
			a.toast = ""
		}
		return a, nil

	case tea.MouseClickMsg:
		if msg.Button == tea.MouseLeft {
			return a.handleClick(msg.Mouse())
		}
		return a, nil

	case tea.MouseWheelMsg:
		return a.handleWheel(msg.Mouse())

	case tea.KeyPressMsg:
		if a.modal != nil {
			return a.handleModalKey(msg)
		}
		if a.menu != nil {
			return a.handleMenuKey(msg)
		}
		return a.handleKey(msg)
	}
	return a, nil
}

func (a *App) handleKey(msg tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	if os.Getenv("RACKVIZ_DEBUG") != "" {
		d := a.selectedDevice()
		name := "<none>"
		if d != nil {
			name = d.Name
		}
		log.Printf("key=%q focus=%d devCursor=%d selected=%s", msg.String(), a.focus, a.devCursor, name)
	}
	a.errMsg = ""
	a.toast = ""
	a.pendingMenu = false // any key other than the enter below cancels the intent
	switch msg.String() {
	case "q", "ctrl+c":
		return a, tea.Quit

	case "tab":
		a.focus = (a.focus + 1) % 3
		return a, nil
	case "shift+tab":
		a.focus = (a.focus + 2) % 3
		return a, nil
	case "right", "l":
		a.focus = focusArea(clamp(int(a.focus)+1, 0, 2))
		return a, nil
	case "left", "h":
		a.focus = focusArea(clamp(int(a.focus)-1, 0, 2))
		return a, nil

	case "f":
		if a.face == "front" {
			a.face = "rear"
		} else {
			a.face = "front"
		}
		a.devCursor = 0
		return a, a.selectDevice()

	case "r":
		id := a.currentRackID()
		if id != 0 {
			delete(a.rackData, id)
			for _, d := range a.detailIDsInRack(id) {
				delete(a.details, d)
			}
			a.rackData[id] = &rackState{loading: true}
			return a, loadRackCmd(a.client, id)
		}
		return a, nil

	case "j", "down":
		return a, a.moveCursor(1)
	case "k", "up":
		return a, a.moveCursor(-1)

	case "enter":
		if a.focus == focusRacks {
			a.focus = focusElevation
			return a, a.selectRack()
		}
		return a, a.openMenu()
	}
	return a, nil
}

func (a *App) detailIDsInRack(rackID int) []int {
	rd := a.rackData[rackID]
	if rd == nil {
		return nil
	}
	ids := make([]int, 0, len(rd.devices))
	for _, d := range rd.devices {
		ids = append(ids, d.ID)
	}
	return ids
}

func (a *App) moveCursor(delta int) tea.Cmd {
	switch a.focus {
	case focusRacks:
		if len(a.racks) == 0 {
			return nil
		}
		a.rackCursor = clamp(a.rackCursor+delta, 0, len(a.racks)-1)
		return a.selectRack()
	case focusElevation:
		rd := a.rackData[a.currentRackID()]
		if rd == nil || rd.loading {
			return nil
		}
		n := len(rd.blocks(a.face))
		if n == 0 {
			return nil
		}
		a.devCursor = clamp(a.devCursor+delta, 0, n-1)
		return a.selectDevice()
	}
	return nil
}

func clamp(v, lo, hi int) int {
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

func (a *App) View() tea.View {
	v := tea.NewView(a.render())
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	return v
}

func (a *App) render() string {
	if a.width == 0 {
		return "starting…"
	}
	header := styleTitle.Render(" vibe_rackviz ") + styleDim.Render("NetBox "+a.version+" @ "+a.cfg.NetBox.URL)
	if a.dryRun {
		header += styleToast.Render("  [dry-run]")
	}

	footer := styleHelp.Render(" tab/←→ panes · j/k move · enter actions · f face · r refresh · q quit")
	if a.errMsg != "" {
		footer = styleErr.Render(" ✗ " + truncate(a.errMsg, a.width-4))
	} else if a.statusLine != "" {
		footer = styleToast.Render(" " + a.spinner.View() + " " + a.statusLine)
	} else if a.toast != "" {
		footer = styleToast.Render(" " + truncate(a.toast, a.width-4))
	}

	bodyHeight := a.height - 2 // header + footer
	if bodyHeight < 4 {
		bodyHeight = 4
	}

	leftW := 26
	rightW := 44
	midW := a.width - leftW - rightW - 6 // borders/padding
	if midW < 24 {
		rightW = a.width - leftW - 24 - 6
		if rightW < 20 {
			rightW = 20
		}
		midW = a.width - leftW - rightW - 6
	}

	// Pane titles: RACKS | <rack_name> <FACE> | <hostname>.
	titleMid := ""
	if r := a.currentRack(); r != nil {
		titleMid = r.Name + " " + strings.ToUpper(a.face)
	}
	titleRight := ""
	if d := a.selectedDevice(); d != nil {
		titleRight = d.Name
	} else if r := a.currentRack(); r != nil {
		titleRight = r.Name
	}

	// Pane geometry for mouse hit-testing (each pane renders w+2 wide).
	a.hit.paneX = [3]int{0, leftW + 2, leftW + midW + 4}
	a.hit.paneW = [3]int{leftW + 2, midW + 2, rightW + 2}

	left := a.renderPane(focusRacks, leftW, bodyHeight-2, "RACKS", a.renderRackList(leftW))
	mid := a.renderPane(focusElevation, midW, bodyHeight-2, titleMid, a.renderElevationScrolled(midW, bodyHeight-3))
	right := a.renderPane(focusInfo, rightW, bodyHeight-2, titleRight, a.renderInfo(rightW))

	body := lipgloss.JoinHorizontal(lipgloss.Top, left, mid, right)
	a.hit.overlayW = 0
	if overlay := a.renderOverlay(); overlay != "" {
		// Composite the popup over the panes instead of replacing them.
		bw, bh := lipgloss.Width(body), lipgloss.Height(body)
		ow, oh := lipgloss.Width(overlay), lipgloss.Height(overlay)
		x := max((bw-ow)/2, 0)
		y := max((bh-oh)/2, 0)
		a.hit.overlayX, a.hit.overlayY = x, y
		a.hit.overlayW, a.hit.overlayH = ow, oh
		body = lipgloss.NewCompositor(
			lipgloss.NewLayer(body),
			lipgloss.NewLayer(overlay).X(x).Y(y).Z(1),
		).Render()
	}
	return header + "\n" + body + "\n" + footer
}

func (a *App) renderOverlay() string {
	switch {
	case a.modal != nil:
		return a.renderModal()
	case a.menu != nil:
		return a.renderMenu()
	}
	return ""
}

// renderPane draws one bordered pane. Every line is pre-padded to the exact
// content width and the style gets no Width/Height: lipgloss v2's width
// handling re-wraps lines and strips styling from trailing whitespace, which
// clipped block backgrounds at the text edge.
func (a *App) renderPane(area focusArea, w, h int, title, content string) string {
	innerW := w - 2 // horizontal padding
	lines := strings.Split(styleTitle.Render(truncate(title, innerW))+"\n"+content, "\n")
	if len(lines) > h {
		lines = lines[:h]
	}
	for len(lines) < h {
		lines = append(lines, "")
	}
	for i := range lines {
		lines[i] = pad(lines[i], innerW)
	}
	return a.paneStyle(area).Render(strings.Join(lines, "\n"))
}

func (a *App) paneStyle(area focusArea) lipgloss.Style {
	if a.focus == area {
		return stylePaneFocused
	}
	return stylePane
}

// renderElevationScrolled clips the elevation to the pane height, keeping the
// cursor's block visible.
func (a *App) renderElevationScrolled(width, height int) string {
	content := a.renderElevation(width)
	lines := strings.Split(content, "\n")
	a.hit.elevScroll = 0
	if len(lines) <= height {
		return content
	}
	// Find the cursor line: locate the selected block's top row.
	target := 0
	rd := a.rackData[a.currentRackID()]
	if rd != nil && !rd.loading {
		blocks := rd.blocks(a.face)
		rows := rd.rows(a.face)
		if a.devCursor >= 0 && a.devCursor < len(blocks) {
			b := blocks[a.devCursor]
			if b.TopU != 0 {
				for i, r := range rows {
					if r.U == b.TopU {
						target = i
						break
					}
				}
			} else {
				target = len(lines) - 1
			}
		}
	}
	start := clamp(target-height/2, 0, len(lines)-height)
	a.hit.elevScroll = start
	return strings.Join(lines[start:start+height], "\n")
}
