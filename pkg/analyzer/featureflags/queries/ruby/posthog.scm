; PostHog Ruby SDK detection
; Detects is_feature_enabled(), feature_enabled?(), and get_feature_flag() calls

; posthog.is_feature_enabled("flag-key", distinct_id)
((call
  receiver: (identifier) @client
  method: (identifier) @method
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
  (#match? @client "^(posthog|client|analytics)$")
  (#match? @method "^(is_feature_enabled|feature_enabled\\?|get_feature_flag|get_feature_flag_payload)$"))

; PostHog.is_feature_enabled("flag-key", ...)
((call
  receiver: (constant) @receiver
  method: (identifier) @method
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
  (#eq? @receiver "PostHog")
  (#match? @method "^(is_feature_enabled|feature_enabled\\?|get_feature_flag|get_feature_flag_payload)$"))

; is_feature_enabled(:flag_key, ...) - symbol variant
((call
  receiver: (identifier) @client
  method: (identifier) @method
  arguments: (argument_list
    .
    (simple_symbol) @flag_key))
  (#match? @client "^(posthog|client)$")
  (#match? @method "^(is_feature_enabled|feature_enabled\\?|get_feature_flag)$"))
