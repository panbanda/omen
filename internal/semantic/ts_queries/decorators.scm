; Method definition immediately following a decorator in class_body
; In tree-sitter, decorator is a sibling before method_definition
; Example: @Get() findAll() {}
; We match class_body that contains decorators, then capture method names
(class_body
  (decorator)
  (method_definition
    name: (property_identifier) @decorated_method))
