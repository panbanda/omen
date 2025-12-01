; LaunchDarkly Python SDK detection
; Detects variation(), bool_variation(), string_variation(), etc.

; client.variation("flag-key", user, default)
(call
  function: (attribute
    attribute: (identifier) @method)
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
(#match? @method "^(variation|bool_variation|string_variation|int_variation|float_variation|json_variation)$")

; ld_client.variation("flag-key", ...)
(call
  function: (attribute
    object: (identifier) @client
    attribute: (identifier) @method)
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
(#match? @client "^(ld_client|ldclient|client|feature_flags)$")
(#match? @method "^(variation|bool_variation|string_variation|int_variation|float_variation|json_variation)$")
