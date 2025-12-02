; Unleash Go SDK detection
; Detects IsEnabled() and GetVariant() calls

; unleash.IsEnabled("flag-key")
(call_expression
  function: (selector_expression
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @method "^(IsEnabled|GetVariant)$")

; client.IsEnabled("flag-key", ...)
(call_expression
  function: (selector_expression
    operand: (identifier) @client
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @client "^(unleash|unleashClient|client)$")
(#match? @method "^(IsEnabled|GetVariant)$")
