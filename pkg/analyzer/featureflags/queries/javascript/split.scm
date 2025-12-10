; Split.io JavaScript/TypeScript SDK detection
; Detects getTreatment(), getTreatmentWithConfig(), and track() calls

; client.getTreatment(user, "flag-key")
((call_expression
  function: (member_expression
    property: (property_identifier) @method)
  arguments: (arguments
    (_)
    (string
      (string_fragment) @flag_key)))
  (#match? @method "^(getTreatment|getTreatmentWithConfig|track)$"))

; splitClient.getTreatment("flag-key") - single arg variant
((call_expression
  function: (member_expression
    object: (identifier) @client
    property: (property_identifier) @method)
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
  (#match? @client "^(splitClient|split|client)$")
  (#match? @method "^(getTreatment|getTreatmentWithConfig|track)$"))
