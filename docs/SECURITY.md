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
`impact` have a field `security` for the symbol that they describe.

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

Omen resolves the action to a symbol. The class must be in the same chain of modules as the
route. For a route in `namespace :admin`, omen matches `Admin::UsersController`. It does not
match `Other::UsersController`. A route without a module matches only a class that is in no
module. If no class matches, the entry point has no symbol.

A guard is a `before_action` callback of the action. Omen reads the symbols, `only:`, and
`except:` of the call from the syntax tree. A `skip_before_action` call removes a guard for
the actions that it lists. The guards of a superclass in the repository, such as
`ApplicationController`, come first.

Omen treats the openings of a class with one chain of modules and one name as one class. It reads
the callbacks in the order of the files and then of the positions. If a superclass name belongs
to two different classes, omen reads no guards from the superclass.

A guard never claims more than omen can read. A `before_action` call gives no guard in two
cases. The first case is an `if:` or `unless:` option. The second case is an `only:` or
`except:` value that is not a literal list, as in `only: ACTIONS`.

A `skip_before_action` call with `if:` or `unless:` removes the guard for the actions that it
names. A `skip_before_action` call with a value that is not a literal list removes the guard
for each action.

A `before_action` call replaces an earlier callback with the same name, as Rails does. The
replaced callback is gone for each action. If `ApplicationController` has `before_action :auth`
and a child has `before_action :auth, only: :index`, then `auth` guards only `index`.

A callback in a class body follows the same rule for dead branches as a route does. A callback in
any other `if`, `unless`, or `case` is conditional. It adds no guard. A `skip_before_action` call
in it removes the guard for the actions that it names.

A controller path with a leading `/`, such as `to: "/users#show"`, ignores the module of the
`namespace`.

Omen skips each route that it cannot read exactly. It never guesses. It skips these forms:

- A path, a name, or an option that is not a literal, such as a string with interpolation.
- A route in a loop or in a block that omen does not know, such as `each` or `concern`.
- A route in a branch that a literal condition makes dead: `if false`, `if nil`,
  `unless true`, and the `else` branch of `if true`.
- A route in a method, a lambda, or a class body, and a route outside a `routes.draw` block.
- A `resources` call with an option that changes the paths in a way that omen does not
  read, such as `param:` or `shallow:`.
- A `scope` path with an optional segment, such as `"(:locale)"`.
- The calls `concerns`, `draw(:file)`, `mount`, and `devise_for`.

Omen reports a route in any other `if`, `unless`, `case`, or ternary. The condition can be true
at run time, as `if Rails.env.development?` can be.

A nested route needs the singular name of the parent resource. Omen uses the default
inflections of Rails 7.2.2. For `categories`, the parameter is `:category_id`. For `jeans`,
the word is uncountable, and the parameter is `:jeans_id`.

An application can change the inflections. Omen checks the files in `config/initializers`
and the files with `inflect` in the name. If one of them calls `inflections`, `irregular`,
`uncountable`, `acronym`, or `inflect.singular`, omen skips the nested routes. The `member`
routes and the `collection` routes stay. Comments do not change the inflections. Omen does
not find a change in a different file.

For a `resource` call, such as `resource :person`, the entry point has no symbol when the
plural controller name is irregular.

## Limits

- Omen does not follow data. A path is a chain of calls that the index resolves.
- A script and a Python `__main__` block are not symbols. An entry point of this kind
  starts no path.
- A route is an entry point only when its receiver is a framework object. The receiver
  is a framework call, a value that the file binds to one, or an import of a
  constructor of the framework, or of a repository file that binds the name to one.
- Deadcode treats an entry point and the entry points of `[[security.entry_points]]`
  as alive, with their callees.
- Deadcode reads the path in a Rust `#[serde(...)]` string through the module tree of
  the index. If a path does not resolve, deadcode keeps each function with the last
  name of the path alive, and the summary counts the path in `unresolved_paths`. This
  happens when an inline module declares a file module, as in
  `mod outer { pub(super) mod paths; }`, because the tree does not cross from the inline
  module to the file `outer/paths.rs`. The result misses a dead function of that name.
  It does not report a function that is alive.
