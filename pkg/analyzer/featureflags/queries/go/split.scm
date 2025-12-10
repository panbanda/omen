; Split.io Go SDK detection
; Detects Treatment(), TreatmentWithConfig(), and Track() calls

; client.Treatment(key, "flag-key", attributes)
((call_expression
  function: (selector_expression
    field: (field_identifier) @method)
  arguments: (argument_list
    (_)
    (interpreted_string_literal) @flag_key))
  (#match? @method "^(Treatment|TreatmentWithConfig|Treatments|TreatmentsWithConfig|Track)$"))

; splitClient.Treatment(key, "flag-key", ...)
((call_expression
  function: (selector_expression
    operand: (identifier) @client
    field: (field_identifier) @method)
  arguments: (argument_list
    (_)
    (interpreted_string_literal) @flag_key))
  (#match? @client "^(splitClient|split|client)$")
  (#match? @method "^(Treatment|TreatmentWithConfig|Track)$"))
