; Split.io Python SDK detection
; Detects get_treatment() and get_treatment_with_config() calls

; client.get_treatment(key, "flag-key")
(call
  function: (attribute
    attribute: (identifier) @method)
  arguments: (argument_list
    (_)
    (string
      (string_content) @flag_key)))
(#match? @method "^(get_treatment|get_treatment_with_config|get_treatments|get_treatments_with_config)$")

; split_client.get_treatment("flag-key") - with identifier first arg
(call
  function: (attribute
    object: (identifier) @client
    attribute: (identifier) @method)
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
(#match? @client "^(split_client|split|client|factory)$")
(#match? @method "^(get_treatment|get_treatment_with_config)$")
