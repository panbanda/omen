; PostHog Java SDK detection
; Detects isFeatureEnabled() and getFeatureFlag() calls

; posthog.isFeatureEnabled("flag-key", distinctId)
((method_invocation
  name: (identifier) @method
  arguments: (argument_list
    .
    (string_literal) @flag_key))
  (#match? @method "^(isFeatureEnabled|getFeatureFlag|getFeatureFlagPayload)$"))

; client.isFeatureEnabled("flag-key", ...)
((method_invocation
  object: (identifier) @client
  name: (identifier) @method
  arguments: (argument_list
    .
    (string_literal) @flag_key))
  (#match? @client "^(posthog|posthogClient|client|analytics)$")
  (#match? @method "^(isFeatureEnabled|getFeatureFlag|getFeatureFlagPayload)$"))
