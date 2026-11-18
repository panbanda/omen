use super::super::tests::{assert_scales_linearly, entries, entries_lean};
use crate::core::Language;
use crate::index::security::entry::EntryPoint;

const WEBHOOKS: (&str, &str) = (
    "app/controllers/webhooks_controller.rb",
    "class WebhooksController\n  before_action :check\n  def aws; end\n  def show; end\n  def index; end\n  def update; end\nend\n",
);

fn rails(files: &[(&str, &str)]) -> Vec<EntryPoint> {
    entries(Language::Ruby, files)
        .into_iter()
        .filter(|entry| entry.framework == "rails")
        .collect()
}

/// One row for each entry: the method, the path, the handler without its file, and the guards.
fn rows_of(found: &[EntryPoint]) -> Vec<String> {
    let mut rows: Vec<String> = found
        .iter()
        .map(|entry| {
            let route = entry.route.as_ref().expect("a Rails entry has a route");
            let handler = entry
                .symbol
                .as_deref()
                .map_or("-", |symbol| symbol.rsplit('#').next().unwrap_or(symbol));
            format!(
                "{} {} {} [{}]",
                route.method,
                route.path_text(),
                handler,
                entry.guards.join(", ")
            )
        })
        .collect();
    rows.sort();
    rows
}

fn sorted(rows: &[&str]) -> Vec<String> {
    let mut rows: Vec<String> = rows.iter().map(|row| (*row).to_string()).collect();
    rows.sort();
    rows
}

fn rows(routes: &str, others: &[(&str, &str)]) -> Vec<String> {
    let mut files = vec![("config/routes.rb", routes)];
    files.extend_from_slice(others);
    rows_of(&rails(&files))
}

fn draw(body: &str) -> String {
    format!("Rails.application.routes.draw do\n{body}\nend\n")
}

#[test]
fn a_scope_with_a_nil_path_adds_no_prefix() {
    let routes = draw("  scope path: nil do\n    post \"webhooks/aws\" => \"webhooks#aws\"\n  end");
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        ["POST /webhooks/aws WebhooksController.aws [check]"]
    );
}

#[test]
fn a_scope_option_that_changes_no_path_and_no_controller_keeps_the_routes() {
    for options in [
        "path: nil, constraints: { format: %w[json html] }",
        "format: false, constraints: { topic_id: /\\d+/ }",
        "defaults: { format: :json }, as: :hooks",
        "format: :json",
        "format: \"json\"",
        "topic_id: /\\d+/",
        "host: \"a.example\"",
        "id: RouteFormat.username",
    ] {
        let routes = draw(&format!(
            "  scope {options} do\n    post \"webhooks/aws\" => \"webhooks#aws\"\n  end"
        ));
        assert_eq!(
            rows(&routes, &[WEBHOOKS]),
            ["POST /webhooks/aws WebhooksController.aws [check]"],
            "{options}"
        );
    }
}

#[test]
fn a_scope_option_that_omen_cannot_evaluate_skips_the_block() {
    for options in [
        "path: PREFIX",
        "format: FORMAT",
        "shallow: true",
        "shallow_path: \"x\"",
        "path_names: { new: \"make\" }",
        "via: :get",
        "action: :aws",
        "to: \"webhooks#aws\"",
        "only: :index",
        "on: :member",
        "**options",
    ] {
        let routes = draw(&format!(
            "  scope {options} do\n    post \"webhooks/aws\" => \"webhooks#aws\"\n  end"
        ));
        assert!(rows(&routes, &[WEBHOOKS]).is_empty(), "{options}");
    }
}

#[test]
fn a_true_format_makes_the_format_a_part_of_the_path() {
    let routes = draw(
        "  scope format: true do\n    get \"a\" => \"webhooks#show\"\n    get \"b\" => \"webhooks#show\", format: false\n  end\n  get \"c\" => \"webhooks#show\", format: true\n  resources :webhooks, only: :index, format: true",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /a.:format WebhooksController.show [check]",
            "GET /b WebhooksController.show [check]",
            "GET /c.:format WebhooksController.show [check]",
            "GET /webhooks.:format WebhooksController.index [check]",
        ])
    );
}

#[test]
fn a_route_format_that_is_a_literal_keeps_the_route() {
    let routes = draw(
        "  get \"a\" => \"webhooks#show\", :format => :json\n  get \"b\" => \"webhooks#show\", format: \"json\"\n  get \"c\" => \"webhooks#show\", format: /json|html/\n  get \"d\" => \"webhooks#show\", format: FORMAT\n  get \"e\" => \"webhooks#show\", constraints: { format: \"html\" }, defaults: { format: :html }",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /a WebhooksController.show [check]",
            "GET /b WebhooksController.show [check]",
            "GET /c WebhooksController.show [check]",
            "GET /e WebhooksController.show [check]",
        ])
    );
}

const ADMIN_USERS: (&str, &str) = (
    "app/controllers/admin/users_controller.rb",
    "class Admin::UsersController\n  before_action :ensure_admin\n  def index; end\n  def show; end\nend\n",
);

#[test]
fn a_constraint_object_of_a_namespace_is_a_guard_and_the_routes_stay() {
    let routes = draw(
        "  namespace :admin, constraints: StaffConstraint.new do\n    get \"users\" => \"users#index\"\n    resources :users, only: :show\n  end",
    );
    assert_eq!(
        rows(&routes, &[ADMIN_USERS]),
        sorted(&[
            "GET /admin/users Admin.UsersController.index [StaffConstraint.new, ensure_admin]",
            "GET /admin/users/:id Admin.UsersController.show [StaffConstraint.new, ensure_admin]",
        ])
    );
}

#[test]
fn a_constraint_object_of_a_scope_a_block_a_route_or_a_resource_is_a_guard() {
    let routes = draw(
        "  scope constraints: A.new do\n    constraints(B.new(x: 1)) do\n      get \"a\" => \"webhooks#show\", :constraints => C\n      resources :webhooks, only: :index, constraints: D.new\n    end\n  end\n  constraints ->(request) { request.local? } do\n    get \"b\" => \"webhooks#show\"\n  end\n  constraints lambda { |request| request.local? } do\n    get \"c\" => \"webhooks#show\"\n  end",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /a WebhooksController.show [C, check]",
            "GET /webhooks WebhooksController.index [A.new, B.new(x: 1), D.new, check]",
            "GET /b WebhooksController.show [lambda:8, check]",
            "GET /c WebhooksController.show [lambda:11, check]",
        ])
    );
}

#[test]
fn a_constraint_hash_is_not_a_guard_and_a_repeated_constraint_is_one_guard() {
    let routes = draw(
        "  scope constraints: { id: /\\d+/, subdomain: \"admin\" } do\n    constraints A.new do\n      constraints(id: /x/) do\n        get \"a\" => \"webhooks#show\", constraints: A.new\n      end\n    end\n  end",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        ["GET /a WebhooksController.show [A.new, check]"]
    );
}

#[test]
fn a_constraint_text_has_one_space_for_each_run_of_white_space() {
    let routes = draw(
        "  get \"a\" => \"webhooks#show\",\n      constraints: AdminConstraint.new(\n        require_master: true,\n      )",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        ["GET /a WebhooksController.show [AdminConstraint.new( require_master: true, ), check]"]
    );
}

const INFLECTOR_CACHE: &str = "module ActiveSupport\n  module Inflector\n    LRU_CACHES = []\n\n    def self.clear_memoize!\n      LRU_CACHES.each(&:clear)\n    end\n\n    def self.memoize(*args)\n      args.each do |method_name|\n        cache = Cache.new(200)\n        LRU_CACHES << cache\n\n        uncached = \"#{method_name}_without_cache\"\n        alias_method uncached, method_name\n\n        m =\n          define_method(method_name) do |*arguments|\n            found = true\n            data = cache.fetch(arguments) { found = false }\n            cache[arguments] = data = public_send(uncached, *arguments) unless found\n            data.dup\n          end\n\n        ruby2_keywords(m) if Module.respond_to?(:ruby2_keywords, true)\n      end\n    end\n\n    memoize :pluralize, :singularize, :camelize, :underscore, :classify\n  end\nend\n\nmodule ActiveSupport\n  module Inflector\n    class Inflections\n      def self.clear_memoize(*args)\n        args.each do |method_name|\n          orig = \"#{method_name}_without_clear_memoize\"\n          alias_method orig, method_name\n\n          define_method(method_name) do |*arguments|\n            ActiveSupport::Inflector.clear_memoize!\n            public_send(orig, *arguments)\n          end\n        end\n      end\n\n      clear_memoize :acronym, :plural, :singular, :irregular, :uncountable, :human, :clear\n    end\n  end\nend\n";

fn with_inflector(reopen: &str) -> Vec<String> {
    let routes = draw(
        "  post \"webhooks/aws\" => \"webhooks#aws\"\n  resources :webhooks, only: :index do\n    get \"x\" => \"webhooks#show\"\n  end",
    );
    rows(
        &routes,
        &[
            WEBHOOKS,
            ("lib/freedom_patches/inflector_backport.rb", reopen),
        ],
    )
}

#[test]
fn a_reopened_inflector_that_changes_no_rule_keeps_the_names() {
    let readable = sorted(&[
        "POST /webhooks/aws WebhooksController.aws [check]",
        "GET /webhooks WebhooksController.index [check]",
        "GET /webhooks/:webhook_id/x WebhooksController.show [check]",
    ]);
    for reopen in [
        "module ActiveSupport\n  module Inflector\n    X = 1\n  end\nend\n",
        "module ActiveSupport::Inflector\n  def self.helper(word)\n    word\n  end\nend\n",
        INFLECTOR_CACHE,
    ] {
        assert_eq!(with_inflector(reopen), readable, "{reopen}");
    }
}

#[test]
fn a_reopened_inflector_that_can_change_a_name_makes_the_names_unreadable() {
    let unreadable = sorted(&["POST /webhooks/aws - []", "GET /webhooks - []"]);
    for body in [
        "def camelize(term, uppercase_first_letter = true)\n  term\nend",
        "def self.singularize(word)\n  word\nend",
        "def pluralize(word, locale = :en)\n  word\nend",
        "def underscore(word)\n  word\nend",
        "def inflections(locale = :en)\n  yield Fake.new\nend",
        "class Inflections\n  def acronym(word)\n  end\nend",
        "class Inflections\n  def plural(rule, replacement)\n  end\nend",
        "define_method(:camelize) { |term| term }",
        "define_method(name) { |term| term }",
        "alias_method :camelize, :other",
        "alias camelize other",
        "remove_method :singularize",
        "undef_method :pluralize",
        "undef camelize",
        "class_eval(SOURCE)",
        "include Other",
        "prepend Other",
        "extend Other",
        "send(:define_method, :camelize) { |term| term }",
        "def self.wrap(name)\n  alias_method \"old\", name\n  define_method(name) { |*arguments| arguments.first }\nend\nwrap :camelize",
        "def self.wrap(name)\n  old = \"old_#{name}\"\n  define_method(name) { |*arguments| public_send(old, *arguments) }\n  alias_method old, name\nend\nwrap :camelize",
    ] {
        let reopen = format!("module ActiveSupport\n  module Inflector\n{body}\n  end\nend\n");
        assert_eq!(with_inflector(&reopen), unreadable, "{body}");
    }
}

#[test]
fn a_loop_over_a_literal_list_gives_the_routes_of_each_item() {
    let routes = draw(
        "  %w[users u].each do |root_path|\n    get \"#{root_path}\" => \"webhooks#index\"\n    get \"#{root_path}/:id\" => \"webhooks#show\"\n    resources :webhooks, only: :update, path: root_path\n  end\n  [:a, \"b\"].each { |name| get \"x/#{name}\", to: \"webhooks#show\" }\n  %i[show index].each do |action|\n    get action, controller: \"webhooks\"\n    get \"y/#{action}\", to: \"webhooks#show\", as: action\n  end",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /users WebhooksController.index [check]",
            "GET /users/:id WebhooksController.show [check]",
            "PUT /users/:id WebhooksController.update [check]",
            "PATCH /users/:id WebhooksController.update [check]",
            "GET /u WebhooksController.index [check]",
            "GET /u/:id WebhooksController.show [check]",
            "PUT /u/:id WebhooksController.update [check]",
            "PATCH /u/:id WebhooksController.update [check]",
            "GET /x/a WebhooksController.show [check]",
            "GET /x/b WebhooksController.show [check]",
            "GET /show WebhooksController.show [check]",
            "GET /y/show WebhooksController.show [check]",
            "GET /index WebhooksController.index [check]",
            "GET /y/index WebhooksController.show [check]",
        ])
    );
}

#[test]
fn a_loop_over_a_literal_hash_and_a_loop_with_an_index_bind_each_parameter() {
    let routes = draw(
        "  {\n    \"latest\" => \"webhooks#index\",\n    \"t/:id\" => \"webhooks#show\",\n    aws: \"webhooks#aws\",\n  }.each do |path, action|\n    get \"#{path}.:format\", to: action\n    get path, to: action, defaults: { format: :md }\n  end\n  %w[p q].each_with_index do |name, index|\n    get \"#{name}/#{index}\" => \"webhooks#show\"\n  end\n  { a: :b }.each_pair { |key, value| get \"#{key}/#{value}\" => \"webhooks#show\" }",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /latest.:format WebhooksController.index [check]",
            "GET /latest WebhooksController.index [check]",
            "GET /t/:id.:format WebhooksController.show [check]",
            "GET /t/:id WebhooksController.show [check]",
            "GET /aws.:format WebhooksController.aws [check]",
            "GET /aws WebhooksController.aws [check]",
            "GET /p/0 WebhooksController.show [check]",
            "GET /q/1 WebhooksController.show [check]",
            "GET /a/b WebhooksController.show [check]",
        ])
    );
}

#[test]
fn a_loop_that_omen_cannot_evaluate_gives_no_route() {
    for looped in [
        "FILTERS.each do |name|\n    get \"#{name}\" => \"webhooks#show\"\n  end",
        "Site.filters.each { |name| get \"#{name}\" => \"webhooks#show\" }",
        "[:a, NAME].each { |name| get \"#{name}\" => \"webhooks#show\" }",
        "[:a, [:b]].each { |name| get \"#{name}\" => \"webhooks#show\" }",
        "%W[a #{b}].each { |name| get \"#{name}\" => \"webhooks#show\" }",
        "{ a: NAME }.each { |key, value| get \"#{key}\" => \"webhooks#show\" }",
        "{ a: :b }.each { |pair| get \"#{pair}\" => \"webhooks#show\" }",
        "{ a: :b, **more }.each { |key, value| get \"#{key}\" => \"webhooks#show\" }",
        "%w[a b].each { |(name, other)| get \"#{name}\" => \"webhooks#show\" }",
        "%w[a b].each { |name, other| get \"#{name}\" => \"webhooks#show\" }",
        "%w[a b].each { |name = \"c\"| get \"#{name}\" => \"webhooks#show\" }",
        "%w[a b].each { get \"#{it}\" => \"webhooks#show\" }",
        "%w[a b].map { |name| get \"#{name}\" => \"webhooks#show\" }",
        "%w[a b].each_slice(1) { |name| get \"#{name}\" => \"webhooks#show\" }",
        "%w[a b].each(&block)",
        "%w[a b].each do |name|\n    name = \"c\"\n    get \"#{name}\" => \"webhooks#show\"\n  end",
        "%w[a b].each do |name|\n    get \"#{name}\" => \"webhooks#show\"\n    name += \"c\"\n  end",
        "%w[a b].each do |name|\n    get \"#{name.upcase}\" => \"webhooks#show\"\n  end",
        "%w[a b].each do |name|\n    get \"#{name}#{@x}\" => \"webhooks#show\"\n  end",
        "%w[a b].each do |name|\n    get \"#{other}\" => \"webhooks#show\"\n  end",
        "for name in %w[a b]\n    get \"#{name}\" => \"webhooks#show\"\n  end",
    ] {
        let routes = draw(&format!("  {looped}"));
        assert!(rows(&routes, &[WEBHOOKS]).is_empty(), "{looped}");
    }
}

#[test]
fn a_loop_in_a_loop_binds_each_name_and_the_inner_name_hides_the_outer_name() {
    let routes = draw(
        "  %w[a b].each do |outer|\n    scope outer do\n      %w[c d].each do |inner|\n        get \"#{outer}-#{inner}\" => \"webhooks#show\"\n      end\n      %w[e].each do |outer|\n        get outer => \"webhooks#show\"\n      end\n    end\n  end",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /a/a-c WebhooksController.show [check]",
            "GET /a/a-d WebhooksController.show [check]",
            "GET /a/e WebhooksController.show [check]",
            "GET /b/b-c WebhooksController.show [check]",
            "GET /b/b-d WebhooksController.show [check]",
            "GET /b/e WebhooksController.show [check]",
        ])
    );
}

#[test]
fn the_entries_of_a_loop_have_keys_that_differ() {
    let routes = draw(
        "  %w[a b].each do |name|\n    get \"#{name}\" => \"webhooks#show\"\n    get \"same\" => \"webhooks#show\"\n    resources :webhooks, only: :index, path: name\n  end",
    );
    let found = rails(&[("config/routes.rb", &routes), WEBHOOKS]);
    let keys: std::collections::BTreeSet<&str> =
        found.iter().map(|entry| entry.key.as_str()).collect();
    assert_eq!(found.len(), 6);
    assert_eq!(keys.len(), 6);
}

#[test]
fn loops_in_loops_under_the_limit_give_each_route() {
    let items: Vec<String> = (0..40).map(|number| format!("i{number}")).collect();
    let list = format!("%w[{}]", items.join(" "));
    let two = draw(&format!(
        "  {list}.each do |a|\n    {list}.each do |b|\n      get \"#{{a}}/#{{b}}\" => \"webhooks#show\"\n    end\n  end"
    ));
    assert_eq!(rows(&two, &[WEBHOOKS]).len(), 1_600);
}

/// A list of 4,096 literal items.
fn long_list(hash: bool) -> String {
    let items: Vec<String> = (0..4_096)
        .map(|number| {
            if hash {
                format!("\"i{number}\" => \"webhooks#show\"")
            } else {
                format!("i{number}")
            }
        })
        .collect();
    if hash {
        format!("{{ {} }}", items.join(", "))
    } else {
        format!("%w[{}]", items.join(" "))
    }
}

/// A loop of 4,096 items around `size` routes that use the loop parameter, and a route after the
/// loop.
fn a_long_loop_around(size: usize) -> String {
    let mut body = format!("  {}.each do |name|\n", long_list(false));
    for number in 0..size {
        body.push_str(&format!(
            "    get \"#{{name}}/r{number}\" => \"webhooks#show\"\n"
        ));
    }
    body.push_str("  end\n  get \"after\" => \"webhooks#index\"");
    draw(&body)
}

fn assert_unexpanded(found: &[EntryPoint], path: &str) {
    for entry in found {
        let route = entry.route.as_ref().expect("a Rails entry has a route");
        if route.path_text().contains('[') {
            assert!(!entry.guards_complete, "{}", route.path_text());
            assert_eq!(
                entry.guards_unknown_reason,
                Some(super::GuardsUnknownReason::UnexpandedLoop),
                "{}",
                route.path_text()
            );
        } else {
            assert_eq!(route.path_text(), path);
            assert_ne!(
                entry.guards_unknown_reason,
                Some(super::GuardsUnknownReason::UnexpandedLoop)
            );
        }
    }
}

#[test]
fn a_loop_over_the_expansion_limit_gives_each_route_one_time_with_an_unknown_segment() {
    let routes = a_long_loop_around(200);
    let found = rails(&[("config/routes.rb", &routes), WEBHOOKS]);
    let mut expected: Vec<String> = (0..200)
        .map(|number| format!("GET /[name]/r{number} WebhooksController.show [check]"))
        .collect();
    expected.push(String::from("GET /after WebhooksController.index [check]"));
    expected.sort();
    assert_eq!(rows_of(&found), expected);
    assert_unexpanded(&found, "/after");
}

#[test]
fn a_long_loop_in_a_short_loop_gives_the_routes_of_each_outer_item() {
    let mut body = format!(
        "  %w[a b].each do |outer|\n    scope outer do\n      {}.each do |name|\n",
        long_list(false)
    );
    for number in 0..200 {
        body.push_str(&format!(
            "        get \"#{{name}}/r{number}\" => \"webhooks#show\"\n"
        ));
    }
    body.push_str("      end\n    end\n  end");
    let found = rails(&[("config/routes.rb", &draw(&body)), WEBHOOKS]);
    let mut expected: Vec<String> = ["a", "b"]
        .iter()
        .flat_map(|outer| {
            (0..200).map(move |number| {
                format!("GET /{outer}/[name]/r{number} WebhooksController.show [check]")
            })
        })
        .collect();
    expected.sort();
    assert_eq!(rows_of(&found), expected);
    assert_unexpanded(&found, "");
}

#[test]
fn a_long_hash_loop_gives_each_route_with_an_unknown_target() {
    let mut body = format!("  {}.each do |path, action|\n", long_list(true));
    for number in 0..200 {
        body.push_str(&format!("    get \"#{{path}}/r{number}\", to: action\n"));
    }
    body.push_str("  end");
    let found = rails(&[("config/routes.rb", &draw(&body)), WEBHOOKS]);
    let mut expected: Vec<String> = (0..200)
        .map(|number| format!("GET /[path]/r{number} - []"))
        .collect();
    expected.sort();
    assert_eq!(rows_of(&found), expected);
    assert_unexpanded(&found, "");
}

#[test]
fn a_mount_in_a_loop_over_the_expansion_limit_is_not_read() {
    let mut body = format!(
        "  {}.each do |name|\n    mount Blog::Engine, at: name\n",
        long_list(false)
    );
    for number in 0..200 {
        body.push_str(&format!(
            "    get \"#{{name}}/r{number}\" => \"webhooks#show\"\n"
        ));
    }
    body.push_str("  end");
    let routes = draw(&body);
    let found = rails(&[
        ("config/routes.rb", &routes),
        WEBHOOKS,
        BLOG_ENGINE,
        BLOG_ROUTES,
    ]);
    assert_eq!(found.len(), 200);
    assert_unexpanded(&found, "");
}

#[test]
fn an_empty_definition_of_a_verb_in_the_draw_block_disables_the_verb() {
    let routes = "Rails.application.routes.draw do\n  patch \"before\" => \"webhooks#update\"\n  def patch(*)\n  end # Disable PATCH requests\n\n  patch \"after\" => \"webhooks#update\"\n  put \"after\" => \"webhooks#update\"\n  resources :webhooks, only: :update\n  resource :webhook, only: :update, controller: \"webhooks\"\n  scope \"x\" do\n    patch \"y\" => \"webhooks#update\"\n  end\n  match \"m\" => \"webhooks#update\", via: %i[patch put]\nend\n";
    assert_eq!(
        rows(routes, &[WEBHOOKS]),
        sorted(&[
            "PATCH /before WebhooksController.update [check]",
            "PUT /after WebhooksController.update [check]",
            "PUT /webhooks/:id WebhooksController.update [check]",
            "PUT /webhook WebhooksController.update [check]",
            "PATCH /m WebhooksController.update [check]",
            "PUT /m WebhooksController.update [check]",
        ])
    );
}

#[test]
fn a_definition_in_the_draw_block_that_omen_cannot_model_gives_no_route() {
    for definition in [
        "def patch(*)\n    super\n  end",
        "def get(path, **options)\n    match(path, **options, via: :post)\n  end",
        "def resources(*)\n  end",
        "def match(*)\n  end",
        "def scope(*)\n    yield\n  end",
        "def self.patch(*)\n  end",
        "if ENV[\"X\"]\n    def patch(*)\n    end\n  end",
        "scope \"x\" do\n    def patch(*)\n    end\n  end",
        "define_method(:patch) { |*| }",
        "alias patch get",
        "alias_method :patch, :get",
        "undef patch",
        "singleton_class.undef_method(:patch)",
    ] {
        let routes = format!(
            "Rails.application.routes.draw do\n  {definition}\n  get \"a\" => \"webhooks#show\"\nend\n"
        );
        assert!(rows(&routes, &[WEBHOOKS]).is_empty(), "{definition}");
    }
    let helper = "Rails.application.routes.draw do\n  def admin_routes\n    get \"hidden\" => \"webhooks#show\"\n  end\n  get \"a\" => \"webhooks#show\"\nend\n";
    assert_eq!(
        rows(helper, &[WEBHOOKS]),
        ["GET /a WebhooksController.show [check]"]
    );
}

#[test]
fn a_definition_in_a_branch_that_never_runs_changes_nothing() {
    for definition in [
        "if false\n    def patch(*)\n      super\n    end\n  end",
        "unless true\n    alias patch get\n  end",
        "if true\n  else\n    define_method(:get) { |*| }\n  end",
        "def patch(*)\n    super\n  end if nil",
        "case true\n  when false\n    undef get\n  end",
    ] {
        let routes = format!(
            "Rails.application.routes.draw do\n  {definition}\n  get \"a\" => \"webhooks#show\"\nend\n"
        );
        assert_eq!(
            rows(&routes, &[WEBHOOKS]),
            ["GET /a WebhooksController.show [check]"],
            "{definition}"
        );
    }
}

#[test]
fn a_mount_in_a_branch_that_never_runs_mounts_nothing() {
    for mount in [
        "if false\n  Rails.application.routes.append do\n    mount Blog::Engine, at: \"/blog\"\n  end\nend\n",
        "Rails.application.routes.draw do\n  unless true\n    mount Blog::Engine, at: \"/blog\"\n  end\nend\n",
    ] {
        assert!(
            engine_rows(&[("plugins/blog/plugin.rb", mount)]).is_empty(),
            "{mount}"
        );
    }
    let live = "if true\n  Rails.application.routes.append do\n    mount Blog::Engine, at: \"/blog\"\n  end\nend\n";
    assert_eq!(engine_rows(&[("plugins/blog/plugin.rb", live)]).len(), 2);
}

#[test]
fn a_draw_call_in_a_draw_block_gives_no_route() {
    let routes = draw(
        "  scope \"x\" do\n    Rails.application.routes.draw do\n      get \"a\" => \"webhooks#show\"\n    end\n  end",
    );
    assert!(rows(&routes, &[WEBHOOKS]).is_empty());
}

#[test]
fn a_redirect_route_is_an_entry_without_a_handler() {
    let routes = draw(
        "  get \"old\" => redirect(\"/new\")\n  get \"older\", to: redirect { |params, request| \"/new\" }, constraints: A.new\n  get \"app\" => RackApp\n  get \"lambda\", to: ->(env) { [200, {}, []] }",
    );
    let found = rails(&[("config/routes.rb", &routes), WEBHOOKS]);
    assert_eq!(
        rows_of(&found),
        sorted(&[
            "GET /old - []",
            "GET /older - [A.new]",
            "GET /app - []",
            "GET /lambda - []",
        ])
    );
}

const BLOG_ENGINE: (&str, &str) = (
    "plugins/blog/lib/blog/engine.rb",
    "module Blog\n  class Engine < ::Rails::Engine\n    engine_name \"blog\"\n    isolate_namespace Blog\n  end\nend\n",
);

const BLOG_POSTS: (&str, &str) = (
    "plugins/blog/app/controllers/blog/posts_controller.rb",
    "module Blog\n  class PostsController\n    before_action :ensure_blog\n    def index; end\n    def show; end\n  end\nend\n",
);

const BLOG_ROUTES: (&str, &str) = (
    "plugins/blog/config/routes.rb",
    "Blog::Engine.routes.draw do\n  get \"/posts\" => \"posts#index\"\n  resources :posts, only: :show\nend\n",
);

fn engine_rows(others: &[(&str, &str)]) -> Vec<String> {
    let mut files = vec![BLOG_ENGINE, BLOG_POSTS, BLOG_ROUTES];
    files.extend_from_slice(others);
    rows_of(&rails(&files))
}

#[test]
fn the_routes_of_an_engine_have_the_path_of_its_one_mount() {
    let expected = sorted(&[
        "GET /blog/posts Blog.PostsController.index [ensure_blog]",
        "GET /blog/posts/:id Blog.PostsController.show [ensure_blog]",
    ]);
    for mount in [
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/blog\"\nend\n",
        "Rails.application.routes.draw { mount ::Blog::Engine => \"blog\" }\n",
        "after_initialize do\n  Rails.application.routes.append { mount Blog::Engine, at: \"/blog\", as: \"blog\" }\nend\n",
        "Rails.application.routes.prepend do\n  mount Blog::Engine, at: \"blog/\"\nend\n",
        "module Blog\n  Rails.application.routes.append do\n    mount Engine, at: \"/blog\"\n  end\nend\n",
        "Rails.application.routes.draw do\n  scope \"/blog\" do\n    mount Blog::Engine, at: \"/\"\n  end\nend\n",
    ] {
        assert_eq!(engine_rows(&[("plugins/blog/plugin.rb", mount)]), expected, "{mount}");
    }
}

#[test]
fn the_constraints_around_a_mount_guard_the_routes_of_the_engine() {
    let mount = "Rails.application.routes.draw do\n  scope constraints: A.new do\n    mount Blog::Engine, at: \"/blog\", constraints: AdminConstraint.new\n  end\nend\n";
    assert_eq!(
        engine_rows(&[("plugins/blog/plugin.rb", mount)]),
        sorted(&[
            "GET /blog/posts Blog.PostsController.index [AdminConstraint.new, ensure_blog]",
            "GET /blog/posts/:id Blog.PostsController.show [AdminConstraint.new, ensure_blog]",
        ])
    );
    let scoped = "Rails.application.routes.draw do\n  scope constraints: A.new do\n    mount Blog::Engine, at: \"/blog\", constraints: { subdomain: \"x\" }\n  end\nend\n";
    assert_eq!(
        engine_rows(&[("plugins/blog/plugin.rb", scoped)]),
        sorted(&[
            "GET /blog/posts Blog.PostsController.index [A.new, ensure_blog]",
            "GET /blog/posts/:id Blog.PostsController.show [A.new, ensure_blog]",
        ])
    );
}

#[test]
fn a_route_constraint_object_in_an_engine_replaces_the_blocks_but_not_the_mount() {
    let mount = (
        "plugins/blog/plugin.rb",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/blog\", constraints: M.new\nend\n",
    );
    let routes = (
        "plugins/blog/config/routes.rb",
        "Blog::Engine.routes.draw do\n  scope constraints: S.new do\n    get \"/posts\" => \"posts#index\", constraints: R.new\n    resources :posts, only: :show\n  end\nend\n",
    );
    assert_eq!(
        rows_of(&rails(&[BLOG_ENGINE, BLOG_POSTS, routes, mount])),
        sorted(&[
            "GET /blog/posts Blog.PostsController.index [M.new, R.new, ensure_blog]",
            "GET /blog/posts/:id Blog.PostsController.show [M.new, S.new, ensure_blog]",
        ])
    );
}

#[test]
fn an_engine_without_one_readable_mount_gives_no_route() {
    assert!(engine_rows(&[]).is_empty(), "no mount");
    for mount in [
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/a\"\n  mount Blog::Engine, at: \"/b\"\nend\n",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: PREFIX\nend\n",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/a\", via: :get\nend\n",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/a\", **options\nend\n",
        "Rails.application.routes.draw do\n  mount Blog::Engine\nend\n",
        "Rails.application.routes.draw do\n  scope PREFIX do\n    mount Blog::Engine, at: \"/a\"\n  end\nend\n",
        "Rails.application.routes.draw do\n  PATHS.each do |path|\n    mount Blog::Engine, at: \"/a\"\n  end\nend\n",
        "Rails.application.routes.draw do\n  %w[a b].each do |path|\n    mount Blog::Engine, at: \"/#{path}\"\n  end\nend\n",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"/a(/:x)\"\nend\n",
    ] {
        assert!(engine_rows(&[("plugins/blog/plugin.rb", mount)]).is_empty(), "{mount}");
    }
}

#[test]
fn an_engine_that_does_not_isolate_a_namespace_has_no_module() {
    let engine = (
        "lib/shop/engine.rb",
        "module Shop\n  class Engine < Rails::Engine\n  end\nend\n",
    );
    let routes = (
        "lib/shop/config/routes.rb",
        "Shop::Engine.routes.draw do\n  get \"hooks\" => \"webhooks#show\"\nend\n",
    );
    let mount = (
        "config/routes.rb",
        "Rails.application.routes.draw do\n  mount Shop::Engine, at: \"shop\"\nend\n",
    );
    assert_eq!(
        rows_of(&rails(&[engine, routes, mount, WEBHOOKS])),
        ["GET /shop/hooks WebhooksController.show [check]"]
    );
}

#[test]
fn an_engine_namespace_that_omen_cannot_read_gives_no_route() {
    let mount = (
        "config/routes.rb",
        "Rails.application.routes.draw do\n  mount Blog::Engine, at: \"blog\"\nend\n",
    );
    for body in [
        "isolate_namespace NAMESPACE.fetch(:blog)",
        "isolate_namespace Missing",
        "isolate_namespace Blog\n    isolate_namespace Blog",
        "isolate_namespace Blog if ENV[\"X\"]",
        "routes.default_scope = { module: \"other\" }",
        "initializer \"x\" do\n      isolate_namespace Blog\n    end",
        "isolate_namespace Blog\n    endpoint CustomRackApp",
        "isolate_namespace Blog\n    endpoint(-> (env) { [200, {}, []] })",
        "isolate_namespace Blog\n    self.endpoint = CustomRackApp",
    ] {
        let engine =
            format!("module Blog\n  class Engine < ::Rails::Engine\n    {body}\n  end\nend\n");
        let found = rails(&[
            ("plugins/blog/lib/blog/engine.rb", &engine),
            BLOG_POSTS,
            BLOG_ROUTES,
            mount,
        ]);
        assert!(found.is_empty(), "{body}");
    }
}

#[test]
fn the_module_of_an_engine_follows_underscore_of_rails() {
    assert_eq!(super::underscore("DiscourseAi"), "discourse_ai");
    assert_eq!(super::underscore("Admin::HTMLParser"), "admin/html_parser");
    assert_eq!(super::underscore("A::B2C"), "a/b2_c");
    assert_eq!(super::underscore("Chat"), "chat");
    assert_eq!(super::underscore("already_lower"), "already_lower");
    assert_eq!(super::underscore("Dash-Name"), "dash_name");
}

fn sibling_loops_at(size: usize) -> std::time::Duration {
    let mut body = String::new();
    for number in 0..size {
        body.push_str(&format!(
            "  %w[a{number} b{number}].each do |name|\n    get \"#{{name}}\" => \"webhooks#show\"\n  end\n"
        ));
    }
    let routes = draw(&body);
    let (found, elapsed) = entries_lean(Language::Ruby, &[("config/routes.rb", &routes), WEBHOOKS]);
    assert_eq!(found.len(), size * 2);
    elapsed
}

/// Loops of one item in each other, with a route at each depth that uses the name of the first
/// loop.
fn nested_loops_at(depth: usize) -> std::time::Duration {
    let mut body = String::new();
    for number in 0..depth {
        body.push_str(&format!(
            "%w[v{number}].each do |name{number}|\nget \"#{{name0}}/r{number}\" => \"webhooks#show\"\n"
        ));
    }
    body.push_str(&"end\n".repeat(depth));
    let routes = draw(&body);
    let (found, elapsed) = entries_lean(Language::Ruby, &[("config/routes.rb", &routes), WEBHOOKS]);
    assert_eq!(found.len(), depth);
    elapsed
}

/// Scopes in each other, each with the same constraint and a route.
fn nested_constrained_scopes_at(depth: usize) -> std::time::Duration {
    let mut body = String::new();
    for number in 0..depth {
        body.push_str(&format!(
            "scope constraints: StaffConstraint.new do\nget \"r{number}\" => \"webhooks#show\", constraints: StaffConstraint.new\n"
        ));
    }
    body.push_str(&"end\n".repeat(depth));
    let routes = draw(&body);
    let (found, elapsed) = entries_lean(Language::Ruby, &[("config/routes.rb", &routes), WEBHOOKS]);
    assert_eq!(found.len(), depth);
    assert!(found
        .iter()
        .all(|entry| entry.guards == ["StaffConstraint.new", "check"]));
    elapsed
}

/// Engines with one mount each, in one file.
fn many_mounted_engines_at(size: usize) -> std::time::Duration {
    let mut engines = String::new();
    let mut mounts = String::from("Rails.application.routes.draw do\n");
    let mut files: Vec<(String, String)> = Vec::new();
    for number in 0..size {
        engines.push_str(&format!(
            "module E{number}\n  class Engine < ::Rails::Engine\n    isolate_namespace E{number}\n  end\n  class HooksController\n    def show; end\n  end\nend\n"
        ));
        mounts.push_str(&format!("  mount E{number}::Engine, at: \"/e{number}\"\n"));
        files.push((
            format!("engines/e{number}/config/routes.rb"),
            format!("E{number}::Engine.routes.draw do\n  get \"x\" => \"hooks#show\"\nend\n"),
        ));
    }
    mounts.push_str("end\n");
    let mut all: Vec<(&str, &str)> = vec![
        ("lib/engines.rb", engines.as_str()),
        ("config/routes.rb", mounts.as_str()),
    ];
    all.extend(
        files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    );
    let (found, elapsed) = entries_lean(Language::Ruby, &all);
    assert_eq!(found.len(), size);
    assert!(found.iter().all(|entry| entry.symbol.is_some()));
    elapsed
}

/// One loop of 4,096 items around `size` routes.
fn a_long_loop_at(size: usize) -> std::time::Duration {
    let routes = a_long_loop_around(size);
    let (found, elapsed) = entries_lean(Language::Ruby, &[("config/routes.rb", &routes), WEBHOOKS]);
    assert_eq!(found.len(), size + 1);
    elapsed
}

/// `size` loops of 4,096 items, each around one route. The first loops expand, until the work of
/// the expansion gets to the limit.
fn long_sibling_loops_at(size: usize) -> std::time::Duration {
    let list = long_list(false);
    let mut body = String::new();
    for number in 0..size {
        body.push_str(&format!(
            "  {list}.each do |name|\n    get \"#{{name}}/r{number}\" => \"webhooks#show\"\n  end\n"
        ));
    }
    let routes = draw(&body);
    let (found, elapsed) = entries_lean(Language::Ruby, &[("config/routes.rb", &routes), WEBHOOKS]);
    assert!(found.len() >= size);
    assert!(found.len() < size + 100 * 4_096);
    elapsed
}

#[test]
#[ignore = "timing: run in release"]
fn timing_a_long_loop_around_many_routes_resolves_in_linear_time() {
    assert_scales_linearly("long loop", 20_000, a_long_loop_at);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_long_loops_resolve_in_linear_time() {
    assert_scales_linearly("long sibling loops", 200, long_sibling_loops_at);
}

#[test]
fn the_linear_cases_give_each_route() {
    sibling_loops_at(20);
    nested_loops_at(20);
    nested_constrained_scopes_at(20);
    many_mounted_engines_at(10);
    a_long_loop_at(100);
    long_sibling_loops_at(20);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_sibling_loops_resolve_in_linear_time() {
    assert_scales_linearly("sibling loops", 4_000, sibling_loops_at);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_nested_loops_resolve_in_linear_time() {
    assert_scales_linearly("nested loops", 400, nested_loops_at);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_nested_constrained_scopes_resolve_in_linear_time() {
    assert_scales_linearly(
        "nested constrained scopes",
        400,
        nested_constrained_scopes_at,
    );
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_mounted_engines_resolve_in_linear_time() {
    assert_scales_linearly("mounted engines", 500, many_mounted_engines_at);
}

#[test]
fn a_string_path_with_two_parts_and_no_target_names_the_controller_and_the_action() {
    let routes = draw(
        "  get \"webhooks/show\"\n  put \"/web-hooks/do-it\"\n  namespace :admin do\n    get \"users/index\"\n  end\n  get \"webhooks/aws\", action: :show\n  get \"webhooks/:id\"\n  get :webhooks\n  get \"webhooks/show/\"",
    );
    let hyphen = (
        "app/controllers/web_hooks_controller.rb",
        "class WebHooksController\n  def do_it; end\nend\n",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS, ADMIN_USERS, hyphen]),
        sorted(&[
            "GET /webhooks/show WebhooksController.show [check]",
            "PUT /web-hooks/do-it WebHooksController.do_it []",
            "GET /admin/users/index Admin.UsersController.index [ensure_admin]",
            "GET /webhooks/aws - []",
            "GET /webhooks/:id - []",
            "GET /webhooks - []",
            "GET /webhooks/show - []",
        ])
    );
}

#[test]
fn a_resource_in_a_member_or_collection_block_has_the_path_of_the_block() {
    let routes = draw(
        "  resources :api, only: [:index], controller: \"webhooks\" do\n    collection do\n      resources :keys, controller: \"webhooks\", only: %i[index show] do\n        member { post \"revoke\" => \"webhooks#update\" }\n      end\n    end\n    member do\n      resources :parts, controller: \"webhooks\", only: :index\n    end\n  end",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS]),
        sorted(&[
            "GET /api WebhooksController.index [check]",
            "GET /api/keys WebhooksController.index [check]",
            "GET /api/keys/:id WebhooksController.show [check]",
            "POST /api/keys/:id/revoke WebhooksController.update [check]",
            "GET /api/:id/parts WebhooksController.index [check]",
        ])
    );
}

/// One row for each entry: the path, the guards, and `complete` or the reason.
fn guard_rows(routes: &str, others: &[(&str, &str)]) -> Vec<String> {
    let mut files = vec![("config/routes.rb", routes)];
    files.extend_from_slice(others);
    let mut rows: Vec<String> = rails(&files)
        .iter()
        .map(|entry| {
            assert_eq!(
                entry.guards_complete,
                entry.guards_unknown_reason.is_none(),
                "{entry:?}"
            );
            format!(
                "{} [{}] {}",
                entry.route.as_ref().expect("a route").path_text(),
                entry.guards.join(", "),
                entry
                    .guards_unknown_reason
                    .map_or("complete", |reason| reason.name())
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn a_route_without_a_controller_action_tells_why_its_guards_are_not_known() {
    let routes = draw(
        "  get \"old\" => redirect(\"/new\"), constraints: A.new\n  get \"app\" => RackApp\n  get \"lambda\", to: ->(env) { [200, {}, []] }\n  get \"missing\" => \"missing#show\"\n  get \"dynamic\", to: TARGET\n  get \"hidden\" => \"webhooks#secret\"\n  get \"shown\" => \"webhooks#show\"",
    );
    let controller = (
        "app/controllers/webhooks_controller.rb",
        "class WebhooksController\n  before_action :check\n  def show; end\n  private\n  def secret; end\nend\n",
    );
    assert_eq!(
        guard_rows(&routes, &[controller]),
        sorted(&[
            "/old [A.new] complete",
            "/app [] rack_endpoint",
            "/lambda [] rack_endpoint",
            "/missing [] unresolved_controller",
            "/dynamic [] rack_endpoint",
            "/hidden [] unreadable_action",
            "/shown [check] complete",
        ])
    );
}

fn class_rows(classes: &str) -> Vec<String> {
    let routes = draw("  get \"index\" => \"a#index\"\n  get \"show\" => \"a#show\"");
    guard_rows(&routes, &[("app/controllers/a.rb", classes)])
}

#[test]
fn a_callback_with_a_condition_makes_the_guards_of_its_actions_not_complete() {
    assert_eq!(
        class_rows("class AController\n  before_action :a\n  before_action :b, if: :ok?\n  def index; end\n  def show; end\nend\n"),
        ["/index [a] conditional_callback", "/show [a] conditional_callback"]
    );
    assert_eq!(
        class_rows("class AController\n  before_action :a\n  before_action :b, only: [:index], unless: :ok?\n  def index; end\n  def show; end\nend\n"),
        ["/index [a] conditional_callback", "/show [a] complete"]
    );
    assert_eq!(
        class_rows("class AController\n  before_action :a\n  skip_before_action :a, only: :show, if: :ok?\n  def index; end\n  def show; end\nend\n"),
        ["/index [a] complete", "/show [] conditional_callback"]
    );
    assert_eq!(
        class_rows("class AController\n  before_action :a\n  if ENV[\"X\"]\n    before_action :b\n  end\n  def index; end\n  def show; end\nend\n"),
        ["/index [a] conditional_callback", "/show [a] conditional_callback"]
    );
}

#[test]
fn a_callback_that_omen_cannot_read_keeps_the_guards_that_omen_knows() {
    for (statement, reason) in [
        ("before_action :b, only: ACTIONS", "unreadable_callback"),
        ("before_action NAME", "unreadable_callback"),
        ("before_action do\n    halt\n  end", "unreadable_callback"),
        ("before_action -> { halt }", "unreadable_callback"),
        (
            "before_action { |controller| controller.halt }",
            "unreadable_callback",
        ),
        (
            "skip_callback :process_action, :before, :a",
            "unreadable_callback",
        ),
        ("around_action :wrap", "around_callback"),
        ("prepend_around_action :wrap", "around_callback"),
        ("include Extra", "included_module"),
        ("extend Extra", "included_module"),
        ("prepend Extra", "included_module"),
        ("class_eval(SOURCE)", "unreadable_callback"),
    ] {
        let classes = format!(
            "class AController\n  before_action :a\n  {statement}\n  def index; end\n  def show; end\nend\n"
        );
        assert_eq!(
            class_rows(&classes),
            [
                format!("/index [a] {reason}"),
                format!("/show [a] {reason}")
            ],
            "{statement}"
        );
    }
    assert_eq!(
        class_rows("class AController\n  before_action :a\n  append_before_action :b\n  after_action :log\n  def index; end\n  def show; end\nend\n"),
        ["/index [a, b] complete", "/show [a, b] complete"]
    );
}

#[test]
fn a_superclass_that_omen_cannot_find_makes_the_guards_not_complete() {
    for (parent, expected) in [
        ("ActionController::Base", "[a] complete"),
        ("::ActionController::API", "[a] complete"),
        ("Missing", "[a] unresolved_superclass"),
        ("Gem::BaseController", "[a] unresolved_superclass"),
        ("BaseController", "[base, a] complete"),
        ("::BaseController", "[base, a] complete"),
        ("Admin::BaseController", "[admin, a] complete"),
        ("::Admin::BaseController", "[admin, a] complete"),
        ("Twice", "[a] unresolved_superclass"),
    ] {
        let classes = format!(
            "class BaseController < ActionController::Base\n  before_action :base\nend\nclass Admin::BaseController < ActionController::Base\n  before_action :admin\nend\nmodule One\n  class Twice\n  end\nend\nmodule Two\n  class Twice\n  end\nend\nclass AController < {parent}\n  before_action :a\n  def index; end\n  def show; end\nend\n"
        );
        assert_eq!(
            class_rows(&classes),
            [format!("/index {expected}"), format!("/show {expected}")],
            "{parent}"
        );
    }
}

#[test]
fn a_class_with_only_an_include_has_no_superclass() {
    assert_eq!(
        class_rows("module Extra\nend\nclass AController\n  include Extra\n  before_action :a\n  def index; end\n  def show; end\nend\n"),
        ["/index [a] included_module", "/show [a] included_module"]
    );
}

fn a_chain_of_conditional_callbacks_at(depth: usize) -> std::time::Duration {
    let mut routes = String::new();
    let mut classes = String::new();
    for number in 0..depth {
        routes.push_str(&format!("  get \"r{number}\" => \"c{number}#show\"\n"));
        let parent = if number == 0 {
            String::new()
        } else {
            format!(" < C{}Controller", number - 1)
        };
        classes.push_str(&format!(
            "class C{number}Controller{parent}\n  before_action :g{number}, only: [:a{number}], if: :ok?\n  def show; end\nend\n"
        ));
    }
    let routes = draw(&routes);
    let files = [
        ("config/routes.rb", routes.as_str()),
        ("app/controllers/chain.rb", classes.as_str()),
    ];
    let (found, elapsed) = entries_lean(Language::Ruby, &files);
    assert_eq!(found.len(), depth);
    assert!(found.iter().all(|entry| entry.guards_complete));
    elapsed
}

#[test]
fn a_chain_of_conditional_callbacks_for_other_actions_is_complete() {
    a_chain_of_conditional_callbacks_at(20);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_a_chain_of_conditional_callbacks_resolves_in_linear_time() {
    assert_scales_linearly(
        "conditional callbacks",
        2_000,
        a_chain_of_conditional_callbacks_at,
    );
}

const MACRO_BASE: &str = "class ApplicationController\n  before_action :base\n\n  def self.requires_login(arg = {})\n    @requires_login_arg = arg\n  end\n\n  def self.requires_plugin(plugin_name)\n    before_action do\n      raise PluginDisabled if disabled?(plugin_name)\n    end\n  end\n\n  def self.requires_admin\n    before_action :ensure_admin\n    before_action :ensure_fresh, only: :show\n  end\n\n  class << self\n    def requires_staff\n      before_action :ensure_staff\n    end\n  end\nend\n";

fn macro_rows(base: &str, body: &str) -> Vec<String> {
    let routes = draw("  get \"index\" => \"a#index\"\n  get \"show\" => \"a#show\"");
    let child = format!(
        "class AController < ApplicationController\n  {body}\n  def index; end\n  def show; end\nend\n"
    );
    guard_rows(
        &routes,
        &[
            ("app/controllers/application_controller.rb", base),
            ("app/controllers/a_controller.rb", &child),
        ],
    )
}

#[test]
fn a_class_method_of_the_chain_gives_the_callbacks_that_it_registers() {
    for (body, index, show) in [
        ("requires_login", "[base] complete", "[base] complete"),
        (
            "requires_login(key: 1)",
            "[base] complete",
            "[base] complete",
        ),
        (
            "requires_plugin \"chat\"",
            "[base, requires_plugin] complete",
            "[base, requires_plugin] complete",
        ),
        (
            "requires_admin",
            "[base, ensure_admin] complete",
            "[base, ensure_admin, ensure_fresh] complete",
        ),
        (
            "requires_staff",
            "[base, ensure_staff] complete",
            "[base, ensure_staff] complete",
        ),
        (
            "requires_admin if ENV[\"X\"]",
            "[base] conditional_callback",
            "[base] conditional_callback",
        ),
        (
            "before_action :own\n  requires_staff\n  skip_before_action :base, only: :show",
            "[base, own, ensure_staff] complete",
            "[own, ensure_staff] complete",
        ),
    ] {
        assert_eq!(
            macro_rows(MACRO_BASE, body),
            [format!("/index {index}"), format!("/show {show}")],
            "{body}"
        );
    }
}

#[test]
fn a_class_method_call_uses_the_definition_that_exists_at_the_call() {
    let routes = draw("  get \"index\" => \"a#index\"");
    let base =
        "class ApplicationController\n  def self.guarded\n    before_action :parent\n  end\nend\n";
    for (child, expected) in [
        (
            "class AController < ApplicationController\n  guarded\n  def self.guarded\n    before_action :child\n  end\n  def index; end\nend\n",
            "/index [parent] complete",
        ),
        (
            "class AController < ApplicationController\n  def self.guarded\n    before_action :child\n  end\n  guarded\n  def index; end\nend\n",
            "/index [child] complete",
        ),
    ] {
        assert_eq!(
            guard_rows(
                &routes,
                &[
                    ("app/controllers/application_controller.rb", base),
                    ("app/controllers/a_controller.rb", child),
                ]
            ),
            [expected],
            "{child}"
        );
    }
    let reopened = guard_rows(
        &routes,
        &[
            ("app/controllers/application_controller.rb", base),
            (
                "app/controllers/a_controller.rb",
                "class AController < ApplicationController\n  guarded\n  def index; end\nend\n",
            ),
            (
                "lib/a_patch.rb",
                "class AController\n  def self.guarded\n    before_action :child\n  end\nend\n",
            ),
        ],
    );
    assert_eq!(reopened, ["/index [] unresolved_macro"]);
}

#[test]
fn each_callback_block_is_a_separate_callback() {
    for (body, index, show) in [
        (
            "http_basic_authenticate_with name: \"a\", password: \"b\", only: :index\n  http_basic_authenticate_with name: \"c\", password: \"d\", only: :show",
            "[base, http_basic_authenticate_with] complete",
            "[base, http_basic_authenticate_with] complete",
        ),
        (
            "requires_plugin \"a\"\n  requires_plugin \"b\" if ENV[\"X\"]",
            "[base, requires_plugin] conditional_callback",
            "[base, requires_plugin] conditional_callback",
        ),
        (
            "http_basic_authenticate_with name: \"a\", password: \"b\"\n  skip_before_action :http_basic_authenticate_with",
            "[base, http_basic_authenticate_with] complete",
            "[base, http_basic_authenticate_with] complete",
        ),
    ] {
        assert_eq!(
            macro_rows(MACRO_BASE, body),
            [format!("/index {index}"), format!("/show {show}")],
            "{body}"
        );
    }
}

#[test]
fn a_callback_with_a_name_and_a_block_registers_both() {
    assert_eq!(
        macro_rows(MACRO_BASE, "before_action(:check) { check_region }"),
        [
            "/index [base, check] unreadable_callback",
            "/show [base, check] unreadable_callback"
        ]
    );
    let base = "class ApplicationController\n  before_action :base\n  def self.guarded\n    before_action(:check) { check_region }\n  end\nend\n";
    assert_eq!(
        macro_rows(base, "guarded"),
        [
            "/index [base, check, guarded] complete",
            "/show [base, check, guarded] complete"
        ]
    );
    assert_eq!(
        macro_rows(MACRO_BASE, "before_action :check"),
        [
            "/index [base, check] complete",
            "/show [base, check] complete"
        ]
    );
}

#[test]
fn a_call_with_the_receiver_self_is_a_call_of_the_class() {
    for (body, index, show) in [
        (
            "self.requires_admin",
            "[base, ensure_admin] complete",
            "[base, ensure_admin, ensure_fresh] complete",
        ),
        (
            "self.requires_staff()",
            "[base, ensure_staff] complete",
            "[base, ensure_staff] complete",
        ),
        (
            "self.before_action :own, only: :show",
            "[base] complete",
            "[base, own] complete",
        ),
        (
            "self.unknown_macro",
            "[base] unresolved_macro",
            "[base] unresolved_macro",
        ),
    ] {
        assert_eq!(
            macro_rows(MACRO_BASE, body),
            [format!("/index {index}"), format!("/show {show}")],
            "{body}"
        );
    }
}

#[test]
fn a_class_method_that_omen_cannot_resolve_makes_the_guards_not_complete() {
    for body in [
        "unknown_macro",
        "load_and_authorize_resource",
        "authorize_with :admin, only: :show",
        "configure do\n    limit 1\n  end",
    ] {
        assert_eq!(
            macro_rows(MACRO_BASE, body),
            [
                "/index [base] unresolved_macro",
                "/show [base] unresolved_macro"
            ],
            "{body}"
        );
    }
    for definition in [
        "def self.guarded(*names)\n    before_action(*names)\n  end",
        "def self.guarded(options = {})\n    before_action :checked, options\n  end",
        "def self.guarded(options = {})\n    before_action :checked, **options\n  end",
        "def self.guarded\n    super\n  end",
        "def self.guarded\n    other_macro\n  end",
        "def self.guarded\n    include Extra\n  end",
        "def self.guarded\n    before_action :one\n  end\n  def self.guarded\n    before_action :two\n  end",
        "def self.guarded\n    send(:before_action, :one)\n  end",
    ] {
        let base = format!("class ApplicationController\n  before_action :base\n  {definition}\nend\n");
        let rows = macro_rows(&base, "guarded");
        assert_eq!(rows.len(), 2, "{definition}");
        assert!(
            rows.iter().all(|row| row.ends_with(" unresolved_macro")
                || row.ends_with(" unreadable_callback")
                || row.ends_with(" included_module")),
            "{definition}: {rows:?}"
        );
    }
}

#[test]
fn a_class_method_of_ruby_or_of_rails_that_registers_no_callback_changes_nothing() {
    let body = "layout false\n  helper_method :current\n  rescue_from(Failure) { |error| render plain: error.message }\n  attr_reader :state\n  after_action :log\n  public\n  NAME = compute(1)\n  self.page_size = 10\n  Registry.add(self)\n  content_security_policy do |policy|\n    policy.default_src :self\n  end";
    assert_eq!(
        macro_rows(MACRO_BASE, body),
        ["/index [base] complete", "/show [base] complete"]
    );
}

#[test]
fn the_forgery_protection_calls_of_rails_are_callbacks() {
    for (body, index, show) in [
        (
            "protect_from_forgery with: :exception",
            "[base, verify_authenticity_token] complete",
            "[base, verify_authenticity_token] complete",
        ),
        (
            "protect_from_forgery\n  skip_forgery_protection only: :index",
            "[base] complete",
            "[base, verify_authenticity_token] complete",
        ),
        (
            "protect_from_forgery prepend: true, except: [:show]",
            "[verify_authenticity_token, base] complete",
            "[base] complete",
        ),
        (
            "http_basic_authenticate_with name: \"a\", password: \"b\", only: :show",
            "[base] complete",
            "[base, http_basic_authenticate_with] complete",
        ),
    ] {
        assert_eq!(
            macro_rows(MACRO_BASE, body),
            [format!("/index {index}"), format!("/show {show}")],
            "{body}"
        );
    }
}

#[test]
fn the_nearest_definition_of_a_class_method_decides() {
    let routes = draw("  get \"index\" => \"a#index\"");
    let classes = "class ApplicationController\n  def self.guarded\n    before_action :far\n  end\nend\nclass MiddleController < ApplicationController\n  def self.guarded\n    before_action :near\n  end\nend\nclass AController < MiddleController\n  guarded\n  def index; end\nend\n";
    assert_eq!(
        guard_rows(&routes, &[("app/controllers/all.rb", classes)]),
        ["/index [near] complete"]
    );
}

fn a_chain_of_macro_calls_at(depth: usize) -> std::time::Duration {
    let mut routes = String::new();
    let mut classes = String::from(
        "class C0Controller\n  def self.guarded\n    before_action :checked\n  end\n  def show; end\nend\n",
    );
    for number in 1..=depth {
        routes.push_str(&format!("  get \"r{number}\" => \"c{number}#show\"\n"));
        classes.push_str(&format!(
            "class C{number}Controller < C{}Controller\n  guarded\n  def self.own{number}\n  end\n  def show; end\nend\n",
            number - 1
        ));
    }
    let routes = draw(&routes);
    let files = [
        ("config/routes.rb", routes.as_str()),
        ("app/controllers/chain.rb", classes.as_str()),
    ];
    let (found, elapsed) = entries_lean(Language::Ruby, &files);
    assert_eq!(found.len(), depth);
    assert!(found
        .iter()
        .all(|entry| entry.guards == ["checked"] && entry.guards_complete));
    elapsed
}

#[test]
fn a_chain_of_classes_that_call_a_class_method_of_the_first_class_resolves() {
    a_chain_of_macro_calls_at(20);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_a_chain_of_macro_calls_resolves_in_linear_time() {
    assert_scales_linearly("macro calls", 1_500, a_chain_of_macro_calls_at);
}

#[test]
fn a_route_without_a_controller_in_a_module_has_the_module_as_its_controller() {
    let routes = draw(
        "  namespace :webhooks do\n    get \"show\"\n    get :index\n    get \"other\", action: :aws\n  end\n  scope module: \"admin\" do\n    scope module: \"users\" do\n      get \"deep\"\n    end\n  end\n  namespace :admin do\n    namespace :users do\n      get \"index\"\n    end\n  end\n  get \"alone\"",
    );
    assert_eq!(
        rows(&routes, &[WEBHOOKS, ADMIN_USERS]),
        sorted(&[
            "GET /webhooks/show WebhooksController.show [check]",
            "GET /webhooks/index WebhooksController.index [check]",
            "GET /webhooks/other WebhooksController.aws [check]",
            "GET /deep - [ensure_admin]",
            "GET /admin/users/index Admin.UsersController.index [ensure_admin]",
            "GET /alone - []",
        ])
    );
}

#[test]
fn a_proc_target_is_a_rack_endpoint() {
    let routes = draw("  get \"/favicon.ico\", to: proc { [200, {}, [\"\"]] } if Rails.env.test?");
    assert_eq!(guard_rows(&routes, &[]), ["/favicon.ico [] rack_endpoint"]);
}

/// An opening of the inflector that wraps `singularize` with the block `body`.
fn wrapped_singularize(body: &str) -> String {
    format!(
        "module ActiveSupport\n  module Inflector\n    def self.wrap(*names)\n      names.each do |method_name|\n        old = \"#{{method_name}}_without_wrap\"\n        alias_method old, method_name\n        define_method(method_name) do |*arguments|\n          {body}\n        end\n      end\n    end\n\n    wrap :singularize\n  end\nend\n"
    )
}

#[test]
fn a_wrapper_that_returns_the_result_of_the_old_method_keeps_the_names() {
    let readable = sorted(&[
        "POST /webhooks/aws WebhooksController.aws [check]",
        "GET /webhooks WebhooksController.index [check]",
        "GET /webhooks/:webhook_id/x WebhooksController.show [check]",
    ]);
    for body in [
        "public_send(old, *arguments)",
        "log(1)\n          public_send(old, *arguments)",
        "value = public_send(old, *arguments)\n          value",
        "cache[arguments] ||= public_send(old, *arguments)",
    ] {
        assert_eq!(
            with_inflector(&wrapped_singularize(body)),
            readable,
            "{body}"
        );
    }
}

#[test]
fn a_wrapper_that_can_change_the_result_of_the_old_method_makes_the_names_unreadable() {
    let unreadable = sorted(&["POST /webhooks/aws - []", "GET /webhooks - []"]);
    for body in [
        "public_send(old, *arguments).upcase",
        "value = public_send(old, *arguments)\n          value.upcase",
        "public_send(old, *arguments)\n          \"fixed\"",
        "public_send(old, arguments.first.upcase)",
        "return \"x\" if arguments.empty?\n          public_send(old, *arguments)",
        "value = public_send(old, *arguments)\n          value << \"s\"\n          value",
        "cache[arguments] ||= public_send(old, *arguments).upcase",
        "cache[arguments] += public_send(old, *arguments)",
    ] {
        assert_eq!(
            with_inflector(&wrapped_singularize(body)),
            unreadable,
            "{body}"
        );
    }
}

/// `depth` nested modules, and `classes` controllers in the inner module with a superclass that
/// only the top level has. A `relative` name makes omen search each module around each
/// controller before the top level, and an absolute name `::BaseController` does not.
fn deep_modules_at(depth: usize, classes: usize, relative: bool) -> std::time::Duration {
    let parent = if relative {
        "BaseController"
    } else {
        "::BaseController"
    };
    let mut source =
        String::from("class BaseController < ActionController::Base\n  before_action :base\nend\n");
    for level in 0..depth {
        source.push_str(&format!("module M{level}\n"));
    }
    for number in 0..classes {
        source.push_str(&format!(
            "class C{number}Controller < {parent}\n  def show; end\nend\n"
        ));
    }
    for _ in 0..depth {
        source.push_str("end\n");
    }
    let namespace: Vec<String> = (0..depth).map(|level| format!("m{level}")).collect();
    let routes = format!(
        "Rails.application.routes.draw do\n  get \"/a\", to: \"{}/c0#show\"\nend\n",
        namespace.join("/")
    );
    let (found, elapsed) = entries_lean(
        Language::Ruby,
        &[
            ("config/routes.rb", routes.as_str()),
            ("app/controllers/deep.rb", source.as_str()),
        ],
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].guards, ["base"]);
    assert!(found[0].guards_complete);
    elapsed
}

#[test]
fn a_class_in_deep_modules_finds_the_superclass_of_the_top_level() {
    deep_modules_at(30, 30, true);
    deep_modules_at(30, 30, false);
}

/// The identities of the classes have the text of each module around them, so the time of this
/// input grows with the depth times the number of classes for an absolute name too. The search
/// of each module around a class must not add a factor of the depth to that: a relative name
/// must take about the time of an absolute name. The runs of the two names alternate, so a
/// change in the speed of the machine changes both.
#[test]
#[ignore = "timing: run in release"]
fn timing_a_relative_superclass_in_deep_modules_takes_the_time_of_an_absolute_one() {
    let size = 1_500;
    let mut absolute = std::time::Duration::MAX;
    let mut relative = std::time::Duration::MAX;
    for _ in 0..3 {
        absolute = absolute.min(deep_modules_at(size, size, false));
        relative = relative.min(deep_modules_at(size, size, true));
    }
    let allowed = absolute.max(std::time::Duration::from_millis(100)) * 2;
    assert!(
        relative < allowed,
        "absolute {absolute:?}, relative {relative:?} at {size}"
    );
}
