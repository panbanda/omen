; PostHog Go SDK detection
; Detects IsFeatureEnabled() and GetFeatureFlag() calls

; posthog.IsFeatureEnabled("flag-key", distinctId, groups)
(call_expression
  function: (selector_expression
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @method "^(IsFeatureEnabled|GetFeatureFlag|GetFeatureFlagPayload)$")

; client.IsFeatureEnabled("flag-key", ...)
(call_expression
  function: (selector_expression
    operand: (identifier) @client
    field: (field_identifier) @method)
  arguments: (argument_list
    .
    (interpreted_string_literal) @flag_key))
(#match? @client "^(posthog|posthogClient|client|analytics)$")
(#match? @method "^(IsFeatureEnabled|GetFeatureFlag|GetFeatureFlagPayload)$")

