; Flipper Ruby gem detection
; Detects Flipper.enabled?(), Flipper.enable(), Flipper.disable(), etc.

; Flipper.enabled?(:flag_key)
(call
  receiver: (constant) @receiver
  method: (identifier) @method
  arguments: (argument_list
    .
    (simple_symbol) @flag_key))
(#eq? @receiver "Flipper")
(#match? @method "^(enabled\\?|enable|disable|enable_actor|disable_actor|enable_group|disable_group|enable_percentage_of_actors|enable_percentage_of_time)$")

; Flipper.enabled?("flag_key") - string variant
(call
  receiver: (constant) @receiver
  method: (identifier) @method
  arguments: (argument_list
    .
    (string
      (string_content) @flag_key)))
(#eq? @receiver "Flipper")
(#match? @method "^(enabled\\?|enable|disable)$")

; Flipper[:flag_key].enabled?
(element_reference
  object: (constant) @receiver
  (simple_symbol) @flag_key)
(#eq? @receiver "Flipper")

; flipper.enabled?(:flag_key) - instance method
(call
  receiver: (identifier) @client
  method: (identifier) @method
  arguments: (argument_list
    .
    (simple_symbol) @flag_key))
(#match? @client "^(flipper|feature_flags)$")
(#match? @method "^(enabled\\?)$")
