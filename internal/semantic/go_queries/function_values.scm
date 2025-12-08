; Function values assigned to struct fields in composite literals
; Example: &cobra.Command{ RunE: runComplexity }
; The value must be a bare identifier (not a call expression)
(keyed_element
  (literal_element
    (identifier) @_field)
  (literal_element
    (identifier) @func_ref))

; Function values in slice/array literals (bare identifiers only)
; Example: handlers := []func(){handleA, handleB}
; This captures identifiers directly inside literal_value
(literal_value
  (literal_element
    (identifier) @func_ref))

; Function values in map literals with string keys
; Example: routes := map[string]func(){"get": handleGet}
(keyed_element
  (literal_element
    (interpreted_string_literal))
  (literal_element
    (identifier) @func_ref))

; Short variable declaration assigning function to variable
; Example: handler := processRequest
; Only captures when the right side is a bare identifier (not a call)
(short_var_declaration
  left: (expression_list
    (identifier))
  right: (expression_list
    (identifier) @func_ref))
