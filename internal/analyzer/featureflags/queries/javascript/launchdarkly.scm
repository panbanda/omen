; LaunchDarkly JavaScript/TypeScript SDK detection
; Detects variation(), boolVariation(), stringVariation(), numberVariation(), jsonVariation() calls

; Method call on client object: client.variation("flag-key", ...)
(call_expression
  function: (member_expression
    property: (property_identifier) @method
    (#match? @method "^(variation|boolVariation|stringVariation|numberVariation|jsonVariation|variationDetail)$"))
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))

; Method call on ldClient: ldClient.variation("flag-key", ...)
(call_expression
  function: (member_expression
    object: (identifier) @client
    (#match? @client "^(ldClient|launchDarklyClient|client|featureFlags)$")
    property: (property_identifier) @method
    (#match? @method "^(variation|boolVariation|stringVariation|numberVariation|jsonVariation|variationDetail)$"))
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
