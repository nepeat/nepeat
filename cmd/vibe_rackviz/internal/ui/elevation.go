package ui

import (
	"fmt"
	"math"
	"sort"
	"strings"

	"charm.land/lipgloss/v2"
	"github.com/charmbracelet/x/ansi"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

// row is one rendered U of the rack: which device (if any) occupies it.
type row struct {
	U        int
	DeviceID int // 0 = empty
}

// block is a contiguous run of rows occupied by the same device, a bay child
// (Bay set), or a bare 0U device. Blocks are the selectable units.
type block struct {
	Device *netbox.Device
	TopU   int // 0 for bay children and 0U devices
	Rows   int
	Bay    string // device-bay name for children of a racked parent
}

// buildRows computes rack occupancy locally from device positions and unit
// heights (the REST elevation endpoint has no GraphQL equivalent). A device
// occupies a face if mounted on it or full-depth; same-face devices win
// slot conflicts over full-depth devices from the other face.
func buildRows(devices []netbox.Device, uHeight int, descUnits bool, face string) []row {
	occ := map[int]int{}
	place := func(d *netbox.Device) {
		base := int(math.Floor(*d.Position))
		h := int(d.DeviceType.UHeight + 0.5)
		if h < 1 {
			h = 1
		}
		for u := base; u < base+h; u++ {
			if occ[u] == 0 {
				occ[u] = d.ID
			}
		}
	}
	for i := range devices {
		if d := &devices[i]; d.Position != nil && d.FaceValue() == face {
			place(d)
		}
	}
	for i := range devices {
		if d := &devices[i]; d.Position != nil && d.FaceValue() != face && d.DeviceType.IsFullDepth {
			place(d)
		}
	}
	rows := make([]row, 0, uHeight)
	if descUnits {
		for u := 1; u <= uHeight; u++ {
			rows = append(rows, row{U: u, DeviceID: occ[u]})
		}
	} else {
		for u := uHeight; u >= 1; u-- {
			rows = append(rows, row{U: u, DeviceID: occ[u]})
		}
	}
	return rows
}

// buildBlocks groups rows into selectable blocks (racked devices), then
// appends bay children (grouped by parent/bay) and bare 0U devices.
func buildBlocks(rows []row, devices []netbox.Device) []block {
	byID := map[int]*netbox.Device{}
	for i := range devices {
		byID[devices[i].ID] = &devices[i]
	}
	var blocks []block
	for i := 0; i < len(rows); i++ {
		if rows[i].DeviceID == 0 {
			continue
		}
		id := rows[i].DeviceID
		n := 1
		for i+n < len(rows) && rows[i+n].DeviceID == id {
			n++
		}
		blocks = append(blocks, block{Device: byID[id], TopU: rows[i].U, Rows: n})
		i += n - 1
	}
	var children, zeroU []block
	for i := range devices {
		d := &devices[i]
		if d.Position != nil {
			continue
		}
		if d.Parent != nil {
			children = append(children, block{Device: d, Rows: 1, Bay: d.BayName()})
		} else {
			zeroU = append(zeroU, block{Device: d, Rows: 1})
		}
	}
	sort.Slice(children, func(i, j int) bool {
		if children[i].Device.Parent.Name != children[j].Device.Parent.Name {
			return children[i].Device.Parent.Name < children[j].Device.Parent.Name
		}
		return children[i].Bay < children[j].Bay
	})
	sort.Slice(zeroU, func(i, j int) bool { return zeroU[i].Device.Name < zeroU[j].Device.Name })
	return append(append(blocks, children...), zeroU...)
}

// childrenByParent indexes bay-child blocks by parent device ID.
func childrenByParent(blocks []block) map[int][]*block {
	out := map[int][]*block{}
	for i := range blocks {
		if blocks[i].Bay != "" && blocks[i].Device.Parent != nil {
			out[blocks[i].Device.Parent.ID] = append(out[blocks[i].Device.Parent.ID], &blocks[i])
		}
	}
	return out
}

// renderElevation draws the rack column. selected indexes into the block list
// (racked blocks first, then 0U shelf entries).
func (a *App) renderElevation(width int) string {
	rd := a.rackData[a.currentRackID()]
	if rd == nil || rd.loading {
		return a.spinner.View() + " loading rack…"
	}
	rows := rd.rows(a.face)
	blocks := rd.blocks(a.face)

	blockAt := map[int]int{}   // U → block index
	blockTop := map[int]bool{} // U is the first row of its block
	for bi, b := range blocks {
		if b.TopU == 0 {
			continue
		}
		for r := 0; r < b.Rows; r++ {
			blockAt[b.TopU-r] = bi
		}
		blockTop[b.TopU] = true
	}

	// Pane content area is width-2 (padding); each line is
	// " NNN <body>" = 5 cols of gutter, so the body gets width-7.
	inner := width - 7
	if inner < 8 {
		inner = 8
	}
	kids := childrenByParent(blocks)
	a.hit.elevLines = a.hit.elevLines[:0]
	var sb strings.Builder
	for ri, r := range rows {
		gutter := styleDim.Render(fmt.Sprintf("%3d", r.U))
		var body string
		if bi, ok := blockAt[r.U]; ok {
			a.hit.elevLines = append(a.hit.elevLines, bi)
			b := blocks[bi]
			label := ""
			switch b.TopU - r.U {
			case 0:
				label = b.Device.Name
			case 1:
				if ch := kids[b.Device.ID]; len(ch) > 0 {
					names := make([]string, len(ch))
					for i, c := range ch {
						names[i] = c.Device.Name
					}
					label = "└ " + strings.Join(names, ", ")
				} else {
					label = strings.TrimSpace(b.Device.DeviceType.Manufacturer.Name + " " + b.Device.DeviceType.Model)
				}
			case 2:
				label = a.devicePowerLine(b.Device.Name)
			}
			st := a.powerStyle(b.Device.Name)
			isCursor := bi == a.devCursor && a.focus == focusElevation
			if isCursor {
				st = st.Reverse(true).Bold(true)
			}
			// Solid rule across the block's last row when another device sits
			// directly below, so adjacent blocks don't merge into one slab.
			// Skipped on the cursor row: lipgloss's underline path styles
			// spaces without Reverse, which mangles the highlight.
			if ri+1 < len(rows) && !isCursor {
				if nbi, below := blockAt[rows[ri+1].U]; below && nbi != bi {
					st = st.Underline(true).UnderlineSpaces(true).UnderlineColor(colorBlockRule)
				}
			}
			body = st.Render(centerPad(label, inner))
		} else {
			a.hit.elevLines = append(a.hit.elevLines, -1)
			body = styleDim.Render(pad(strings.Repeat(" ", inner/2)+"·", inner))
		}
		fmt.Fprintf(&sb, " %s %s\n", gutter, body)
	}

	// Bay children, then bare 0U devices.
	firstBay, firstZero := true, true
	for bi, b := range blocks {
		if b.TopU != 0 {
			continue
		}
		var line string
		if b.Bay != "" {
			if firstBay {
				sb.WriteString(styleDim.Render(" ── bays ──") + "\n")
				a.hit.elevLines = append(a.hit.elevLines, -1)
				firstBay = false
			}
			line = " " + b.Device.Parent.Name + "/" + b.Bay + "  " + b.Device.Name
		} else {
			if firstZero {
				sb.WriteString(styleDim.Render(" ── 0U ──") + "\n")
				a.hit.elevLines = append(a.hit.elevLines, -1)
				firstZero = false
			}
			line = " " + b.Device.Name
			if b.Device.Description != "" {
				line += " — " + b.Device.Description
			}
		}
		line = truncate(line, width-4)
		if bi == a.devCursor && a.focus == focusElevation {
			line = styleSelected.Render(pad(line, width-4))
		}
		sb.WriteString(a.powerDot(b.Device.Name) + line + "\n")
		a.hit.elevLines = append(a.hit.elevLines, bi)
	}
	return strings.TrimRight(sb.String(), "\n")
}

// devicePowerLine sums the live draw of every outlet feeding a device, for
// the third row of tall elevation blocks. Empty until readings arrive.
func (a *App) devicePowerLine(deviceName string) string {
	var watts, amps float64
	measured := 0
	for _, t := range a.feedingOutlets(deviceName) {
		e := a.outletDraw[orKey(t.pdu, t.outlet)]
		if e == nil || e.loading || e.err != "" {
			continue
		}
		watts += e.watts
		amps += e.amps
		measured++
	}
	if measured == 0 {
		return ""
	}
	return fmt.Sprintf("%.1f W · %.2f A", watts, amps)
}

// powerStyle picks the block background for a device's live power state.
func (a *App) powerStyle(deviceName string) lipgloss.Style {
	switch a.deviceState(deviceName) {
	case pdu.StateOn:
		return stylePowerOn
	case pdu.StateOff:
		return stylePowerOff
	}
	return stylePowerNone
}

// powerDot is a small state indicator for 0U shelf lines.
func (a *App) powerDot(deviceName string) string {
	switch a.deviceState(deviceName) {
	case pdu.StateOn:
		return " " + dotOn
	case pdu.StateOff:
		return " " + dotOff
	}
	return "  "
}

// pad right-pads s to width. The last pad char is a NBSP: lipgloss v2's
// bordered-pane rendering strips styling from trailing regular spaces, which
// would clip block backgrounds at the text edge.
func pad(s string, width int) string {
	w := lipgloss.Width(s)
	if w >= width {
		return truncate(s, width)
	}
	return s + strings.Repeat(" ", width-w-1) + " "
}

// centerPad centers s within width with the same NBSP tail trick.
func centerPad(s string, width int) string {
	w := lipgloss.Width(s)
	if w >= width {
		return truncate(s, width)
	}
	left := (width - w) / 2
	return strings.Repeat(" ", left) + pad(s, width-left)
}

func truncate(s string, width int) string {
	if lipgloss.Width(s) <= width {
		return s
	}
	return ansi.Truncate(s, width, "…")
}
