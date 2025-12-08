package semantic

import (
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestRubyExtractor_Callbacks(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "before_save callback",
			code: `class User < ApplicationRecord
  before_save :normalize_name

  private

  def normalize_name
    self.name = name.downcase
  end
end`,
			expected: []string{"normalize_name"},
		},
		{
			name: "multiple lifecycle callbacks",
			code: `class Order < ApplicationRecord
  before_create :set_defaults
  after_save :notify_customer
  before_destroy :cleanup
end`,
			expected: []string{"set_defaults", "notify_customer", "cleanup"},
		},
		{
			name: "callback with conditional",
			code: `class Post < ApplicationRecord
  before_save :update_slug, if: :title_changed?
end`,
			expected: []string{"update_slug", "title_changed?"},
		},
		{
			name: "validates with method",
			code: `class User < ApplicationRecord
  validates :email, presence: true
  validate :email_format

  private

  def email_format
    errors.add(:email, "invalid") unless email =~ /@/
  end
end`,
			expected: []string{"email_format"},
		},
		{
			name: "scope definition",
			code: `class Article < ApplicationRecord
  scope :published, -> { where(published: true) }
  scope :recent, -> { order(created_at: :desc) }
end`,
			expected: []string{"published", "recent"},
		},
		{
			name: "delegate methods",
			code: `class Profile < ApplicationRecord
  belongs_to :user
  delegate :email, :name, to: :user
end`,
			expected: []string{"email", "name"},
		},
		{
			name: "attr_accessor",
			code: `class User
  attr_accessor :name, :email
end`,
			expected: []string{"name", "email"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangRuby, "test.rb")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newRubyExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				got[ref.Name] = true
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q, but didn't", want)
				}
			}

			if len(refs) != len(tt.expected) {
				var names []string
				for _, ref := range refs {
					names = append(names, ref.Name)
				}
				t.Errorf("expected %d refs, got %d: %v", len(tt.expected), len(refs), names)
			}
		})
	}
}

func TestRubyExtractor_DynamicCalls(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "send with symbol",
			code: `class Calculator
  def run
    send(:add)
  end

  def add; end
  def subtract; end
end`,
			expected: []string{"add"},
		},
		{
			name:     "public_send",
			code:     `obj.public_send(:process)`,
			expected: []string{"process"},
		},
		{
			name: "define_method",
			code: `class Builder
  define_method(:create) do
    # implementation
  end
end`,
			expected: []string{"create"},
		},
		{
			name: "alias_method",
			code: `class Legacy
  def new_method; end
  alias_method :old_method, :new_method
end`,
			expected: []string{"old_method", "new_method"},
		},
		{
			name: "method reference",
			code: `class Processor
  def process; end

  def get_method
    method(:process)
  end
end`,
			expected: []string{"process"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangRuby, "test.rb")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newRubyExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				got[ref.Name] = true
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q, but didn't", want)
				}
			}
		})
	}
}

func TestRubyExtractor_RefKinds(t *testing.T) {
	code := `class User < ApplicationRecord
  before_save :normalize

  def run
    send(:process)
  end
end`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangRuby, "test.rb")
	if err != nil {
		t.Fatalf("failed to parse: %v", err)
	}
	defer result.Tree.Close()

	extractor := newRubyExtractor()
	defer extractor.Close()

	refs := extractor.ExtractRefs(result.Tree, result.Source)

	kindByName := make(map[string]RefKind)
	for _, ref := range refs {
		kindByName[ref.Name] = ref.Kind
	}

	if kind, ok := kindByName["normalize"]; !ok || kind != RefCallback {
		t.Errorf("expected 'normalize' to be RefCallback, got %v", kind)
	}

	if kind, ok := kindByName["process"]; !ok || kind != RefDynamicCall {
		t.Errorf("expected 'process' to be RefDynamicCall, got %v", kind)
	}
}

func TestRubyExtractor_NilTree(t *testing.T) {
	extractor := newRubyExtractor()
	defer extractor.Close()

	refs := extractor.ExtractRefs(nil, nil)
	if refs != nil {
		t.Errorf("expected nil for nil tree, got %v", refs)
	}
}
