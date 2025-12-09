; send with symbol: send(:method_name)
(call
  method: (identifier) @send_method
  (#match? @send_method "^(send|public_send|__send__)$")
  arguments: (argument_list
    (simple_symbol) @method_name))

; send with string: send("method_name")
(call
  method: (identifier) @send_method
  (#match? @send_method "^(send|public_send|__send__)$")
  arguments: (argument_list
    (string
      (string_content) @method_name)))

; try with symbol: object.try(:method_name)
(call
  method: (identifier) @try_method
  (#eq? @try_method "try")
  arguments: (argument_list
    (simple_symbol) @method_name))

; respond_to? check: respond_to?(:method_name)
(call
  method: (identifier) @respond_method
  (#eq? @respond_method "respond_to?")
  arguments: (argument_list
    (simple_symbol) @method_name))

; method reference: method(:method_name)
(call
  method: (identifier) @method_ref
  (#eq? @method_ref "method")
  arguments: (argument_list
    (simple_symbol) @method_name))

; define_method: define_method(:name) { ... }
(call
  method: (identifier) @define_method
  (#eq? @define_method "define_method")
  arguments: (argument_list
    (simple_symbol) @method_name))

; alias_method: alias_method :new_name, :old_name
(call
  method: (identifier) @alias_method
  (#eq? @alias_method "alias_method")
  arguments: (argument_list
    (simple_symbol) @new_name
    (simple_symbol) @old_name))
