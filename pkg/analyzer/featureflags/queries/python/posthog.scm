; PostHog Python SDK detection
; Detects feature_enabled(), get_feature_flag(), etc.

; posthog.feature_enabled("flag-key", distinct_id)
((call
  function: (attribute
    attribute: (identifier) @method)
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
  (#match? @method "^(feature_enabled|is_feature_enabled|get_feature_flag|get_feature_flag_payload)$"))

; posthog.feature_enabled("flag-key", ...)
((call
  function: (attribute
    object: (identifier) @client
    attribute: (identifier) @method)
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
  (#match? @client "^(posthog|client|analytics)$")
  (#match? @method "^(feature_enabled|is_feature_enabled|get_feature_flag|get_feature_flag_payload)$"))
