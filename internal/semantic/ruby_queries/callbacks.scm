; Rails lifecycle callbacks: before_save :method_name
(call
  method: (identifier) @callback
  (#match? @callback "^(before|after|around)_(save|create|update|destroy|validation|commit|rollback|initialize|find|touch)$")
  arguments: (argument_list
    (simple_symbol) @method_name))

; Callback with conditions: before_save :method, if: :condition
(call
  method: (identifier) @callback
  (#match? @callback "^(before|after|around)_(save|create|update|destroy|validation|commit|rollback|initialize|find|touch)$")
  arguments: (argument_list
    (simple_symbol) @method_name
    (pair
      key: (hash_key_symbol) @key
      (#match? @key "^(if|unless)$")
      value: (simple_symbol) @condition_method)))

; Scope definitions: scope :name, -> { ... }
(call
  method: (identifier) @scope_method
  (#eq? @scope_method "scope")
  arguments: (argument_list
    (simple_symbol) @scope_name))

; attr_accessor/attr_reader/attr_writer: attr_accessor :name
(call
  method: (identifier) @attr_method
  (#match? @attr_method "^attr_(accessor|reader|writer)$")
  arguments: (argument_list
    (simple_symbol) @attr_name))

; delegate: delegate :name, to: :target
(call
  method: (identifier) @delegate_method
  (#eq? @delegate_method "delegate")
  arguments: (argument_list
    (simple_symbol) @delegate_name))

; validate with custom method: validate :method_name
(call
  method: (identifier) @validate_method
  (#eq? @validate_method "validate")
  arguments: (argument_list
    (simple_symbol) @custom_validator))
