; Function/method values in object properties
; Example: { handler: myFunction, callback: processData }
(pair
  key: (_) @_key
  value: (identifier) @func_ref)

; Function values in array literals
; Example: [callback1, callback2, handler]
(array
  (identifier) @func_ref)

; Function assigned to variable
; Example: const handler = processRequest
(variable_declarator
  name: (identifier)
  value: (identifier) @func_ref)

; Shorthand property (same name for key and value)
; Example: { myFunction } is shorthand for { myFunction: myFunction }
(shorthand_property_identifier) @func_ref

; Function passed as argument
; Example: addEventListener('click', handleClick)
(call_expression
  arguments: (arguments
    (identifier) @func_ref))
