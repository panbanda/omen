package report

import (
	"encoding/json"
	"testing"
	"time"
)

func TestMetadataMarshaling(t *testing.T) {
	meta := Metadata{
		Repository:  "test-repo",
		GeneratedAt: time.Date(2024, 12, 10, 0, 0, 0, 0, time.UTC),
		Since:       "1y",
		OmenVersion: "1.0.0",
		Paths:       []string{".", "src"},
	}

	data, err := json.Marshal(meta)
	if err != nil {
		t.Fatalf("failed to marshal Metadata: %v", err)
	}

	var unmarshaled Metadata
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal Metadata: %v", err)
	}

	if unmarshaled.Repository != meta.Repository {
		t.Errorf("Repository = %q, want %q", unmarshaled.Repository, meta.Repository)
	}
	if unmarshaled.Since != meta.Since {
		t.Errorf("Since = %q, want %q", unmarshaled.Since, meta.Since)
	}
	if unmarshaled.OmenVersion != meta.OmenVersion {
		t.Errorf("OmenVersion = %q, want %q", unmarshaled.OmenVersion, meta.OmenVersion)
	}
	if len(unmarshaled.Paths) != len(meta.Paths) {
		t.Errorf("Paths length = %d, want %d", len(unmarshaled.Paths), len(meta.Paths))
	}
}

func TestSummaryInsightMarshaling(t *testing.T) {
	insight := SummaryInsight{
		ExecutiveSummary: "This is a test summary.",
		KeyFindings:      []string{"Finding 1", "Finding 2"},
		Recommendations: Recommendations{
			HighPriority: []Recommendation{
				{Title: "Fix bug", Description: "Important bug fix"},
			},
			MediumPriority: []Recommendation{
				{Title: "Refactor", Description: "Code cleanup"},
			},
			Ongoing: []Recommendation{
				{Title: "Monitor", Description: "Keep watching"},
			},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal SummaryInsight: %v", err)
	}

	var unmarshaled SummaryInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal SummaryInsight: %v", err)
	}

	if unmarshaled.ExecutiveSummary != insight.ExecutiveSummary {
		t.Errorf("ExecutiveSummary = %q, want %q", unmarshaled.ExecutiveSummary, insight.ExecutiveSummary)
	}
	if len(unmarshaled.KeyFindings) != 2 {
		t.Errorf("KeyFindings length = %d, want 2", len(unmarshaled.KeyFindings))
	}
	if len(unmarshaled.Recommendations.HighPriority) != 1 {
		t.Errorf("HighPriority length = %d, want 1", len(unmarshaled.Recommendations.HighPriority))
	}
}

func TestTrendsInsightMarshaling(t *testing.T) {
	insight := TrendsInsight{
		SectionInsight: "Trend analysis shows improvement.",
		ScoreAnnotations: []ScoreAnnotation{
			{
				Date:        "2024-03",
				Label:       "Major refactor",
				Change:      8,
				Description: "Score improved after cleanup.",
			},
		},
		HistoricalEvents: []HistoricalEvent{
			{
				Period:        "Sep 2018",
				Change:        -10,
				PrimaryDriver: "Duplication 99 to 54",
				Releases:      []string{"v1.0", "v1.1"},
			},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal TrendsInsight: %v", err)
	}

	var unmarshaled TrendsInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal TrendsInsight: %v", err)
	}

	if unmarshaled.SectionInsight != insight.SectionInsight {
		t.Errorf("SectionInsight = %q, want %q", unmarshaled.SectionInsight, insight.SectionInsight)
	}
	if len(unmarshaled.ScoreAnnotations) != 1 {
		t.Errorf("ScoreAnnotations length = %d, want 1", len(unmarshaled.ScoreAnnotations))
	}
	if unmarshaled.ScoreAnnotations[0].Change != 8 {
		t.Errorf("ScoreAnnotations[0].Change = %d, want 8", unmarshaled.ScoreAnnotations[0].Change)
	}
}

func TestComponentsInsightMarshaling(t *testing.T) {
	insight := ComponentsInsight{
		ComponentAnnotations: map[string][]ComponentAnnotation{
			"complexity": {
				{Date: "2024-06", Label: "Feature buildout", From: 95, To: 83, Description: "Complexity increased."},
			},
		},
		ComponentEvents: []ComponentEvent{
			{
				Period:    "Oct-Nov 2016",
				Component: "cohesion",
				From:      100,
				To:        84,
				Context:   "Rapid feature buildout.",
			},
		},
		ComponentInsights: map[string]string{
			"complexity": "Complexity is stable.",
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal ComponentsInsight: %v", err)
	}

	var unmarshaled ComponentsInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal ComponentsInsight: %v", err)
	}

	if len(unmarshaled.ComponentAnnotations["complexity"]) != 1 {
		t.Errorf("ComponentAnnotations[complexity] length = %d, want 1", len(unmarshaled.ComponentAnnotations["complexity"]))
	}
	if len(unmarshaled.ComponentEvents) != 1 {
		t.Errorf("ComponentEvents length = %d, want 1", len(unmarshaled.ComponentEvents))
	}
}

func TestPatternsInsightMarshaling(t *testing.T) {
	insight := PatternsInsight{
		Patterns: []Pattern{
			{Category: "hotspots", Insight: "5 of top 15 hotspots are GraphQL mutations."},
			{Category: "satd", Insight: "6 of 9 critical SATD items are security-related."},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal PatternsInsight: %v", err)
	}

	var unmarshaled PatternsInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal PatternsInsight: %v", err)
	}

	if len(unmarshaled.Patterns) != 2 {
		t.Errorf("Patterns length = %d, want 2", len(unmarshaled.Patterns))
	}
}

func TestHotspotsInsightMarshaling(t *testing.T) {
	insight := HotspotsInsight{
		SectionInsight: "Hotspot analysis shows concentration in API layer.",
		ItemAnnotations: []FileAnnotation{
			{File: "src/api/handler.go", Comment: "Central routing hub."},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal HotspotsInsight: %v", err)
	}

	var unmarshaled HotspotsInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal HotspotsInsight: %v", err)
	}

	if unmarshaled.SectionInsight != insight.SectionInsight {
		t.Errorf("SectionInsight = %q, want %q", unmarshaled.SectionInsight, insight.SectionInsight)
	}
}

func TestSATDInsightMarshaling(t *testing.T) {
	insight := SATDInsight{
		SectionInsight: "SATD patterns show security debt.",
		ItemAnnotations: []SATDAnnotation{
			{File: "pkg/auth/oauth.go", Line: 142, Comment: "Security-related debt."},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal SATDInsight: %v", err)
	}

	var unmarshaled SATDInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal SATDInsight: %v", err)
	}

	if len(unmarshaled.ItemAnnotations) != 1 {
		t.Errorf("ItemAnnotations length = %d, want 1", len(unmarshaled.ItemAnnotations))
	}
	if unmarshaled.ItemAnnotations[0].Line != 142 {
		t.Errorf("ItemAnnotations[0].Line = %d, want 142", unmarshaled.ItemAnnotations[0].Line)
	}
}

func TestFlagsInsightMarshaling(t *testing.T) {
	insight := FlagsInsight{
		SectionInsight: "11 feature flags are critically stale.",
		ItemAnnotations: []FlagAnnotation{
			{Flag: "connect", Comment: "3,371 days old - remove immediately"},
			{Flag: "new_checkout", Comment: "1,456 days old - likely safe to remove"},
		},
	}

	data, err := json.Marshal(insight)
	if err != nil {
		t.Fatalf("failed to marshal FlagsInsight: %v", err)
	}

	var unmarshaled FlagsInsight
	if err := json.Unmarshal(data, &unmarshaled); err != nil {
		t.Fatalf("failed to unmarshal FlagsInsight: %v", err)
	}

	if unmarshaled.SectionInsight != insight.SectionInsight {
		t.Errorf("SectionInsight = %q, want %q", unmarshaled.SectionInsight, insight.SectionInsight)
	}
	if len(unmarshaled.ItemAnnotations) != 2 {
		t.Errorf("ItemAnnotations length = %d, want 2", len(unmarshaled.ItemAnnotations))
	}
	if unmarshaled.ItemAnnotations[0].Flag != "connect" {
		t.Errorf("ItemAnnotations[0].Flag = %q, want %q", unmarshaled.ItemAnnotations[0].Flag, "connect")
	}
}
