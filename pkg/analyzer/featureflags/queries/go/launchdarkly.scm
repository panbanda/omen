; LaunchDarkly Go SDK detection
; Detects BoolVariation(), StringVariation(), IntVariation(), etc.

; client.BoolVariation("flag-key", context, default)
(call_expression
  function: (selector_expression
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @method "^(BoolVariation|StringVariation|IntVariation|Float64Variation|JSONVariation|BoolVariationDetail|StringVariationDetail|IntVariationDetail|Float64VariationDetail|JSONVariationDetail)$")

; ldClient.BoolVariation("flag-key", ...)
(call_expression
  function: (selector_expression
    operand: (identifier) @client
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @client "^(ldClient|client|ldclient|featureFlags)$")
(#match? @method "^(BoolVariation|StringVariation|IntVariation|Float64Variation|JSONVariation)$")
