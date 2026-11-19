// Package tui is the `speak voices` picker: browse, hear a sample, save a default.
package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"

	"github.com/peetzweg/speak/spikes/go/internal/tts"
)

type playedMsg struct {
	voice string
	err   error
}

type model struct {
	voices  []tts.Voice
	cursor  int
	current string
	status  string
	chosen  string
	play    func(voice string) error
}

// Pick runs the picker. play synthesises and plays a sample for a voice; the returned
// string is the chosen voice ("" if the user quit). opts lets tests drive it headless.
func Pick(voices []tts.Voice, current string, play func(string) error, opts ...tea.ProgramOption) (string, error) {
	m := model{voices: voices, current: current, play: play}
	for i, v := range voices {
		if v.ID == current {
			m.cursor = i
		}
	}
	out, err := tea.NewProgram(m, opts...).Run()
	if err != nil {
		return "", err
	}
	return out.(model).chosen, nil
}

func (m model) Init() tea.Cmd { return nil }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case playedMsg:
		if msg.err != nil {
			m.status = "error: " + msg.err.Error()
		} else {
			m.status = "played " + msg.voice
		}
	case tea.KeyPressMsg:
		switch msg.String() {
		case "up", "k":
			m.cursor = max(0, m.cursor-1)
		case "down", "j":
			m.cursor = min(len(m.voices)-1, m.cursor+1)
		case "space", " ", "p":
			v := m.voices[m.cursor].ID
			m.status = "synthesising " + v + "..."
			return m, func() tea.Msg { return playedMsg{v, m.play(v)} }
		case "enter":
			m.chosen = m.voices[m.cursor].ID
			return m, tea.Quit
		case "q", "esc", "ctrl+c":
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m model) View() tea.View {
	var b strings.Builder
	b.WriteString("speak voices   ↑/↓ move · space play · enter set default · q quit\n\n")
	const rows = 14
	start := max(0, min(m.cursor-rows/2, len(m.voices)-rows))
	for i := start; i < min(len(m.voices), start+rows); i++ {
		v := m.voices[i]
		cur, def := "  ", " "
		if i == m.cursor {
			cur = "> "
		}
		if v.ID == m.current {
			def = "*"
		}
		fmt.Fprintf(&b, "%s%s %-14s %s\n", cur, def, v.ID, v.Label)
	}
	fmt.Fprintf(&b, "\n%s\n", m.status)
	return tea.NewView(b.String())
}
