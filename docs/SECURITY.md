# Security analysis

The command `omen security` finds three things: the sinks (the calls and the
constructs that can cause harm), the entry points (the places where outside input
enters the code), and the paths from an entry point to a sink.

Omen does not follow data. A path says that a call chain exists. It does not say
that input reaches the sink. A sink with no known entry point is not safe: it means
that no entry point that omen knows reaches it through a call that the index
resolves. Each result carries a coverage statement that says what omen recognises.
An empty result does not mean that the code is safe.

## Commands

```bash
omen security                       # all sections
omen security sinks                 # the sinks
omen security surface               # the entry points
omen security paths                 # the paths from entry points to sinks
omen security paths --entry KEY     # the paths of one entry point
omen security paths --sink KEY      # the paths to one sink
omen security --category weak_hash  # the sinks of one category
omen security --min-basis same_file # use only the strongest call edges
omen security --experimental        # include the experimental categories
omen -f sarif security              # SARIF 2.1.0 for a code scanning service
omen -f mermaid security paths      # a Mermaid diagram of the paths
```

The MCP tool `security` has the same sections and filters. The tools `symbol` and
`impact` have a field `security` for the symbol that they describe. Each entry point in
these results has the fields `guards_complete` and `guards_unknown_reason`. The summary
has the number of the entry points with a guard list that is not complete, and the
number for each reason. The SARIF output has the two numbers in the properties of the
run.

## Sink categories

| Category | CWE | Example |
|---|---|---|
| command_execution | 78 | `subprocess.run(command, shell=True)` |
| code_evaluation | 95 | `eval(text)` |
| unsafe_deserialization | 502 | `pickle.loads(data)` |
| sql_string_construction | 89 | `cursor.execute("..." + name)` |
| weak_hash | 328 | `hashlib.md5(data)` |
| weak_cipher | 327 | `DES.new(key)` |
| tls_verification_disabled | 295 | `requests.get(url, verify=False)` |
| hardcoded_credential (experimental) | 798 | a token with a provider prefix |
| memory_unsafe | 119 | a Rust `unsafe` block |

A sink has a basis: `imported_symbol` (an import binds the callee), `builtin` (the
language gives the name), or `name_only` (only the name is evidence). A sink in test
code has the flag `in_test`. A call with only literal arguments has the flag
`constant_argument`.

The precision of each category on real repositories is in
`docs/benchmarks/security-precision.md`. A category with a precision below 50
percent is experimental: omen does not look for it unless you use `--experimental`.

## Suppression

A comment on the line of the sink, or on the line before it, suppresses a sink:

```python
os.system(command)  # omen:allow command_execution: the value comes from our own build
```

The reason is mandatory. A suppression with no reason, or with an unknown category,
does not suppress anything, and the output lists it in `invalid_suppressions`. The
output counts the suppressed sinks and lists them in `suppressed`.

## Configuration

```toml
[security]
gate = "off"                   # off, warn, or error
min_basis = "same_package"     # the weakest call edge of a path
gate_categories = []           # empty means each category
gate_basis = "builtin"         # the gate counts this basis or a stronger one
experimental = false           # include the experimental categories
```

The gate (`--gate`) compares the branch with its base and counts the sinks that the
change adds. You can add sinks and entry points for your own wrappers and
frameworks with `[[security.sinks]]` and `[[security.entry_points]]`. See
`omen.example.toml`.

## Pull requests and reports

`omen diff -f markdown` adds a Security section to the comment: the counts, the new
sinks, the new entry points, and one diagram for each new path. `omen report
generate` writes `security.json`, and the report shows the section "Attack
surface" with the coverage statement, a matrix, a Sankey diagram, the entry
points, and the choke points. A choke point is a symbol that each path to a
sink goes through.

## Guards that omen did not read

Each entry point has the field `guards`, and two fields that tell if the list is complete.

- `guards_complete` is `true` when omen read each place that can give the entry point a guard.
  Then `guards` is the full list, and an empty list tells that the entry point has no declared
  guard.
- `guards_complete` is `false` when code that omen did not read can add a guard, or can remove
  a guard that omen lists. Then `guards` has only the guards that omen read. An empty list does
  not tell that the entry point has no guard.
- `guards_unknown_reason` gives one reason when `guards_complete` is `false`. An entry point
  can have more than one reason. Omen gives the first one in the order of the table.

Do not conclude that an entry point is not authenticated when `guards_complete` is `false`.

| Reason | Framework | Meaning |
| --- | --- | --- |
| `unresolved_controller` | Rails | Omen did not find the controller class or the action of the route. |
| `unreadable_action` | Rails | The method of the action is not public, or omen cannot read its visibility. |
| `rack_endpoint` | Rails | The target of the route is an object that is not a controller action, as a Rack application or a lambda. |
| `unresolved_superclass` | Rails | A class of the chain has a superclass that omen cannot find. The base classes of Rails, such as `ActionController::Base`, do not count. |
| `included_module` | Rails | A class of the chain has `include`, `extend`, or `prepend`, in its body or from outside. A module can add callbacks with `included do`. |
| `callback_outside_class` | Rails | A callback call or `class_eval` in another place has the constant of a class of the chain as its receiver. |
| `unresolved_macro` | Rails | A class of the chain calls a class method that omen cannot resolve to a definition that it can read. |
| `unreadable_callback` | Rails | A callback call has a form or a place that omen cannot read: a name that is not a literal, a block or a lambda with no name, a list that is not a literal, a receiver, or `skip_callback`. |
| `conditional_callback` | Rails | A callback for the action has `if:` or `unless:`, or it is in a branch that omen cannot evaluate. |
| `around_callback` | Rails | A class of the chain has an `around_action` callback. Omen reads only the `before_action` callbacks. |
| `imported_router` | FastAPI | An import binds the router of the route, or the router that includes it. |
| `unresolved_router` | FastAPI, Go router types | FastAPI: the router is not one name with one assignment of a `FastAPI` call or an `APIRouter` call. Go: a part of the path is not known. The path starts with `<unresolved>`. |
| `included_elsewhere` | FastAPI | The file does not include the `APIRouter` in an application. A different file can include it with more dependencies. |
| `unreadable_include` | FastAPI | The file includes the router two times, in a condition, after the route, or in a loop of routers. |
| `unreadable_dependency` | FastAPI | A dependency is not a name, or the `dependencies` value is not a literal list. |
| `wildcard_import` | FastAPI | The file has `from module import *`, which can bind a name again. |
| `unread_middleware` | chi, echo, gin, gorilla/mux, Go router types | A middleware argument is not a name, or a spread of a slice that omen cannot read. Its guard text is `<expression>` or `<function literal>`. |
| `layer_not_read` | See the next table | The framework has a place for guards that omen does not read. |

Omen reads the full chain of the guards only for Rails, for FastAPI, and for the Go routers.
For each other HTTP framework, omen reads the guards that the route itself declares, and
`guards_complete` is always `false` with the reason `layer_not_read`.

| Framework | Guards that omen reads | Guards that omen does not read |
| --- | --- | --- |
| Rails | The `before_action` chain of the controller and of its superclasses, and the constraint objects of the routes | A guard of Rails itself, such as the CSRF check, and the Rack middleware of the application. These do not make the list incomplete. |
| FastAPI | The dependencies of the route, of its router, of the application, and of an `include_router` call of the file | A middleware. A file with `add_middleware` or `middleware` gives `layer_not_read`. A middleware in a different file is not seen. |
| chi, echo, gin, gorilla/mux, `net/http` | The middleware of `Use`, `Pre`, `With`, `Group`, and of the route call, and a wrapper of the handler | A wrapper of the whole router at the server, as `http.ListenAndServe(address, auth(router))`. It does not make the list incomplete. |
| Express | The middleware names in the route call | `app.use` and `router.use`, a middleware that is a call or a function, and the mount of a router |
| Flask | The known decorators below the route decorator | `before_request`, and a decorator that omen does not know |
| Django | The known decorators of the view function | The `MIDDLEWARE` setting, a mixin of a class-based view, and a wrapper in `path(...)` |
| Spring | The known annotations of the class and of the method | The security configuration, such as a `SecurityFilterChain` |
| ASP.NET | `[Authorize]` and `[AllowAnonymous]` on the class and on the method, and `RequireAuthorization` | A global filter, a fallback policy, and a route group |
| axum | None | `.layer(...)`, `.route_layer(...)`, and an extractor parameter such as `Extension<Principal>` |
| actix, rocket | None | `wrap`, a guard of a scope, a fairing, and a request guard parameter |
| Laravel | The `middleware(...)` calls of the route | A route group, the middleware of a controller, and the kernel |
| Next | None | The `middleware` file |

An entry point that is not an HTTP handler, such as `main` or a scheduled job, has no guard
concept. Its list is empty and complete.

## Rails routes

Omen reads the routes in each file whose name ends with `routes.rb`. Each route is an
entry point of the framework `rails`. Omen reads the syntax tree, not the text.

| Form | Entry points |
| --- | --- |
| `get`, `post`, `put`, `patch`, `delete "/path", to: "c#a"` | One route. The forms `controller: "c", action: "a"` and `"/path" => "c#a"` also work. |
| `match "/x", to: "c#a", via: [:get, :post]` | One route for each verb. `via: :all` gives the method `ANY`. |
| `root "pages#home"` or `root to: "pages#home"` | `GET /`. |
| `resources :users` | The seven routes of Rails, with `PATCH` and `PUT` for `update`. |
| `resource :profile` | The routes of a singular resource: no `index` and no `:id`. The controller is `ProfilesController`. |
| `only:` and `except:` | Limit the actions of a `resources` call or a `resource` call. |
| Nested `resources` | The path `/users/:user_id/posts/:id`. |
| `member do` and `collection do` | The paths `/users/:id/x` and `/users/x`. The option `on:` has the same effect. |
| `namespace :admin do` | The path prefix `/admin`. The controller is `Admin::UsersController`. |
| `scope "/api" do` and `scope module: "v1"` | A path prefix, and a module for the controller. |

Omen reads these options and blocks, and it skips each other form. The table gives the audit of
each form against the source of Rails 7.2.2.

| Form | Result |
| --- | --- |
| `draw` on `Rails.application.routes` | Read. |
| `draw` on the routes of a class of the repository with the superclass `Rails::Application` | Read. Omen finds the class as Ruby finds a constant: in the modules around the call from the inner one to the outer one, and then at the top level. A call in more than 64 modules is skipped. |
| `draw` on the routes of a class of the repository with the superclass `Rails::Engine` | Read only when omen reads exactly one `mount` call of the engine. See "Engines". |
| A `draw` call in a `draw` block | Skipped. Rails evaluates the block with a new mapper, so the blocks around the call do not apply. |
| `draw` on any other receiver | Skipped. |
| `namespace :x` with `path:`, `module:`, `as:`, `constraints:`, `defaults:`, `format:` | Read. `path: nil` adds no prefix. |
| `namespace` or `scope` with `shallow:`, `shallow_path:`, `shallow_prefix:`, `path_names:`, `via:`, `action:`, `to:`, `only:`, `except:`, `param:`, `concerns:`, `anchor:`, `on:`, `internal:`, or a `**` splat | The block is skipped. Such an option can change a path, a controller, or the set of the routes, and omen does not model it. |
| `scope` with `path:`, `module:`, `controller:`, `as:`, `constraints:`, `defaults:`, `format:` | Read. `path: nil` adds no prefix. A second positional path is skipped. A `path:`, `module:`, or `controller:` value that is not a literal skips the block. |
| `scope`, `namespace`, `resources`, or a route with a key that Rails does not know, as `id: /\d+/` or `id: Format.username` | Read. Rails makes the key a requirement or a default of each route. It changes no path and no controller. |
| `controller :x do` | Read. |
| `constraints do`, `defaults do` | Read. They change no path. |
| `constraints:` or `constraints(...)` with an object, as `constraints: StaffConstraint.new` | Read on `scope`, `namespace`, `constraints`, `resources`, `resource`, `mount`, and a route. Rails asks the object if the request matches, so the object is a guard of each route in its reach. The guard is the source text of the object, with one space for each run of white space. A lambda is `lambda:` and its line. The guards of the blocks come first, from the outer block to the inner block. Then the guard of the route comes, and then the callbacks of the controller. A guard with the text of an earlier guard is not repeated. A hash, as `constraints: { id: /\d+/ }`, constrains segments and request attributes. It is not a guard. |
| `format: true` on `scope`, `namespace`, `resources`, `resource`, or a route | Read. The path ends with `.:format`, as `Mapping.normalize_path` gives it. `format: false`, `nil`, a string, a symbol, and a regular expression add nothing to the path. Any other value skips the route or the block. The option of a route replaces the option of its blocks. |
| `each` and `each_with_index` on a literal array, and `each` and `each_pair` on a literal hash | Read. See "Loops over literal lists". |
| Devise blocks, `concern`, any other loop, and any block that omen does not know | The block is skipped. |
| `def get(*)`, `def post(*)`, `def put(*)`, `def patch(*)`, or `def delete(*)` with an empty body, as a statement of the `draw` block | Read. Rails evaluates the block with `instance_exec`, so the definition replaces the helper of the mapper of this `draw` call. From that statement on, a call of the helper gives no route, and `resources` and `resource` give no route for that request method. `match` does not use the helpers. |
| Any other definition of a method of the mapper in a `draw` block, and `alias`, `undef`, `define_method`, `define_singleton_method`, `alias_method`, `undef_method`, `remove_method`, `singleton_class`, `extend`, `instance_eval`, `class_eval`, or `module_eval` in it | The `draw` block gives no route. A definition with another name is a helper of the file. Omen reads no route from its body, and it changes nothing. |
| `get`, `post`, `put`, `patch`, `delete` with a string path or a symbol path | Read, with the options `to`, `controller`, `action`, `on`, `as`, `constraints`, `defaults`, `anchor`, and `format`. The options `path:` and `internal:`, `via:` on a helper, and a `**` splat skip the route. |
| A route with no controller in a `namespace` block or a `scope module:` block, as `get "settings"` in `namespace :site` | Read as `site#settings`. `add_controller_module` of Rails takes the module as the controller when the route and its blocks give none. |
| A string path of two or more parts with no `to:` and no `action:`, as `put "topics/bulk"` | Read as `topics#bulk`, as `get_to_from_path` of Rails gives it. Each part has only letters, digits, `_`, and `-`. A `-` becomes `_` in the controller and in the action. |
| A target that is not a controller action, as `redirect("/new")`, a Rack application, or a lambda | The route is an entry point, because the server answers it. It has no symbol, because no controller action runs. Its guards are the constraint objects of the route and of its blocks. |
| `match` with `via:` | Read. A call without `via:` is skipped. Omen knows only symbols: `:all` alone gives `ANY`, and another symbol gives its upper case form with dashes, as `:m_search` gives `M-SEARCH`. A string, such as `"all"`, and `:all` with another value give no entry. |
| `root` | Read. In a `resources` block the path is the collection path. In a `resource` block it is skipped. |
| `resources` and `resource` with `only:`, `except:`, `path:`, `controller:`, `module:`, `as:`, `constraints:`, `defaults:`, `concerns:`, `format:` | Read. The option `as:` gives the name for the nested parameter: `as: :articles` gives `:article_id`. The options `param:`, `shallow:`, `shallow_path:`, `shallow_prefix:`, `path_names:`, `via:`, `action:`, `to:`, `anchor:`, `on:`, and `internal:` skip the call. |
| `resources` or `resource` in a `member` or `collection` block | Read. Rails does not nest such a resource: its path follows the path of the block, as `/api/keys` for `resources :keys` in the `collection` block of `resources :api`. |
| `config.api_only = true` | Read for each route set. The body of the application class, a file in `config`, and `Rails.application.config` give the value for `Rails.application` and for the application class. The body of an engine class, and `Name::Engine.config`, give a value for an engine. If any engine assigns `api_only`, omen cannot know which route set the value governs, and it skips each `resources` and `resource` call of each route set. Any other receiver gives a value that omen cannot attribute. With the value `true`, the `resources` and `resource` calls have no `new` and no `edit` route, unless `only:` lists them. A value that is not `true` or `false`, an assignment in a branch that omen cannot evaluate, and an assignment that omen cannot give to one route set, skip each `resources` and `resource` call. |
| A string in double quotes or single quotes | Omen evaluates the string as Ruby does, before it escapes the path. The same evaluation reads the string of `inflect.acronym`. It models `\n \t \r \a \b \e \f \v \s`, the octal forms (the value is taken modulo 256, as Ruby does) and the `\x` forms, `\uXXXX`, `\u{...}`, and a backslash before another character. An interpolation skips the route, unless it is the name of a loop parameter alone, as `"#{root_path}/x"`. The forms `\c`, `\C-`, `\M-`, a line continuation, and the forms `%q` and `%Q` skip the route. |
| A route, a callback, an inflection call, or an `api_only` assignment under any node that omen does not model | Omen reads only statement lists, blocks of known calls, and conditions that it can evaluate. A route under an expression such as `false && get(...)`, an assignment, an argument, a `begin` block, or parentheses is skipped. A callback in such a place makes the guards of the class unknown. An inflection call or an `api_only` assignment in such a place makes the inflections or the value unreadable. |
| The block of `inflections` | Each statement must be a call `param.method(literals)` of a method that changes the inflections, a comment, or a condition or a `case` that omen can evaluate with branches of the same kind. Any other statement, such as `binding`, `eval`, `send`, a definition, or a condition that omen cannot evaluate, makes the inflections unreadable. A stored result of `ActiveSupport::Inflector.inflections` makes them unreadable too. |
| A controller string with an empty segment, a leading `/` only for the absolute form, or a character other than `a` to `z`, `0` to `9`, and `_` | The route has no symbol and no guard. Rails can build a class from such a string: `users/#show` gives `Users::Controller`. Omen does not model this, so it gives less than Rails does. The path stays. |
| `ActiveSupport::Inflector.inflections` with a locale | Only the call with no argument, `:en`, or `"en"` counts, because the names of the controller classes use the default locale. A call with another literal locale is ignored. A call with a locale that is not a literal makes the inflections unreadable. |
| `case` with a literal subject and literal `when` patterns | Omen keeps the branch that runs. A route in any other `case` is skipped. |
| A repeated option, as `to: "a#b", to: "c#d"`, or a repeated hash-rocket key | The last value wins, as in Ruby. |
| A hash-rocket target together with `to:`, `controller:`, or `action:` | The route has no symbol and no guard. Rails lets the hash-rocket value win over `to:`, and it reads a rocket value without `#` as a controller. Omen does not model this. |
| A target that is not a literal, as `to: TARGET`, `controller: NAME`, or `action: ACTION`, and a `to:` string with more than one `#` | The route has no symbol and no guard. The path stays. |
| A `before_action` call with a name that is not a literal or with a block and no name, a `skip_before_action` call with a name that is not a literal, a callback call with a receiver, `skip_callback`, `set_callback`, and `reset_callbacks` | The guards of the class, and of each class that inherits from it, are not complete, with the reason `unreadable_callback`. The guards that omen read stay in the list. |
| `around_action`, `prepend_around_action`, and `skip_around_action` | Not read. The guards of the class, and of each class that inherits from it, are not complete, with the reason `around_callback`. |
| `append_before_action` | Read as `before_action`. |
| `protect_from_forgery` and `skip_forgery_protection` | Read as `before_action :verify_authenticity_token` and as the skip of it, with the options `only:`, `except:`, `if:`, `unless:`, and `prepend:`. Rails 7.2.2 defines them in this way. |
| `http_basic_authenticate_with` | Read as a callback with the name `http_basic_authenticate_with`, with the options `only:`, `except:`, `if:`, and `unless:`. |
| A call of a class method in a class body, as `requires_plugin "chat"` | See "Class methods that register callbacks". |
| A superclass that omen cannot find in the repository, other than `ActionController::Base`, `ActionController::API`, `ActionController::Metal`, and `AbstractController::Base` | The guards of the class, and of each class that inherits from it, are not complete, with the reason `unresolved_superclass`. |
| A key with quotes and a colon, as `"only": :index`, `"if": :ok`, `"module": "admin"` | The key is a symbol, as in Ruby. Only a key with `=>` is a string key. |
| `include`, `extend`, `prepend`, or `class_eval` in a controller or in a superclass, and a call of a callback method, `include`, `extend`, `prepend`, or `class_eval` with the constant of a class as the receiver, in any file | The guards of that class, and of each class that inherits from it, are not complete. The reason is `included_module`, `callback_outside_class`, or `unreadable_callback`. A module can add or remove callbacks with `included do`, and omen does not read them. The guards that omen read stay in the list. |
| `config.api_only \|\|= true`, `&&=`, and any other operator assignment | The value is unknown, and omen skips each `resources` and `resource` call. |
| A name that a callback call repeats, as `before_action :a, :b, :a` | The last place of the name counts, as Rails does: the guards are `b`, `a`. |
| `Inflections.instance`, in any form | The inflections are unreadable. |
| `module ActiveSupport::Inflector` or a class in it, opened again | See "The inflector, opened again". |
| `concerns`, `draw(:file)`, `devise_for`, `direct`, `resolve`, and a `mount` of an application that is not an engine of the repository | No route. |
| A `scope` path or a resource `path:` with a parenthesis, as `"/a(:x)"` | The block is skipped. Rails rewrites the optional segments over the whole path, and omen does not model it. |
| A statement after `next`, `break`, `return`, `redo`, or `retry` in a list of the routes | Ruby does not run it, and omen reads no route from it. |
| A callback, an inflection literal, or an `api_only` assignment with a nesting deeper than 1,000 levels | The guards are not complete, and the inflections or the `api_only` value are unknown. A routes file with such nesting gives no `resources` or `resource` route. |
| `member do`, `collection do`, `on: :member`, `on: :collection` | Read. A canonical action name, such as `get :show`, adds no path segment. A `namespace` in a `member` or `collection` block keeps this rule. `on: :new` is skipped. |
| A route in a `scope` in a `resources` block | Read. The nested path follows the path of the scope: `/x/users/:user_id/y`. |

Omen gives the path as the mapper of Rails gives it. The mapper escapes the path as
`URI::RFC2396_Parser#escape` does: each byte that is not a letter, a digit, or one of
`-_.!~*'();/?:@&=+$,[]` becomes `%` and two upper case hexadecimal digits. Thus `%` becomes `%25`
and a space becomes `%20`. Then `Journey::Router::Utils.normalize_path` and `Mapper.normalize_path`
act: one leading `/`, no repeated `/`, and no trailing `/` except the root path. Omen does not
report the optional `(.:format)` segment.

Omen resolves the action to a symbol. The class must be in the same chain of modules as the
route. For a route in `namespace :admin`, omen matches `Admin::UsersController`. It does not
match `Other::UsersController`. A route without a module matches only a class that is in no
module. If no class matches, the entry point has no symbol.

A route matches only a public instance method. Rails does not run an action when the method is
not public. If the nearest class that defines the method has it as `private` or `protected`,
the route has no symbol and no guard. A `def self.x` method, a method in `class << self`, a method
after `private` or `protected`, and a method that `private :x` or `private def x` hides are not
public. In one file, the visibility calls run in the order of the source. If two files define
one method, or a `private :x` call in one file names a method of another file, the order of the
files is not known, and the route has no symbol and no guard. A `private`, `protected`, or
`public` call for a method that the class inherits changes the action of the class. If a class
calls `private :show`, the inherited `show` has no symbol and no guard. The same holds for a class with a
definition in a block, or a visibility call that omen cannot read. If no class defines the
method, a view can give the action, so the guards stay.

### Loops over literal lists

Omen reads a loop when each item of the list is a literal string or symbol. It reads the block one
time for each item, with the value of the block parameter. The forms are:

- `%w[users u].each do |root_path|`, `[:a, "b"].each { |name| ... }`, and `%i[a b].each`
- `%w[a b].each_with_index do |name, index|`, where `index` is the place of the item
- `{ "latest" => "list#latest" }.each do |path, action|` and `each_pair`

A parameter has a value in three places: as a path, a target, or an option value, as `get path, to:
action`; as a hash-rocket key; and as an interpolation that is the name alone, as
`"#{root_path}/:id"`. Any other use, as `"#{name.upcase}"`, skips the route.

Omen does not read a loop in these conditions:

- An item is not a literal, or the list is not a literal, as `Site.filters.each`.
- The block parameters are not plain names of the right number: one for an array, two for a hash
  or for `each_with_index`.
- The file assigns a value to a name of a parameter in any place, or uses the name as the receiver
  of a call or of `<<`. Then the value can change in the block.
- The loops around a route run it more than 4,096 times. Loops in each other multiply their sizes,
  and this limit keeps the cost in proportion to the size of the file.

Each entry point of a loop has the range of its call. The key of an entry point has its path when
two entry points of one call have one method.

### Engines

A route in `Blog::Engine.routes.draw` has a path that starts at the place where the application
mounts the engine. Omen reads the `mount` calls in the block of `draw`, `append`, or `prepend` on
the routes of the application, in each Ruby file, and in the blocks in it that omen can enter.
The forms are `mount Blog::Engine, at: "/blog"` and `mount Blog::Engine => "/blog"`, with the
options `as:`, `defaults:`, and `constraints:`. A `constraints:` object of the mount, and of the
blocks around it, is a guard of each route of the engine.

Omen gives the routes of an engine only when it reads exactly one mount of the engine. These
conditions give no route for the engine:

- no mount that omen reads, or two mounts
- a mount path that is not a literal, or that has a parenthesis
- an option other than `at:`, `as:`, `defaults:`, and `constraints:`
- a mount in a `resources` block or a `resource` block

Omen does not read a `mount` call in any other place, as in a method or in the `draw` block of
an engine. It does not read a mount with a target that is not a constant. If such a call mounts the engine
a second time, omen gives the routes of the first mount only.

`isolate_namespace Blog` in the body of the engine class gives the module `blog` to each route:
Rails sets the default scope to `underscore` of the name of the constant. An engine without the
call has no module. Omen reads one call with a constant of the repository, as a statement of the
class body. A second call, a call in another place, a `default_scope` in the class, and a name
that an acronym of the application can change give no route for the engine.

### The inflector, opened again

A file can open `ActiveSupport::Inflector` again, or a class or a module in it. Omen reads each
such opening. The names stay readable when the opening changes no method that gives a name.
These statements make the names unreadable: the nested routes are skipped, and a route has no
symbol and no guard.

- A definition of `camelize`, `underscore`, `singularize`, `pluralize`, `classify`,
  `constantize`, `safe_constantize`, `inflections`, `apply_inflections`, or of a method of the
  `Inflections` class that holds or changes the rules, such as `acronym`. A singleton method counts.
- `alias`, `alias_method`, `undef`, `undef_method`, `remove_method`, `attr_accessor`, or `delegate`
  with one of these names, or with a name that omen cannot read.
- `define_method` with one of these names, or with a name that is not a literal.
- A call of `inflections`, `acronym`, `plural`, `singular`, `irregular`, `uncountable`, `human`,
  or `clear`.
- `class_eval`, `module_eval`, `instance_eval`, `eval`, `include`, `prepend`, `extend`, `send`,
  `public_send`, `define_singleton_method`, `singleton_class`, `const_set`, and `refine`.

One form of `define_method` with a name that is not a literal keeps the names readable: a wrapper
that calls the method that it replaces. The statements before it must have
`alias_method old, name`, and the block must call `public_send(old, ...)`, `send`, or `__send__`.
The value of `old` must be a string with an interpolation and a literal part that is in none of
the method names above, as `"#{name}_without_cache"`. A cache has this form. Omen does not prove
that the wrapper returns the result of the old method. It takes the call of the old method as the
sign that the wrapper keeps the result.

A guard is a `before_action` callback of the action. Omen reads the symbols, `only:`, and
`except:` of the call from the syntax tree. A `skip_before_action` call removes a guard for
the actions that it lists. The guards of a superclass in the repository, such as
`ApplicationController`, come first.

Omen treats the openings of a class with one chain of modules and one name as one class. It reads
the callbacks in the order of the files and then of the positions.

Omen finds a superclass as Ruby finds a constant. A name with a leading `::` is the full name
of the class. Omen searches any other name, as `Base` or `Admin::BaseController`, in each module
around the `class` statement from the inner one to the outer one, and then at the top level. If
this search finds no class, the name can be the tail of the full name of exactly one class of
the repository. If the name is the tail of two classes, or of none, omen reads no guards from
the superclass, and the guards are not complete with the reason `unresolved_superclass`.

A guard never claims more than omen can read, within the forms in the audit table. A
`before_action` call gives no guard in two cases. The first case is an `if:` or `unless:` option. The second case is an `only:` or
`except:` value that is not a literal list, as in `only: ACTIONS`.

A `skip_before_action` call with `if:` or `unless:` removes the guard for the actions that it
names. A `skip_before_action` call with a value that is not a literal list removes the guard
for each action.

In each of these cases the callback can run, so the guards are not complete. The reason is
`conditional_callback` for a condition, and `unreadable_callback` for a list that is not a
literal. A callback with a literal `only:` list makes only the actions of the list not
complete.

### Class methods that register callbacks

An application can define a class method that calls `before_action`, and a controller can call
that method in its body. Omen reads a call without a receiver that is a statement of the class
body, or of a branch of a condition in it. A call in a block is not such a call.

Omen searches the name in the class and then in each superclass in the repository. A definition
is `def self.name` in a class body, or `def name` in `class << self`. The definition of the
nearest class decides. Omen reads the body of the definition as it reads a class body, and it
applies the callbacks at the place of the call.

- A body with no callback call changes nothing, as a method that only sets a variable.
- A body with `before_action :name` and literal options gives that callback.
- A body with `before_action do ... end`, or with a lambda, gives a guard with the name of the
  class method. `requires_plugin` of an application with such a body gives the guard
  `requires_plugin`.
- A call in a branch that omen cannot evaluate gives conditional callbacks.

The guards are not complete, with the reason `unresolved_macro`, in these conditions:

- No class of the chain defines the name, and the name is not in the list below.
- The class has two definitions of the name.
- The body calls `super`, calls a different class method without a receiver, or has a callback
  call that omen cannot read, as `before_action :check, options`.

These names register no `before_action` callback, and omen ignores a call of them: the
visibility calls and the attribute calls of Ruby, `require`, `delegate`, `class_attribute`,
`layout`, `helper`, `helper_method`, `rescue_from`, the `after_action` family, `wrap_parameters`,
`respond_to`, `etag`, `content_security_policy`, and `permissions_policy`. The constant
`CLASS_METHODS_WITHOUT_CALLBACKS` in `src/index/security/entry/rails.rs` has the full list.

A `before_action` call replaces an earlier callback with the same name, as Rails does. The
replaced callback is gone for each action. If `ApplicationController` has `before_action :auth`
and a child has `before_action :auth, only: :index`, then `auth` guards only `index`.

A callback in a class body follows the same rule for dead branches as a route does. A callback in
any other `if`, `unless`, or `case` is conditional. A callback in a block, such as `each do` or a
lambda, is also conditional. It adds no guard. A `skip_before_action` call in it removes the guard
for the actions that it names. Omen ignores a callback in a method.

A `before_action` call with `prepend: true`, and a `prepend_before_action` call, put the callback
at the front of the chain. If the `prepend:` value is not a literal, the callback adds no guard.

A controller path with a leading `/`, such as `to: "/users#show"`, ignores the module of the
`namespace`.

Omen reports a route in any `if`, `unless`, or ternary, except in a branch that a literal condition
makes dead: `if false`, `if nil`, `unless true`, and the `else` branch of `if true`. The condition
can be true at run time, as `if Rails.env.development?` can be. A route in a `case` is reported
only when the subject and each pattern are `true`, `false`, or `nil`. Omen skips a route in any
other `case`. Omen skips a route
in a method, a lambda, a loop that omen cannot expand, a class body, and outside a `draw` block.

A nested route needs the singular name of the parent resource. Omen uses the default
inflections of Rails 7.2.2. For `categories`, the parameter is `:category_id`. For `jeans`,
the word is uncountable, and the parameter is `:jeans_id`. A `resource` call uses the plural
of its name for the controller: `resource :wife` gives `WivesController`. Omen uses the plural
rules of Rails 7.2.2 with their irregular words and uncountable words.

An application can change the inflections. Omen checks the files in `config/initializers`
and the files with `inflect` in the name. If one of them calls `irregular`, `uncountable`,
`clear`, `inflect.singular`, or `inflect.plural`, omen skips the nested routes. It also gives no
controller for a `resource` call. The `member` routes and the `collection` routes stay. Comments
do not change the inflections. Omen does not find a change in a different file.

Omen reads a literal `inflect.acronym "API"` call in the same files. The receiver must be the
parameter of the block of `ActiveSupport::Inflector.inflections`, or the result of that call. The
constant must be exactly `ActiveSupport::Inflector`, with or without a leading `::`. If the
repository defines a module or a class with that name, omen does not trust it, and the class
names are not known. A call on another receiver has no effect. It builds the class name as
`camelize` of ActiveSupport does, so `api/users` gives `API::UsersController`. A call in a dead
branch has no effect. A call in any other `if`, `case`, loop, method, or block, except the block of
`inflections`, makes the inflections unreadable. If an `acronym` argument is not a string, or the
files call `clear`, omen does not know the class names. Then no route has a symbol or a guard.

## Python routes

Omen reads the routes of FastAPI, Flask, and Django.

| Framework | Entry point | Guards | Parameters |
| --- | --- | --- | --- |
| FastAPI | A decorator `get`, `post`, `put`, `patch`, `delete`, `head`, or `options` on a `FastAPI` value or an `APIRouter` value | The known decorators below the route decorator, then the declared dependencies | The parameters of the path, then the request parameters of the handler |
| Flask | A decorator `route`, or one of the verbs, on a `Flask` value or a `Blueprint` value | The known decorators below the route decorator | The parameters of the path |
| Django | A call of `path`, `re_path`, or `url` with a view that is one function of the repository | The known decorators of the view function | The parameters of the path |

The known decorators are `login_required`, `permission_required`, `user_passes_test`,
`jwt_required`, `token_required`, `auth_required`, `requires_auth`, `roles_required`, and
`roles_accepted`. Omen compares the last part of the name of the decorator.

### FastAPI dependencies

A guard of a FastAPI route is a declared dependency: the name `f` in `Depends(f)` or in
`Security(f)`. A guard is a declared fact. It does not mean that the handler is authenticated.
A dependency such as `Depends(get_db)` opens a database session, and omen lists `get_db` as it
lists `verify_key`. Omen does not know what a dependency does. The other frameworks have the
same rule: omen lists each `before_action` callback of Rails and each middleware of Express,
chi, echo, and gin.

Omen gives the guards in the order in which FastAPI adds the declared dependencies to the route.
`APIRouter.add_api_route` and `APIRouter.include_router` give this order. It is an order of
declarations. It is not the order in which the functions run: FastAPI runs a dependency of a
dependency first, and omen does not read the dependencies of a dependency. The order is:

1. The dependencies of the routers around the router, from the outer router to the inner one.
   For each `include_router` call, the dependencies of the router that gets the routes come
   before the dependencies of the call.
2. The dependencies of the router of the route.
3. The dependencies of the route decorator.
4. The dependencies in the parameters of the handler, in the order of the signature.

A name occurs one time in the list, at its first place.

| Form | Result |
| --- | --- |
| `dependencies=[Depends(f), Security(g, scopes=["a"])]` on the route decorator | Read. The guards are `f` and `g`. Omen does not report the scopes. |
| `dependencies=(Depends(f),)` | Read. The value can be a literal list or a literal tuple. |
| `dependencies=NAME`, or any other value that is not a literal | No guard. |
| An item of the list that is not a `Depends` call or a `Security` call, as `NAME` or `*NAMES` | The item gives no guard. The other items stay. The guards are not complete, with the reason `unreadable_dependency`. |
| `APIRouter(dependencies=[...])` and `FastAPI(dependencies=[...])` | Read for each route of the router. The receiver of the route decorator must be a name with one binding in its scope, and the binding must be an assignment of the call. |
| A router name with two bindings in its scope, a name in a `global` statement or a `nonlocal` statement, a router that an import gives, and a receiver that is not a name | No guard from the router. The guards of the route decorator and of the parameters stay. |
| `parent.include_router(router, dependencies=[...])` | Read for the routes of `router` in three conditions. The file has one `include_router` call for the router. The call is a statement of the body of the module or of the function that has the route decorator. The call comes after the route decorator. The route also gets the dependencies of `parent`, and the guards that the same rule gives to `parent`. |
| An `include_router` call on a receiver that is not a `FastAPI` value or an `APIRouter` value of the file, as a value of a different class or a router that an import gives | No guard from the call. |
| A route decorator after the `include_router` call | No guard from the call. FastAPI copies the routes of the router at the time of the call. |
| Two `include_router` calls for one router, a call in a condition, a loop, a class, or a different function, and routers that include each other | No guard from an `include_router` call. |
| An `include_router` call in a different file | Not read. The route has only the guards that its own file declares. A call in a different file can also serve the route without the dependencies of the call in this file. |
| A parameter default `= Depends(f)` or `= Security(f)` | Read. |
| `Annotated[T, Depends(f)]`, `typing.Annotated[...]`, and `Annotated` from `typing_extensions` | Read. With two annotations of FastAPI in one `Annotated`, FastAPI uses the last one, and omen does the same. |
| A name that the file binds one time to an `Annotated` type, as `UserDep = Annotated[User, Depends(f)]` or `type UserDep = ...` | Read. |
| A name for an `Annotated` type that an import gives | Not read. The parameter is in the list of the request parameters. |
| `Depends()` without an argument | No guard. FastAPI uses the type of the parameter as the dependency. Omen does not read it. The parameter is not a request parameter. |
| `Depends(Checker("admin"))`, a lambda, a subscript, or any dependency that is not a name or a dotted name | No guard. The parameter is not a request parameter. |
| `Depends(dependency=f)` | Read. |
| `from fastapi import Depends as D`, `import fastapi as fa` with `fa.Depends(f)`, and `from fastapi.params import Depends` | Read. The index gives the import of each name. |
| A function, a class, a parameter, or an assignment that binds `Depends` in the scope of the use, and a `Depends` from a module that is not `fastapi` | No guard. Omen resolves the name in the scopes of Python: the function, the functions around it, and the module. |
| Two imports of the same name of the same module, as in the branches of `if TYPE_CHECKING:` | One import. |
| `from module import *` | Omen does not read the names of a wildcard import. If a wildcard import binds `Depends` again, omen does not see it. |
| `Depends(f)` in a string or in a comment | No guard. Omen reads the syntax tree. |
| `router.dependencies.append(...)` and `dependency_overrides` | Not read. |
| `api_route`, `websocket`, `add_api_route`, and a route method that `getattr` gives | No entry. |
| The `prefix` of `APIRouter` and of `include_router` | Not read. The path is the path of the route decorator. |

### FastAPI request parameters

The parameters of a FastAPI route are the parameters of the path, in the order of the path, and
then the request parameters of the handler, in the order of the signature. A name occurs one
time. Omen gives the names only. It does not give the type or the source of a parameter.

| Form | Result |
| --- | --- |
| A parameter with a type, without a type, with a plain default, or with a default `Query`, `Path`, `Header`, `Cookie`, `Body`, `Form`, or `File` | A request parameter. The name is the name of the function parameter. Omen does not read `alias=`. |
| A body model, as `item: Item` | A request parameter. |
| A parameter that is a dependency, with a name or without one | Not a request parameter. FastAPI also reads the request parameters of the dependency function. Omen does not list them. |
| A parameter with the type `Request`, `Response`, `BackgroundTasks`, `WebSocket`, `HTTPConnection`, or `SecurityScopes`, or a response class such as `JSONResponse`, when an import from `fastapi` or `starlette` gives the name | Not a request parameter. FastAPI gives the object to the handler. |
| The same type name from a different module, or from a class of the file | A request parameter. |
| A string annotation that is one name, as `"Request"` | Omen resolves the name as it resolves an annotation without quotation marks. |
| `Annotated[Request, "text"]`, with no annotation of FastAPI in the metadata | Omen reads the base type. A framework type is not a request parameter. |
| A type around a framework type, as `Optional[Request]` | A request parameter. |
| `*args` and `**kwargs` | Not listed. |

### Flask and Django guards

| Form | Result |
| --- | --- |
| A known decorator below the route decorator of Flask or FastAPI | A guard. The route gets the function with the decorator. |
| A known decorator above the route decorator | No guard. The framework registers the function before the decorator wraps it. |
| A known decorator on the view function of a Django route, in any file of the repository | A guard. Each decorator wraps the function before `path` gets it. |
| A qualified view, as `views.detail` | The view is the one function with the name in the module that the import of `views` gives. A function with the same name in the file of the route is not the view. If the index did not resolve the import, the view is the one function with the name in the repository. With two such functions, the route gives no entry. |
| `path("x/", login_required(view))`, a class-based view with `as_view()`, and `include(...)` | No entry. |
| `before_request` of Flask, a middleware of Django, and a mixin such as `LoginRequiredMixin` | Not read. Thus the guards of each Flask route and each Django route are not complete, with the reason `layer_not_read`. |
| A handler of Flask | The handler gets only the parameters of the path. Omen lists no other parameter. |

## Limits

- Omen does not follow data. A path is a chain of calls that the index resolves.
- A script and a Python `__main__` block are not symbols. An entry point of this kind
  starts no path.
- A route is an entry point only when its receiver is a framework object. The receiver
  is a framework call, a value that the file binds to one, or an import of a
  constructor of the framework, or of a repository file that binds the name to one.
- Omen reads the routes of these Go frameworks: `net/http`, gin, chi, echo, and
  gorilla/mux. Omen models the registrations of a valid program. It does not model a
  framework panic. A program that panics at start, as chi does when `Use` follows a
  route in some versions, is out of scope.
- A router type of the repository is a type whose route methods send their pattern to a
  router of one of these frameworks, in a field of the type. The body of each method is the
  evidence. A name of a type is not evidence. Gitea has such a type, `modules/web.Router`.
- A call of a route method of such a type registers a route. The last argument is the
  handler, and the arguments before it are guards. A group method that keeps its prefix in
  a field adds the prefix and its guards to each route inside its function. A combo method
  keeps the pattern for each method of its chain.
- Omen follows such a router through parameters, returned values, and mounts in the whole
  repository. A route gets one path for each chain of mounts and calls up to a root. A
  router that only a parameter gives, and that no call gives, has no path. A prefix that is
  not a string literal, a cycle of calls, and a router with more than 256 chains give one
  entry for each route. Its path starts with `<unresolved>` in place of the part that omen
  cannot read, and its guards are not complete, with the reason `unresolved_router`.
- Omen does not read the code of a method that changes the path. For example, Gitea removes
  a slash at the end of a pattern, so 16 paths of Gitea that end with a slash have that
  slash in the result of omen, but not in Gitea.
- A slice of guards is read when it is a literal, an `append` of known slices, or a variadic
  parameter. The guards of a variadic parameter come from each call of the function, also
  through a chain of calls. A change of a slice in a condition, in a loop, in a function
  literal, or at an index makes its guards not complete, with the reason
  `unread_middleware`. The guards that omen read stay in the list.
- A method of a router type that keeps its variadic argument in a field, or gives it to the
  framework router, as `Use`, `BeforeRouting`, or `AfterRouting`, adds middleware that omen
  does not read. The guards of each route of that router, and of each router that it mounts,
  are not complete, with the reason `layer_not_read`. A test file gives no route and no
  mount.
- A router is a value that the import of the framework proves. It is a constructor
  call, a group or sub-router of a router, or a parameter that has a router type. A
  variable name alone does not prove a router.
- Omen reads the body of each function one time. A route call gives its entry at its
  own place, with the guards that the function knows.
- A function that makes a router and returns it has a summary when it is pure. A pure
  function writes no package-level variable, no parameter, and no router that it did
  not make. The summary has the prefixes and the guards of the returned router. A call
  of the function gives a new router with these prefixes and guards.
- A function or a method can register routes on a router that it makes and returns. This
  function is a factory. The walk finds a factory from the return statement. The result
  type does not matter. A literal that holds the router, as in `&Server{Mux: router}`,
  also counts.
- The paths of the routes of a factory depend on the call. A call can mount the result
  at a prefix. Omen reads each use of the factory in the whole repository. It uses the
  references of the index and the syntax of each file.
- A use of a factory has one of three kinds. The first kind serves the result as the root.
  Examples are `http.ListenAndServe(address, router)`, a literal `http.Server{Handler: router}`,
  and a call that discards the result. A local variable that only calls methods or serves
  the result is the same kind.
- The second kind attaches the result. The call passes the result as an argument, returns
  it, or stores it in a field, a package variable, or a literal. The third kind is a use
  that omen cannot classify.
- A caller can add middleware to the result of a factory with `Use` or `Pre`. This
  changes the guards of the routes of the factory. Such a call is a use of the second
  kind. A call of a method that serves the router is not. A call that registers a route
  or makes a group on the result adds routes to the router of the factory. When the
  factory has one call, and the call is in the file of the factory, the routes of the
  factory come first. The routes of the caller follow, and the rules of the framework for
  the same key apply to them all. When the factory has more than one call, or a call in
  another file registers a route, omen cannot order the routes. Then the factory and the
  callers give no entry for them. A factory without routes of its own gives the entries of
  the caller. This rule holds only when the factory returns the router of the framework.
- A factory can return a literal that holds a router, as `&Server{Mux: router}`. The
  type of the literal can define a method that hides a method of the router. Thus each
  use of the result that is not serving is a use of the second kind.
- A handler name in a Go file has a package. A name with a qualifier, as `dep.Show`,
  names a function of the package of the import. The index must resolve that import.
  If it does not, the entry has no handler. A name without a qualifier names a function
  of the package of the file. A package is the files of one directory. A name never
  names a method, so a method that has the name is not the handler.
- A local binding can hide a name. A parameter, a short variable declaration, a
  variable, a range variable, and a type switch variable are local bindings. The scope
  of a parameter is its function. The scope of another binding starts after its
  declaration and ends with its block, its statement, or its case clause. A handler name
  that has a local binding in scope has no handler.
- A value of a type that is not a type of the framework is not a router. This holds for a
  struct that embeds or holds a router or a multiplexer. A wrapper type can define a
  method with the name of a method of the framework, or the method `ServeHTTP`. Then its
  values are not routers, and the routes of its factory give no entry, also when a call
  serves the result as the root. Omen compares the name of the type without a pointer,
  a package, and type arguments, so `Server[int]` and `Server[T]` are the type `Server`.
  Omen follows a type alias, as in `type Alias = Server`, and a chain of aliases, to the
  base type. A type has the key of its package, which is a directory, and its name. An
  alias and its base type are in the same package. A key that two declarations have gives
  no entry. An alias to a type of another package, or to a type that is not a name, gives
  no entry when a factory uses it as a wrapper. A cycle of aliases gives no entry too.
- A branch that never runs gives no entry. This holds for the body of `if false`, the
  `else` branch of `if true`, and the body of `for false`. This holds for the routes of
  `net/http` too. A route under another condition or another loop keeps its entry.
- chi `Mount` and `Route` register their handler for all methods on the pattern and on the
  pattern and a slash, and on the pattern and `/*`. These registrations replace the routes
  of the same keys that come before them, also when omen cannot read the mounted handler.
  A `Group` with a function that a name gives is an unread registration of the tree.
- A gorilla/mux sub-router that omen cannot read, a sub-router with a `NotFoundHandler` or a
  `MethodNotAllowedHandler`, and a router with `StrictSlash(true)` give no entry. The first
  two hide the later routes of the parent too. A template that the router would clean,
  as `/a//b` or `/a/../b`, gives no entry. An echo or gin `Group` with a prefix that omen
  cannot read is an unread registration of the router.
- A later registration can replace or hide an earlier one. chi and echo keep the last
  registration of a method and a path. A chi router that `With` or `Group` made shares
  the tree of its parent, and `Handle` has all methods. A registration that has only some
  of its methods left gives no entry. echo treats two paths as equal when only the names
  of the parameters differ. chi does the same for its own patterns: `/{id}` and `/{name}`
  have one node, and a regular expression stays part of the node. gin and `net/http`
  panic on a second registration, which is out of scope.
- gorilla/mux gives a request to the first route that matches. A route gives no entry when
  an earlier route of its router can match a path that it matches. Omen compares the
  literal start of the paths, up to the first variable, and the methods. A route with a
  variable that has no constraint is not hidden by a route with an exact path, so
  `/u/me` before `/u/{id}` gives two entries. A matcher such as `Headers` does not make
  a route safe. The order of the routers of one tree is not known, so a route of another
  router of the tree counts as earlier. A registration that omen cannot read, as a path
  that is not a literal or a call such as `Host`, can match any request. Then the routes
  after it, and the routes of the other routers of the tree, give no entry. A route that
  never matches, as a route with an error or `BuildOnly`, hides nothing. This check can
  drop a route that a request reaches.
- chi `Mount` registers the pattern, the pattern and a slash, and the pattern and `/*` for
  all methods. A route of the same tree that has one of these keys replaces the mount
  endpoint of its method on that node only. An exact `/x` route does not replace the
  endpoint `/x/*`, which serves the routes below the mount. Omen gives no entry for the
  routers below the mount in all of these cases. This rule drops more than the library
  replaces.
- A route of the parent tree whose path is below a mount prefix, as `/api/x` below `Mount("/api")`,
  takes the request before the catch-all of the mount. Then the routers below the mount
  give no entry. A route of the parent that is not below the prefix has no effect on them.
- chi tries the children of a node in the order static, regular expression, parameter, and
  catch-all, and goes back when a child fails. When a node has a regular expression child,
  its parameter child and its catch-all child give no entry. A catch-all child gives no
  entry when the node has a parameter child. A node with two different regular expressions
  gives no entry for either. Omen does not check if a regular expression accepts all that a
  parameter accepts.
- A chi registration that omen omits still takes part in the precedence of its tree. It
  can have a spread middleware list or an unread handler. A chi registration whose path
  omen cannot read, such as a variable path, hides all routes of its tree and of the
  routers mounted on it. echo replaces a route that has the same method and path, so an
  echo registration whose path omen cannot read hides all routes of its echo router. An
  echo route with a method that omen cannot read replaces the earlier routes of its path
  for every method, a custom method too, because echo stores each method separately.
- echo tries the children of a node in the order static, parameter, and wildcard, and goes
  back when a child fails. A parameter at the end of a path takes the rest of the path. So
  an echo wildcard route gives no entry when its node has a parameter child. This rule is
  over-careful for a parameter that has children, and it drops that wildcard too.
  gin and `net/http` panic on a second registration, and gin lets a static child and a
  parameter child coexist, so an unread registration hides nothing there.
- In the gin tree of the pinned source, a static child and a parameter child coexist, and
  the static child goes first. A catch-all and a parameter at the same node make gin panic,
  and so does a catch-all that has a sibling that gin has already stored. A valid program
  has no gin route that a sibling hides. In `net/http`, patterns that match the same
  requests conflict and make `ServeMux` panic, so one more specific pattern does not cover
  a pattern whole. A set of more specific patterns can cover it, as `/x/{$}`, `/x/{a}`, and
  `/x/{a}/{b...}` cover `/x/{a...}`. Omen checks only one case: a final `{name...}` that
  has a sibling `{$}` and a sibling `{name}` in the same file gives no entry. It ignores
  methods and hosts for this check, and it does not check other sets.
- A router that a call gets, other than a framework call or a server, can get routes that
  omen does not read. That call is an unread registration at its place, for chi, echo, and
  gorilla/mux. A function that has a root router as a parameter, as `*echo.Echo`, and that
  a reference names anywhere in the repository gives no entry, and so does a function
  literal that has a router parameter. A function that nothing references keeps its entries.
  `Handle`, `HandleFunc`, and the servers of `net/http` do not count as such a call.
- echo `File`, `FileFS`, `Static`, and `StaticFS` register a GET route for the path, or for
  the prefix and `*`. Omen omits the entry, because it has no handler, and the route
  replaces a route of the same path. A call of `Router().Add` is an unread registration of
  the echo router. A `*` in the middle of an echo path stores the route on the node of the
  `*`, so omen reads the path up to the `*` and omits the entry. The method
  `echo_route_not_found` is not a route, and omen ignores it.
- A name that the package declares hides a predeclared name or a qualifier. The package is
  the files of one directory that have the same package clause, so a declaration in
  another file of the package counts.
- A package qualifier counts only when no name hides it. The qualifiers are `http`,
  `chi`, `mux`, `echo`, `gin`, and the aliases that the imports give. A parameter, a
  receiver, a local variable, a range variable, or a declaration of the package with the
  same name hides the qualifier. Then the call is not a call of the library, and omen
  gives no entry. A parameter name is the text that the framework reads, with the
  spaces that it has.
  A multiplexer of `net/http` is a variable that has one binding in the file, and the
  binding is a call of `http.NewServeMux()`.
- Omen reads a string literal as Go does. An interpreted literal has the escapes `\a`,
  `\b`, `\f`, `\n`, `\r`, `\t`, `\v`, `\\`, `\"`, octal, `\x`, `\u`, and `\U`. A raw
  literal has no escape. A bad escape, or a value that is not UTF-8, gives no entry.
- A gorilla/mux route can have an error that stops it from matching. A template with
  unbalanced braces, an empty name or pattern, an odd number of arguments in `Headers`,
  and a call of `Name` on a route that has a name that is not empty set this error. A call
  of `Name` with an empty string leaves the name empty, so the next `Name` is valid. A
  pattern that Go does not accept as a regular expression sets the error too.
- Omen checks a pattern with a small grammar that Go and Rust read in the same way. It
  has ASCII letters, digits, and escaped punctuation, the dot, classes of letters,
  digits, and ranges, the repetitions `*`, `+`, `?`, `{m}`, `{m,}`, and `{m,n}` with
  numbers up to 1000, a lazy `?`, and `|`. A pattern outside this grammar gives no
  entry. Thus a group, a flag, an anchor, and a shorthand class such as `\d` give no
  entry.
- A capture group in a pattern makes gorilla/mux v1.8.1 panic at start. A program that
  panics at start is out of scope, and omen gives no entry for a pattern with a group.
- Omen accepts these calls in a chain of gorilla/mux: `Methods`, `Path`, `PathPrefix`,
  `Headers`, `Name`, `Schemes`, `MatcherFunc`, and `BuildVarsFunc`. It checks the
  templates of the paths. A route with `Host`, `Queries`, `HeadersRegexp`, or another
  call gives no entry.
- A handler with a qualifier that is not an import has no function. A package variable
  or a field, as `box.Show`, is an example. The index does not give a function for it.
- Any mention of the name of a factory that is not a call is a use of the third kind. A
  function value, a method value, and an argument that is a function are examples. A
  mention of another function with the same name also counts.
- If one use is of the second or the third kind, omen gives no entry for the routes of
  the factory. A factory that no use in the repository mentions keeps its routes at the
  paths of the function. The function can be an entry of a library.
- A function without a summary that returns a router it made gives no entry for these
  routes. A function that returns a group or a sub-router gives none either.
- These values have no proof, and the routes on them give no entry:
  - the result of a function without a summary
  - the result of a recursive function
  - a package-level router that a function assigns
  - a router that a function receives as a parameter of a type that can be a group or
    a sub-router, such as `chi.Router`
- A function that receives a root router, such as `*gin.Engine`, gives entries. A route
  callback that a name gives, as in `r.Route("/api", api)`, gives no entry. A function
  literal gives entries.
- A call can wrap a handler, as in `http.Handle("/p", auth(http.HandlerFunc(show)))`.
  The function `show` is the handler when a conversion proves it. It is also the handler
  when its parameter list is the parameter list of a handler of the framework. For
  `net/http` it is `(http.ResponseWriter, *http.Request)`. For gin it is
  `(*gin.Context)`. For echo it is `(echo.Context) error`. A wrapper with more than
  one such function gives no handler. A wrapper is a guard only when the repository
  defines it as a function that takes a handler and gives a handler. A conversion such
  as `http.HandlerFunc` or `gin.WrapF` is not a guard.
- Two entries can have the same place and the same method. Then the key of each entry
  has the path at its end. If the keys are still equal, each key has the number of its
  entry in the walk. A router that two mounts attach does not give such entries,
  because it gives no entry.
- For chi, a prefix comes from `Route`, which also gives the new router. A guard comes
  from `Use` and `With`. `Use` guards each route of its router and of its sub-routers,
  in any source order. This holds for a router that `New` or `NewMux` makes. A router that
  `With` or `Group` makes has the guards of its parent at its creation, and a later `Use`
  of the parent does not guard its routes. `Mount` joins a router of the same file. A router that two mounts
  attach gives no entry. A router of another file is not joined. A pattern without a
  leading slash gives no entry.
  `Query` gives the method `QUERY`.
- For echo, a prefix and the guards of `Group` apply to the routes of the group. A
  group copies the guards of its parent when the group is made. `Use` and `Pre` on the
  echo value apply to each route. The path is the plain concatenation of the prefixes
  and the path. `Any` gives one entry for each method in the list of echo v4.13.4.
  `Match` gives entries only for a literal list of strings.
- For gin, the guards of a route are the arguments between the path and the last
  argument. A group has a prefix, and it copies the guards of its parent when the
  group is made. `Use` gives a guard to the routes that follow it. Each method that adds
  a route or a guard returns the router, so calls can chain. Paths join as `path.Join`
  does. `Any` gives one entry for each method in the list of gin v1.11.0. `Handle` gives
  one entry. `Match` gives entries only for a literal list of strings.
  gin and echo keep a custom method of `Handle`, `Match`, and `Add` as the source
  writes it.
- For gorilla/mux, `PathPrefix(...).Subrouter()` gives a prefix. A chain of matchers
  that ends in `HandlerFunc` or `Handler` gives a route, as `r.Methods("GET").Path("/x")
  .HandlerFunc(show)` does. A chain without `Path` or `PathPrefix` gives no entry. A chain with more than one
  `Path` or `PathPrefix` call gives no entry, because gorilla/mux needs each matcher to match.
  A `Schemes` call needs literal values `http` or `https`, in any case. A handler that
  runs before the router can set another value in `URL.Scheme`, so omen gives no entry for
  other values. A `Methods` call needs literal values that are HTTP tokens. Other values
  give no entry. `PathPrefix("")` matches
  every path, so it hides the routes after it. The
  path matchers join in the
  order of the calls. Each `Methods` list limits the methods, so the result is the
  common methods of the lists. A route without `Methods` has the method `ANY`. `Use`
  applies to each route of its router and of the sub-routers, as the library does.
  A `Methods` argument that is not a string literal gives no entry.
  A chain that ends in `BuildOnly()`, or that has another call that omen does not know,
  gives no entry. A `BuildOnly` route never matches a request.
- chi v5.3.2 reads a method before a space or a tab in the pattern of `Handle`. It
  accepts only the methods `CONNECT`, `DELETE`, `GET`, `HEAD`, `OPTIONS`, `PATCH`,
  `POST`, `PUT`, `QUERY`, and `TRACE`. Another method needs `RegisterMethod`, so it
  gives no entry.
- Each framework has its own syntax for the parameters of a path. chi and gorilla/mux
  read `{name}` and `{name:expression}`, and chi also reads `*`. echo reads `:name` and
  `*`, and gin reads `:name` and `*name`. A backslash before `:` makes it a letter in
  echo and gin. A colon is a letter in chi and gorilla/mux. A brace is a letter in
  echo and gin.
- net/http follows the syntax of Go 1.24 for `[METHOD ][HOST]/[PATH]`. A method that is
  not a valid token, a missing slash, a bad wildcard, or a path that the library
  changes when it cleans the path gives no entry. A method keeps its case, so `get`
  does not match a request. A host does not change the path. A multiplexer is a
  variable that has one binding in the file, and the binding is a call of
  `http.NewServeMux()`.
- A list with `...` in `Use`, `Pre`, `With`, or `Group`, or in the middleware of a route,
  is not known. The routes that it changes give no entry. A function literal or another
  expression in these lists has the label `<function literal>` or `<expression>`.
- gin `Default()` adds the middleware `Logger` and `Recovery`. The source does not name
  them, so omen does not list them as guards.
- A function is a factory if any return statement can return a router that the function
  made. A bare `return` gives the value of each named result. Each return statement
  counts, so the walk takes the union of the routers.
- Omen does not recognise these forms. They are coverage gaps, and they give no entry:
  - a route that a caller registers on the result of a factory of another file
  - an echo group that `Host` makes
  - a gorilla/mux route that `NewRoute()` builds
  - the gin methods `Static`, `StaticFile`, `StaticFileFS`, and `StaticFS`
- Deadcode treats an entry point and the entry points of `[[security.entry_points]]`
  as alive, with their callees.
- Deadcode reads the path in a Rust `#[serde(...)]` string through the module tree of
  the index. If a path does not resolve, deadcode keeps each function with the last
  name of the path alive, and the summary counts the path in `unresolved_paths`. This
  happens when an inline module declares a file module, as in
  `mod outer { pub(super) mod paths; }`, because the tree does not cross from the inline
  module to the file `outer/paths.rs`. The result misses a dead function of that name.
  It does not report a function that is alive.
