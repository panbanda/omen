; Unleash JavaScript/TypeScript SDK detection
; Detects isEnabled() and getVariant() calls

; unleash.isEnabled("flag-key")
((call_expression
  function: (member_expression
    property: (property_identifier) @method)
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
  (#match? @method "^(isEnabled|getVariant)$"))

; client.isEnabled("flag-key")
((call_expression
  function: (member_expression
    object: (identifier) @client
    property: (property_identifier) @method)
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
  (#match? @client "^(unleash|unleashClient|client)$")
  (#match? @method "^(isEnabled|getVariant)$"))
