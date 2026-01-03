package main

import (
	"testing"
)

// TestContextTierLimits verifies tier-specific limits
func TestContextTierLimits(t *testing.T) {
	tests := []struct {
		tier             string
		expectedMaxFiles int
		expectedMaxGraph int
		expectedMaxSyms  int
	}{
		{"essential", 20, 10, 10},
		{"standard", 100, 50, 50},
		{"full", 0, 0, 0}, // 0 means unlimited
	}

	for _, tt := range tests {
		t.Run(tt.tier, func(t *testing.T) {
			limits := getTierLimits(tt.tier)
			if limits.MaxFiles != tt.expectedMaxFiles {
				t.Errorf("tier %s: MaxFiles = %d, want %d", tt.tier, limits.MaxFiles, tt.expectedMaxFiles)
			}
			if limits.MaxGraphNodes != tt.expectedMaxGraph {
				t.Errorf("tier %s: MaxGraphNodes = %d, want %d", tt.tier, limits.MaxGraphNodes, tt.expectedMaxGraph)
			}
			if limits.MaxSymbols != tt.expectedMaxSyms {
				t.Errorf("tier %s: MaxSymbols = %d, want %d", tt.tier, limits.MaxSymbols, tt.expectedMaxSyms)
			}
		})
	}
}

// TestContextTierDefault verifies default tier is "standard"
func TestContextTierDefault(t *testing.T) {
	limits := getTierLimits("")
	standardLimits := getTierLimits("standard")

	if limits.MaxFiles != standardLimits.MaxFiles {
		t.Error("empty tier should default to standard")
	}
	if limits.MaxGraphNodes != standardLimits.MaxGraphNodes {
		t.Error("empty tier MaxGraphNodes should default to standard")
	}
	if limits.MaxSymbols != standardLimits.MaxSymbols {
		t.Error("empty tier MaxSymbols should default to standard")
	}
}

// TestContextTierUnknown verifies unknown tier defaults to standard
func TestContextTierUnknown(t *testing.T) {
	limits := getTierLimits("unknown-tier")
	standardLimits := getTierLimits("standard")

	if limits.MaxFiles != standardLimits.MaxFiles {
		t.Error("unknown tier should default to standard")
	}
}

// TestTierLimitsStruct verifies TierLimits struct fields
func TestTierLimitsStruct(t *testing.T) {
	limits := TierLimits{
		MaxFiles:      100,
		MaxGraphNodes: 50,
		MaxSymbols:    25,
	}

	if limits.MaxFiles != 100 {
		t.Errorf("MaxFiles = %d, want 100", limits.MaxFiles)
	}
	if limits.MaxGraphNodes != 50 {
		t.Errorf("MaxGraphNodes = %d, want 50", limits.MaxGraphNodes)
	}
	if limits.MaxSymbols != 25 {
		t.Errorf("MaxSymbols = %d, want 25", limits.MaxSymbols)
	}
}
