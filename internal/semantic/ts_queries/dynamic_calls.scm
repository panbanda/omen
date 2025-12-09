; Dynamic property access with string literal
; Example: obj["methodName"]() or obj['process']
(subscript_expression
  index: (string
    (string_fragment) @method_name))
