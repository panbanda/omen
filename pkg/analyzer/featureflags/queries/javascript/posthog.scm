; PostHog JavaScript/TypeScript SDK detection
; Detects isFeatureEnabled(), getFeatureFlag(), and onFeatureFlags() calls

; posthog.isFeatureEnabled("flag-key")
(call_expression
  function: (member_expression
    property: (property_identifier) @method)
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
(#match? @method "^(isFeatureEnabled|getFeatureFlag|getFeatureFlagPayload)$")

; posthog.isFeatureEnabled("flag-key")
(call_expression
  function: (member_expression
    object: (identifier) @client
    property: (property_identifier) @method)
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
(#match? @client "^(posthog|posthogClient|analytics)$")
(#match? @method "^(isFeatureEnabled|getFeatureFlag|getFeatureFlagPayload)$")

; useFeatureFlagEnabled("flag-key") - React hook
(call_expression
  function: (identifier) @hook
  arguments: (arguments
    .
    (string
      (string_fragment) @flag_key)))
(#match? @hook "^(useFeatureFlagEnabled|useFeatureFlagPayload|useFeatureFlagVariantKey)$")
