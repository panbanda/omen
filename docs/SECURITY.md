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

Omen reads these options and blocks, and it skips each other form. The table gives the audit of
each form against the source of Rails 7.2.2.

| Form | Result |
| --- | --- |
| `draw` on `Rails.application.routes` | Read. |
| `draw` on `Name::Application.routes` or `Name::Engine.routes` | Read only when the repository defines `Name::Application < Rails::Application` or `Name::Engine < Rails::Engine`. |
| `draw` on any other receiver | Skipped. |
| `namespace :x` with `path:`, `module:`, `as:` | Read. |
| `namespace` with any other option, such as `only:`, `except:`, `constraints:` | The block is skipped. |
| `scope` with `path:`, `module:`, `controller:`, `as:`, `constraints:`, `defaults:` | Read. A second positional path is skipped. |
| `scope` with any other option | The block is skipped. |
| `controller :x do` | Read. |
| `constraints do`, `defaults do` | Read. They change no path. |
| Devise blocks, `concern`, loops, `each`, and any block that omen does not know | The block is skipped. |
| `get`, `post`, `put`, `patch`, `delete` with a string path or a symbol path | Read, with the options `to`, `controller`, `action`, `on`, `as`, `constraints`, `defaults`, `anchor`, `format: false`, and a constraint with a regular expression. Any other option skips the route. |
| `match` with `via:` | Read. A call without `via:` is skipped. |
| `root` | Read. In a `resources` block the path is the collection path. In a `resource` block it is skipped. |
| `resources` and `resource` with `only:`, `except:`, `path:`, `controller:`, `module:`, `as:`, `constraints:`, `defaults:`, `concerns:`, `format: false` | Read. The option `as:` gives the name for the nested parameter: `as: :articles` gives `:article_id`. Any other option, such as `param:` or `shallow:`, skips the call. |
| `config.api_only = true` | Read for each route set. The body of the application class, a file in `config`, and `Rails.application.config` give the value for `Rails.application` and for the application class. The body of an engine class, and `Name::Engine.config`, give the value for the routes of that engine. Any other receiver gives a value that omen cannot attribute. With the value `true`, the `resources` and `resource` calls have no `new` and no `edit` route, unless `only:` lists them. A value that is not `true` or `false`, an assignment in a branch that omen cannot evaluate, and an assignment that omen cannot give to one route set, skip each `resources` and `resource` call. |
| A string in double quotes or single quotes | Omen evaluates the string as Ruby does, before it escapes the path. The same evaluation reads the string of `inflect.acronym`. It models `\n \t \r \a \b \e \f \v \s`, the octal forms (the value is taken modulo 256, as Ruby does) and the `\x` forms, `\uXXXX`, `\u{...}`, and a backslash before another character. An interpolation, `\c`, `\C-`, `\M-`, a line continuation, and the forms `%q` and `%Q` skip the route. |
| A route, a callback, an inflection call, or an `api_only` assignment under any node that omen does not model | Omen reads only statement lists, blocks of known calls, and conditions that it can evaluate. A route under an expression such as `false && get(...)`, an assignment, an argument, a `begin` block, or parentheses is skipped. A callback in such a place makes the guards of the class unknown. An inflection call or an `api_only` assignment in such a place makes the inflections or the value unreadable. |
| The block of `inflections` | Each statement must be a call `param.method(literals)` of a method that changes the inflections, a comment, or a condition or a `case` that omen can evaluate with branches of the same kind. Any other statement, such as `binding`, `eval`, `send`, a definition, or a condition that omen cannot evaluate, makes the inflections unreadable. A stored result of `ActiveSupport::Inflector.inflections` makes them unreadable too. |
| A controller string with an empty segment, a leading `/` only for the absolute form, or a character other than `a` to `z`, `0` to `9`, and `_` | The route has no symbol and no guard. Rails can build a class from such a string: `users/#show` gives `Users::Controller`. Omen does not model this, so it gives less than Rails does. The path stays. |
| `ActiveSupport::Inflector.inflections` with a locale | Only the call with no argument, `:en`, or `"en"` counts, because the names of the controller classes use the default locale. A call with another literal locale is ignored. A call with a locale that is not a literal makes the inflections unreadable. |
| `case` with a literal subject and literal `when` patterns | Omen keeps the branch that runs. A route in any other `case` is skipped. |
| A repeated option, as `to: "a#b", to: "c#d"`, or a repeated hash-rocket key | The last value wins, as in Ruby. |
| `member do`, `collection do`, `on: :member`, `on: :collection` | Read. A canonical action name, such as `get :show`, adds no path segment. `on: :new` is skipped. |
| A route in a `scope` in a `resources` block | Read. The nested path follows the path of the scope: `/x/users/:user_id/y`. |
| `concerns`, `draw(:file)`, `mount`, `devise_for`, `direct`, `resolve` | No route. |

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
in a method, a lambda, a loop, a class body, and outside a `draw` block.

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
  kind. A call of a method that registers a route, makes a group, or serves the router
  is not. This rule holds only when the factory returns the router of the framework.
- A factory can return a literal that holds a router, as `&Server{Mux: router}`. The
  type of the literal can define a method that hides a method of the router. Thus each
  use of the result that is not serving is a use of the second kind.
- A handler name in a Go file has a package. A name with a qualifier, as `dep.Show`,
  names a function of the package of the import. The index must resolve that import.
  If it does not, the entry has no handler. A name without a qualifier names a function
  of the package of the file. A package is the files of one directory.
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
  `else` branch of `if true`, and the body of `for false`. A route under another
  condition or another loop keeps its entry.
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
  in any source order. `Mount` joins a router of the same file. A router that two mounts
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
  .HandlerFunc(show)` does. A chain without `Path` or `PathPrefix` gives no entry. The
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
