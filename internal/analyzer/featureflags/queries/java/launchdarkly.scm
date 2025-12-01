; LaunchDarkly Java SDK detection
; Detects boolVariation(), stringVariation(), intVariation(), etc.

; client.boolVariation("flag-key", context, default)
(method_invocation
  name: (identifier) @method
  (#match? @method "^(boolVariation|stringVariation|intVariation|doubleVariation|jsonValueVariation|boolVariationDetail|stringVariationDetail|intVariationDetail|doubleVariationDetail|jsonValueVariationDetail)$")
  arguments: (argument_list
    .
    (string_literal) @flag_key))

; ldClient.boolVariation("flag-key", ...)
(method_invocation
  object: (identifier) @client
  (#match? @client "^(ldClient|client|featureFlags)$")
  name: (identifier) @method
  (#match? @method "^(boolVariation|stringVariation|intVariation|doubleVariation|jsonValueVariation)$")
  arguments: (argument_list
    .
    (string_literal) @flag_key))

; @FeatureFlag annotation
(annotation
  name: (identifier) @annotation_name
  (#match? @annotation_name "^(FeatureFlag|Toggle|Flag|LDFlag)$")
  arguments: (annotation_argument_list
    (string_literal) @flag_key))
