//! Tests for the findings of the review of the route entry points.

use super::tests::{assert_scales_linearly, entries, entries_timed};
use super::{EntryKind, EntryPoint, GuardsUnknownReason, MAX_GUARDS};
use crate::core::Language;
use std::fmt::Write;

fn row(entry: &EntryPoint) -> String {
    let (method, path) = entry.route.as_ref().map_or(("-", "-"), |route| {
        (route.method.as_str(), route.path_text())
    });
    let handler = entry.symbol.as_deref().map_or("-", |symbol| {
        symbol.rsplit(['#', '.']).next().unwrap_or(symbol)
    });
    format!("{} {method} {path} {handler}", entry.framework)
}

fn http_rows(language: Language, files: &[(&str, &str)]) -> Vec<String> {
    let mut rows: Vec<String> = entries(language, files)
        .iter()
        .filter(|entry| entry.kind == EntryKind::HttpHandler)
        .map(row)
        .collect();
    rows.sort();
    rows
}

fn go(imports: &str, body: &str) -> String {
    format!(
        "package p\n\nimport (\n\t\"net/http\"\n{imports}\n)\n\nfunc auth(next http.Handler) http.Handler {{ return next }}\nfunc first(w http.ResponseWriter, r *http.Request) {{}}\nfunc second(w http.ResponseWriter, r *http.Request) {{}}\nfunc third(w http.ResponseWriter, r *http.Request) {{}}\n\n{body}\n"
    )
}

const CHI: &str = "\t\"github.com/go-chi/chi/v5\"";
const MUX: &str = "\t\"github.com/gorilla/mux\"";

fn go_rows(imports: &str, body: &str) -> Vec<String> {
    let source = go(imports, body);
    http_rows(Language::Go, &[("main.go", &source)])
}

fn only_go_entry(imports: &str, body: &str) -> EntryPoint {
    let source = go(imports, body);
    let mut found = entries(Language::Go, &[("main.go", &source)]);
    assert_eq!(
        found.len(),
        1,
        "{:?}",
        found.iter().map(row).collect::<Vec<_>>()
    );
    found.remove(0)
}

fn rails_rows(routes: &str, controllers: &str) -> Vec<String> {
    http_rows(
        Language::Ruby,
        &[
            ("config/routes.rb", routes),
            ("app/controllers/controllers.rb", controllers),
        ],
    )
}

fn draw(body: &str) -> String {
    format!("Rails.application.routes.draw do\n{body}\nend\n")
}

fn only_rails_entry(body: &str) -> EntryPoint {
    let routes = draw(body);
    let mut found = entries(Language::Ruby, &[("config/routes.rb", &routes)]);
    assert_eq!(
        found.len(),
        1,
        "{:?}",
        found.iter().map(row).collect::<Vec<_>>()
    );
    found.remove(0)
}

// Go: conditional middleware.

#[test]
fn a_chi_layer_in_a_condition_does_not_make_a_complete_list() {
    let entry = only_go_entry(
        CHI,
        "func routes(on bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tr.Use(auth)\n\t}\n\tr.Get(\"/x\", first)\n}",
    );
    assert_eq!(entry.guards, ["auth"]);
    assert!(!entry.guards_complete);
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::ConditionalMiddleware)
    );
}

#[test]
fn a_chi_layer_in_a_loop_does_not_make_a_complete_list() {
    let entry = only_go_entry(
        CHI,
        "func routes(all []int) {\n\tr := chi.NewRouter()\n\tfor range all {\n\t\tr.Use(auth)\n\t}\n\tr.Get(\"/x\", first)\n}",
    );
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::ConditionalMiddleware)
    );
}

#[test]
fn a_chi_layer_in_a_case_does_not_make_a_complete_list() {
    let entry = only_go_entry(
        CHI,
        "func routes(n int) {\n\tr := chi.NewRouter()\n\tswitch n {\n\tcase 1:\n\t\tr.Use(auth)\n\t}\n\tr.Get(\"/x\", first)\n}",
    );
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::ConditionalMiddleware)
    );
}

#[test]
fn a_conditional_layer_of_a_parent_reaches_the_routes_of_its_group() {
    let entry = only_go_entry(
        CHI,
        "func routes(on bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tr.Use(auth)\n\t}\n\tr.Route(\"/a\", func(s chi.Router) {\n\t\ts.Get(\"/x\", first)\n\t})\n}",
    );
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::ConditionalMiddleware)
    );
}

#[test]
fn a_layer_of_a_router_made_in_the_same_branch_is_not_conditional() {
    let entry = only_go_entry(
        CHI,
        "func routes(on bool) {\n\tif on {\n\t\tr := chi.NewRouter()\n\t\tr.Use(auth)\n\t\tr.Get(\"/x\", first)\n\t}\n}",
    );
    assert_eq!(entry.guards, ["auth"]);
    assert!(entry.guards_complete);
}

#[test]
fn a_layer_in_the_callback_of_a_group_is_not_conditional() {
    let entry = only_go_entry(
        CHI,
        "func routes() {\n\tr := chi.NewRouter()\n\tr.Route(\"/a\", func(s chi.Router) {\n\t\ts.Use(auth)\n\t\ts.Get(\"/x\", first)\n\t})\n}",
    );
    assert_eq!(entry.guards, ["auth"]);
    assert!(entry.guards_complete);
}

#[test]
fn a_layer_in_a_branch_that_always_runs_is_not_conditional() {
    let entry = only_go_entry(
        CHI,
        "func routes() {\n\tr := chi.NewRouter()\n\tif true {\n\t\tr.Use(auth)\n\t}\n\tr.Get(\"/x\", first)\n}",
    );
    assert_eq!(entry.guards, ["auth"]);
    assert!(entry.guards_complete);
}

// Go: registrations in branches.

#[test]
fn registrations_in_exclusive_branches_both_stay() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tr.Get(\"/x\", first)\n\t} else {\n\t\tr.Get(\"/x\", second)\n\t}\n}"
        ),
        ["chi GET /x first", "chi GET /x second"]
    );
}

#[test]
fn a_registration_in_a_branch_does_not_replace_an_earlier_route() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on bool) {\n\tr := chi.NewRouter()\n\tr.Get(\"/x\", first)\n\tif on {\n\t\tr.Get(\"/x\", second)\n\t}\n}"
        ),
        ["chi GET /x first", "chi GET /x second"]
    );
}

#[test]
fn a_later_registration_outside_a_branch_replaces_the_route_in_the_branch() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tr.Get(\"/x\", first)\n\t}\n\tr.Get(\"/x\", second)\n}"
        ),
        ["chi GET /x second"]
    );
}

#[test]
fn two_registrations_in_one_branch_replace_each_other() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tr.Get(\"/x\", first)\n\t\tr.Get(\"/x\", second)\n\t}\n}"
        ),
        ["chi GET /x second"]
    );
}

#[test]
fn echo_registrations_in_exclusive_branches_both_stay() {
    assert_eq!(
        go_rows(
            "\t\"github.com/labstack/echo/v4\"",
            "func h1(c echo.Context) error { return nil }\nfunc h2(c echo.Context) error { return nil }\nfunc routes(on bool) {\n\te := echo.New()\n\tif on {\n\t\te.GET(\"/x\", h1)\n\t} else {\n\t\te.GET(\"/x\", h2)\n\t}\n}"
        ),
        ["echo GET /x h1", "echo GET /x h2"]
    );
}

// Go: `net/http` catch-all patterns.

#[test]
fn a_catch_all_with_only_empty_and_one_segment_siblings_stays() {
    assert_eq!(
        go_rows(
            "",
            "func routes() {\n\tm := http.NewServeMux()\n\tm.HandleFunc(\"/x/{rest...}\", first)\n\tm.HandleFunc(\"/x/{$}\", second)\n\tm.HandleFunc(\"/x/{id}\", third)\n}"
        ),
        [
            "net/http ANY /x/{$} second",
            "net/http ANY /x/{id} third",
            "net/http ANY /x/{rest...} first",
        ]
    );
}

#[test]
fn a_catch_all_with_a_sibling_for_each_depth_gives_no_entry() {
    assert_eq!(
        go_rows(
            "",
            "func routes() {\n\tm := http.NewServeMux()\n\tm.HandleFunc(\"/x/{rest...}\", first)\n\tm.HandleFunc(\"/x/{$}\", second)\n\tm.HandleFunc(\"/x/{id}\", third)\n\tm.HandleFunc(\"/x/{id}/{tail...}\", third)\n}"
        ),
        [
            "net/http ANY /x/{$} second",
            "net/http ANY /x/{id} third",
            "net/http ANY /x/{id}/{tail...} third",
        ]
    );
}

// Go: gorilla/mux.

#[test]
fn an_empty_methods_matcher_does_not_hide_a_later_route() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes() {\n\tr := mux.NewRouter()\n\tr.HandleFunc(\"/dead\", first).Methods()\n\tr.HandleFunc(\"/live\", second)\n}"
        ),
        ["gorilla/mux ANY /live second"]
    );
}

// Rails.

#[test]
fn an_inherited_public_action_has_the_handler_of_its_class() {
    let routes = draw("  get \"/x\", to: \"users#show\"");
    assert_eq!(
        rails_rows(
            &routes,
            "class ParentController\n  def show; end\nend\nclass UsersController < ParentController\nend\n"
        ),
        ["rails GET /x show"]
    );
}

#[test]
fn an_action_that_a_subclass_makes_public_has_the_handler_of_its_class() {
    let routes = draw("  get \"/x\", to: \"users#show\"");
    assert_eq!(
        rails_rows(
            &routes,
            "class ParentController\n  private\n  def show; end\nend\nclass UsersController < ParentController\n  public :show\nend\n"
        ),
        ["rails GET /x show"]
    );
}

#[test]
fn an_inherited_private_action_stays_hidden() {
    let routes = draw("  get \"/x\", to: \"users#show\"");
    assert_eq!(
        rails_rows(
            &routes,
            "class ParentController\n  private\n  def show; end\nend\nclass UsersController < ParentController\nend\n"
        ),
        ["rails GET /x -"]
    );
}

#[test]
fn the_nearest_definition_of_an_inherited_action_wins() {
    let routes = draw("  get \"/x\", to: \"c#show\"");
    let controllers = "class AController\n  def show; end\nend\nclass BController < AController\n  def show; end\nend\nclass CController < BController\nend\n";
    let found = entries(
        Language::Ruby,
        &[
            ("config/routes.rb", &routes),
            ("app/controllers/controllers.rb", controllers),
        ],
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].symbol.as_deref(),
        Some("app/controllers/controllers.rb#BController.show")
    );
}

#[test]
fn a_duplicate_key_of_a_literal_hash_keeps_the_last_value() {
    let routes = draw(
        "  { x: \"a#one\", y: \"a#three\", x: \"a#two\" }.each do |path, target|\n    get path, to: target\n  end",
    );
    let controllers = "class AController\n  def one; end\n  def two; end\n  def three; end\nend\n";
    assert_eq!(
        rails_rows(&routes, controllers),
        ["rails GET /x two", "rails GET /y three"]
    );
}

#[test]
fn a_string_key_and_a_symbol_key_of_a_hash_are_different_keys() {
    let routes = draw(
        "  { x: \"a#one\", \"x\" => \"a#two\" }.each do |path, target|\n    get path, to: target\n  end",
    );
    let controllers = "class AController\n  def one; end\n  def two; end\nend\n";
    assert_eq!(
        rails_rows(&routes, controllers),
        ["rails GET /x one", "rails GET /x two"]
    );
}

/// `Mapper#map_match` takes the first key that is a string as the path.
#[test]
fn only_the_first_hash_rocket_path_of_a_route_registers() {
    let routes = draw("  get \"/a\" => \"c#a\", \"/b\" => \"c#b\"");
    assert_eq!(
        rails_rows(
            &routes,
            "class CController\n  def a; end\n  def b; end\nend\n"
        ),
        ["rails GET /a a"]
    );
}

#[test]
fn a_local_with_a_literal_hash_is_a_request_constraint_and_not_a_guard() {
    let entry = only_rails_entry(
        "  rules = { host: \"example.test\" }\n  get \"/x\", to: redirect(\"/y\"), constraints: rules",
    );
    assert!(entry.guards.is_empty(), "{:?}", entry.guards);
    assert!(entry.guards_complete);
}

#[test]
fn a_local_with_a_constraint_object_stays_a_guard() {
    let entry = only_rails_entry(
        "  admin = AdminConstraint.new\n  get \"/x\", to: redirect(\"/y\"), constraints: admin",
    );
    assert_eq!(entry.guards, ["admin"]);
    assert!(entry.guards_complete);
}

#[test]
fn a_constant_constraint_stays_a_guard() {
    let entry = only_rails_entry("  get \"/x\", to: redirect(\"/y\"), constraints: AdminOnly");
    assert_eq!(entry.guards, ["AdminOnly"]);
    assert!(entry.guards_complete);
}

#[test]
fn a_local_with_an_unknown_value_is_not_a_named_guard() {
    let entry = only_rails_entry(
        "  rules = pick_rules\n  rules = other_rules\n  get \"/x\", to: redirect(\"/y\"), constraints: rules",
    );
    assert!(!entry.guards.iter().any(|guard| guard == "rules"));
    assert!(!entry.guards_complete);
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::UnreadMiddleware)
    );
}

#[test]
fn a_name_with_no_assignment_is_not_a_named_guard() {
    let entry = only_rails_entry("  get \"/x\", to: redirect(\"/y\"), constraints: pick_rules");
    assert!(!entry.guards.iter().any(|guard| guard == "pick_rules"));
    assert!(!entry.guards_complete);
}

#[test]
fn a_block_constraint_with_a_hash_local_adds_no_guard() {
    let entry = only_rails_entry(
        "  rules = { host: \"example.test\" }\n  constraints rules do\n    get \"/x\", to: redirect(\"/y\")\n  end",
    );
    assert!(entry.guards.is_empty(), "{:?}", entry.guards);
    assert!(entry.guards_complete);
}

// Proof of the framework.

#[test]
fn a_next_verb_function_must_be_exported() {
    assert!(http_rows(
        Language::TypeScript,
        &[("app/x/route.ts", "function GET() {}\n")]
    )
    .is_empty());
    assert_eq!(
        http_rows(
            Language::TypeScript,
            &[("app/x/route.ts", "export async function GET() {}\n")]
        ),
        ["next GET /x GET"]
    );
    assert_eq!(
        http_rows(
            Language::TypeScript,
            &[("app/x/route.ts", "function POST() {}\nexport { POST };\n")]
        ),
        ["next POST /x POST"]
    );
}

#[test]
fn a_csharp_map_call_on_another_type_is_not_a_route() {
    let source = "class Fake {\n    public void MapGet(string path, System.Action handler) {}\n}\nclass C {\n    void H() {}\n    void F(Fake fake) {\n        fake.MapGet(\"/x\", H);\n    }\n}\n";
    assert!(http_rows(Language::CSharp, &[("a.cs", source)]).is_empty());
}

#[test]
fn a_csharp_map_call_on_a_web_application_is_a_route() {
    let top_level = "var builder = WebApplication.CreateBuilder(args);\nvar app = builder.Build();\napp.MapGet(\"/x\", () => 1);\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Program.cs", top_level)]),
        ["aspnet GET /x -"]
    );
    let extension = "static class Api {\n    public static void Map(this IEndpointRouteBuilder routes) {\n        routes.MapPost(\"/y\", () => 1);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", extension)]),
        ["aspnet POST /y -"]
    );
    let group = "static class Api {\n    public static void Map(this IEndpointRouteBuilder routes) {\n        var api = routes.MapGroup(\"/api\");\n        api.MapGet(\"/z\", () => 1);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", group)]),
        ["aspnet GET /z -"]
    );
}

#[test]
fn a_csharp_map_call_in_a_file_that_uses_asp_net_is_a_route() {
    let source = "using Microsoft.AspNetCore.Builder;\nclass Startup {\n    void Configure(object endpoints) {\n        endpoints.MapGet(\"/x\", () => 1);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Startup.cs", source)]),
        ["aspnet GET /x -"]
    );
}

#[test]
fn a_rust_verb_attribute_of_another_crate_is_not_a_route() {
    let source = "use my_macros::get;\n\n#[get(\"/x\")]\nfn handler() {}\n";
    assert!(http_rows(Language::Rust, &[("a.rs", source)]).is_empty());
    let unbound = "#[get(\"/x\")]\nfn handler() {}\n";
    assert!(http_rows(Language::Rust, &[("a.rs", unbound)]).is_empty());
}

#[test]
fn a_rust_verb_attribute_needs_the_import_of_its_framework() {
    let actix = "use actix_web::{get, HttpResponse};\n\n#[get(\"/x\")]\nasync fn handler() -> HttpResponse { todo!() }\n";
    assert_eq!(
        http_rows(Language::Rust, &[("a.rs", actix)]),
        ["actix GET /x handler"]
    );
    let wildcard = "use actix_web::*;\n\n#[get(\"/x\")]\nasync fn handler() {}\n";
    assert_eq!(
        http_rows(Language::Rust, &[("a.rs", wildcard)]),
        ["actix GET /x handler"]
    );
    let rocket = "use rocket::get;\n\n#[get(\"/x\")]\nfn handler() {}\n";
    assert_eq!(
        http_rows(Language::Rust, &[("a.rs", rocket)]),
        ["rocket GET /x handler"]
    );
}

#[test]
fn a_rocket_macro_import_of_the_crate_root_proves_a_verb_attribute() {
    let root = "#[macro_use]\nextern crate rocket;\n\nmod routes;\n";
    let module = "#[get(\"/x\")]\nfn handler() {}\n";
    assert_eq!(
        http_rows(
            Language::Rust,
            &[("src/main.rs", root), ("src/routes.rs", module)]
        ),
        ["rocket GET /x handler"]
    );
}

#[test]
fn a_local_python_path_function_is_not_a_django_route() {
    let source =
        "import django\n\ndef view():\n    pass\n\ndef path(pattern, handler):\n    pass\n\npath(\"/x\", view)\n";
    assert!(http_rows(Language::Python, &[("a.py", source)]).is_empty());
}

#[test]
fn a_django_path_needs_the_import_of_the_name() {
    let urls = "from django.urls import path\n\ndef view(request):\n    pass\n\nurlpatterns = [path(\"x/\", view)]\n";
    assert_eq!(
        http_rows(Language::Python, &[("urls.py", urls)]),
        ["django ANY /x/ view"]
    );
    let old = "from django.conf.urls import url\n\ndef view(request):\n    pass\n\nurlpatterns = [url(r\"^x/$\", view)]\n";
    assert_eq!(http_rows(Language::Python, &[("urls.py", old)]).len(), 1);
    let wildcard = "from django.urls import *\n\ndef view(request):\n    pass\n\nurlpatterns = [path(\"x/\", view)]\n";
    assert_eq!(
        http_rows(Language::Python, &[("urls.py", wildcard)]).len(),
        1
    );
}

#[test]
fn a_java_annotation_of_the_repository_is_not_a_spring_route() {
    let source = "@interface GetMapping {\n    String value();\n}\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert!(http_rows(Language::Java, &[("C.java", source)]).is_empty());
}

#[test]
fn a_spring_route_needs_the_import_of_the_annotation() {
    let named = "import org.springframework.web.bind.annotation.GetMapping;\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::Java, &[("C.java", named)]),
        ["spring GET /x handler"]
    );
    let wildcard = "import org.springframework.web.bind.annotation.*;\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::Java, &[("C.java", wildcard)]),
        ["spring GET /x handler"]
    );
}

// The limit of the guards of one entry.

fn csharp_with_guards(guards: usize, actions: usize) -> String {
    let mut source = String::from("using Microsoft.AspNetCore.Mvc;\n[");
    for place in 0..guards {
        source.push_str(if place == 0 {
            "Authorize"
        } else {
            ", Authorize"
        });
    }
    source.push_str("]\nclass CController {\n");
    for action in 0..actions {
        writeln!(
            source,
            "    [HttpGet(\"/r{action}\")] void A{action}() {{}}"
        )
        .unwrap();
    }
    source.push_str("}\n");
    source
}

fn chi_with_guards(guards: usize, routes: usize) -> String {
    let mut body = String::from("func routes() {\n\tr := chi.NewRouter()\n");
    for _ in 0..guards {
        body.push_str("\tr.Use(auth)\n");
    }
    for route in 0..routes {
        writeln!(body, "\tr.Get(\"/r{route}\", first)").unwrap();
    }
    body.push('}');
    go(CHI, &body)
}

fn rails_action_with_guards(guards: usize, routes: usize) -> (String, String) {
    let mut body = String::new();
    for route in 0..routes {
        writeln!(body, "  get \"/r{route}\", to: \"c#show\"").unwrap();
    }
    let mut controller = String::from("class CController\n");
    for guard in 0..guards {
        writeln!(controller, "  before_action :g{guard}").unwrap();
    }
    controller.push_str("  def show; end\nend\n");
    (draw(&body), controller)
}

fn rails_many_actions(guards: usize, actions: usize) -> (String, String) {
    let mut body = String::new();
    let mut methods = String::new();
    for action in 0..actions {
        writeln!(body, "  get \"/r{action}\", to: \"c#a{action}\"").unwrap();
        writeln!(methods, "  def a{action}; end").unwrap();
    }
    let mut controller = String::from("class CController\n");
    for guard in 0..guards {
        writeln!(controller, "  before_action :g{guard}").unwrap();
    }
    writeln!(controller, "  skip_before_action :g0, only: [:a0]").unwrap();
    controller.push_str(&methods);
    controller.push_str("end\n");
    (draw(&body), controller)
}

fn rails_nested_constraints(depth: usize, routes: usize) -> String {
    let mut body = String::new();
    for level in 0..depth {
        writeln!(body, "constraints G{level} do").unwrap();
    }
    for route in 0..routes {
        writeln!(body, "get \"/r{route}\", to: redirect(\"/x\")").unwrap();
    }
    for _ in 0..depth {
        body.push_str("end\n");
    }
    draw(&body)
}

#[test]
fn a_list_at_the_limit_stays_whole() {
    let source = chi_with_guards(MAX_GUARDS, 1);
    let found = entries(Language::Go, &[("main.go", &source)]);
    assert_eq!(found[0].guards.len(), MAX_GUARDS);
    assert!(found[0].guards_complete);
}

#[test]
fn a_go_list_past_the_limit_keeps_its_start_and_tells_why() {
    let source = chi_with_guards(MAX_GUARDS + 1, 1);
    let found = entries(Language::Go, &[("main.go", &source)]);
    assert_eq!(found[0].guards.len(), MAX_GUARDS);
    assert!(!found[0].guards_complete);
    assert_eq!(
        found[0].guards_unknown_reason,
        Some(GuardsUnknownReason::TooManyGuards)
    );
    assert_eq!(GuardsUnknownReason::TooManyGuards.name(), "too_many_guards");
}

#[test]
fn a_csharp_list_past_the_limit_tells_why() {
    let source = csharp_with_guards(MAX_GUARDS + 4, 2);
    let found = entries(Language::CSharp, &[("a.cs", &source)]);
    assert_eq!(found.len(), 2);
    for entry in &found {
        assert_eq!(entry.guards.len(), MAX_GUARDS);
        assert_eq!(
            entry.guards_unknown_reason,
            Some(GuardsUnknownReason::TooManyGuards)
        );
    }
}

#[test]
fn a_rails_action_past_the_limit_keeps_the_first_guards_in_order() {
    let (routes, controller) = rails_action_with_guards(MAX_GUARDS + 1, 1);
    let found = entries(
        Language::Ruby,
        &[
            ("config/routes.rb", &routes),
            ("app/controllers/c_controller.rb", &controller),
        ],
    );
    assert_eq!(found[0].guards.len(), MAX_GUARDS);
    assert_eq!(found[0].guards[0], "g0");
    assert_eq!(
        found[0].guards[MAX_GUARDS - 1],
        format!("g{}", MAX_GUARDS - 1)
    );
    assert_eq!(
        found[0].guards_unknown_reason,
        Some(GuardsUnknownReason::TooManyGuards)
    );
}

#[test]
fn a_listed_rails_action_past_the_limit_uses_the_order_of_rails() {
    let (routes, controller) = rails_many_actions(MAX_GUARDS + 2, 2);
    let found = entries(
        Language::Ruby,
        &[
            ("config/routes.rb", &routes),
            ("app/controllers/c_controller.rb", &controller),
        ],
    );
    let listed = found
        .iter()
        .find(|entry| {
            entry
                .route
                .as_ref()
                .is_some_and(|route| route.path_text() == "/r0")
        })
        .unwrap();
    assert_eq!(listed.guards[0], "g1", "the skip removes g0");
    assert_eq!(listed.guards.len(), MAX_GUARDS);
    assert_eq!(
        listed.guards_unknown_reason,
        Some(GuardsUnknownReason::TooManyGuards)
    );
}

#[test]
fn rails_constraints_past_the_limit_tell_why() {
    let routes = rails_nested_constraints(MAX_GUARDS + 1, 1);
    let found = entries(Language::Ruby, &[("config/routes.rb", &routes)]);
    assert_eq!(found[0].guards.len(), MAX_GUARDS);
    assert_eq!(found[0].guards[0], "G0");
    assert_eq!(
        found[0].guards_unknown_reason,
        Some(GuardsUnknownReason::TooManyGuards)
    );
}

// The cost of the search.

fn search_time(language: Language, file: &str, source: &str) -> std::time::Duration {
    entries_timed(language, &[(file, source)], false).1
}

#[test]
#[ignore = "timing: run in release"]
fn timing_nested_ruby_modules_in_a_routes_file_cost_linear_time() {
    assert_scales_linearly("nested modules", 20_000, |depth| {
        let mut source = "module A\n".repeat(depth);
        source.push_str(&"end\n".repeat(depth));
        search_time(Language::Ruby, "config/routes.rb", &source)
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_routes_to_one_guarded_rails_action_cost_linear_time() {
    assert_scales_linearly("guarded action", 3_000, |size| {
        let (routes, controller) = rails_action_with_guards(size, size);
        entries_timed(
            Language::Ruby,
            &[
                ("config/routes.rb", &routes),
                ("app/controllers/c_controller.rb", &controller),
            ],
            false,
        )
        .1
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_rails_actions_with_many_guards_cost_linear_time() {
    assert_scales_linearly("guarded actions", 3_000, |size| {
        let (routes, controller) = rails_many_actions(size, size);
        entries_timed(
            Language::Ruby,
            &[
                ("config/routes.rb", &routes),
                ("app/controllers/c_controller.rb", &controller),
            ],
            false,
        )
        .1
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_routes_inside_many_rails_constraints_cost_linear_time() {
    assert_scales_linearly("nested constraints", 2_000, |size| {
        let routes = rails_nested_constraints(size, size);
        search_time(Language::Ruby, "config/routes.rb", &routes)
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_rust_route_calls_cost_linear_time() {
    assert_scales_linearly("rust route calls", 40_000, |size| {
        let mut source = String::from("struct X;\nimpl X { fn route(&self) {} }\nfn f(x: &X) {\n");
        source.push_str(&"    x.route();\n".repeat(size));
        source.push_str("}\n");
        search_time(Language::Rust, "a.rs", &source)
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_go_layers_and_routes_cost_linear_time() {
    assert_scales_linearly("go layers", 3_000, |size| {
        search_time(Language::Go, "main.go", &chi_with_guards(size, size))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_groups_under_a_router_with_many_layers_cost_linear_time() {
    assert_scales_linearly("go groups", 3_000, |size| {
        let mut body = String::from("func routes() {\n\tr := chi.NewRouter()\n");
        body.push_str(&"\tr.Use(auth)\n".repeat(size));
        for group in 0..size {
            writeln!(
                body,
                "\tr.Route(\"/g{group}\", func(s chi.Router) {{\n\t\ts.Get(\"/x\", first)\n\t}})"
            )
            .unwrap();
        }
        body.push('}');
        search_time(Language::Go, "main.go", &go(CHI, &body))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_csharp_actions_with_many_class_guards_cost_linear_time() {
    assert_scales_linearly("csharp guards", 3_000, |size| {
        search_time(Language::CSharp, "a.cs", &csharp_with_guards(size, size))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_java_methods_with_many_class_guards_cost_linear_time() {
    assert_scales_linearly("java guards", 3_000, |size| {
        let mut source = String::from("import org.springframework.web.bind.annotation.*;\n");
        source.push_str(&"@PreAuthorize(\"x\")\n".repeat(size));
        source.push_str("class C {\n");
        for method in 0..size {
            writeln!(
                source,
                "    @GetMapping(\"/r{method}\") void a{method}() {{}}"
            )
            .unwrap();
        }
        source.push_str("}\n");
        search_time(Language::Java, "C.java", &source)
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_net_http_calls_with_many_imports_cost_linear_time() {
    assert_scales_linearly("go imports", 3_000, |size| {
        let mut source = String::from("package p\nimport (\n");
        for import in 0..size {
            writeln!(source, "\t_ \"example.test/m{import}\"").unwrap();
        }
        source.push_str(
            "\t\"net/http\"\n)\nfunc h(w http.ResponseWriter, r *http.Request) {}\nfunc f() {\n",
        );
        for route in 0..size {
            writeln!(source, "\thttp.HandleFunc(\"/r{route}\", h)").unwrap();
        }
        source.push_str("}\n");
        search_time(Language::Go, "a.go", &source)
    });
}
