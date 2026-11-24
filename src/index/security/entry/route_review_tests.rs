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

// The second review: forms of proof, and suppression across branches.

#[test]
fn a_next_verb_can_be_an_export_alias() {
    assert_eq!(
        http_rows(
            Language::TypeScript,
            &[(
                "app/x/route.ts",
                "function handler() {}\nexport { handler as GET };\n"
            )]
        ),
        ["next GET /x handler"]
    );
    assert_eq!(
        http_rows(
            Language::TypeScript,
            &[(
                "app/x/route.ts",
                "function GET() {}\nexport { GET as POST };\n"
            )]
        ),
        ["next POST /x GET"]
    );
}

#[test]
fn an_actix_verb_through_a_repository_re_export_is_a_route() {
    let found = http_rows(
        Language::Rust,
        &[
            ("src/lib.rs", "pub use actix_web::get;\nmod routes;\n"),
            (
                "src/routes.rs",
                "use crate::get;\n\n#[get(\"/x\")]\nasync fn handler() {}\n",
            ),
        ],
    );
    assert_eq!(found, ["actix GET /x handler"]);
}

#[test]
fn a_verb_through_a_re_export_of_another_crate_is_not_a_route() {
    let found = http_rows(
        Language::Rust,
        &[
            ("src/lib.rs", "pub use my_macros::get;\nmod routes;\n"),
            (
                "src/routes.rs",
                "use crate::get;\n\n#[get(\"/x\")]\nasync fn handler() {}\n",
            ),
        ],
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_csharp_global_using_alias_of_another_file_proves_a_receiver() {
    let found = http_rows(
        Language::CSharp,
        &[
            (
                "GlobalUsings.cs",
                "global using Routes = Microsoft.AspNetCore.Routing.IEndpointRouteBuilder;\n",
            ),
            (
                "Api.cs",
                "static class Api {\n    static void Add(Routes routes) {\n        routes.MapGet(\"/x\", () => 1);\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(found, ["aspnet GET /x -"]);
}

#[test]
fn a_csharp_global_using_of_asp_net_proves_a_receiver() {
    let found = http_rows(
        Language::CSharp,
        &[
            ("GlobalUsings.cs", "global using Microsoft.AspNetCore.Builder;\n"),
            (
                "Api.cs",
                "class Api {\n    void Add(object endpoints) {\n        endpoints.MapGet(\"/x\", () => 1);\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(found, ["aspnet GET /x -"]);
}

#[test]
fn a_csharp_field_read_through_this_proves_a_receiver() {
    let field = "class Api {\n    private WebApplication app;\n    void H() {}\n    void Add() {\n        this.app.MapGet(\"/x\", H);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", field)]),
        ["aspnet GET /x H"]
    );
    let property = "class Api {\n    private WebApplication App { get; }\n    void H() {}\n    void Add() {\n        this.App.MapGet(\"/x\", H);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", property)]),
        ["aspnet GET /x H"]
    );
}

#[test]
fn a_csharp_type_must_match_a_whole_name() {
    let source = "class NotWebApplication {\n    public void MapGet(string path, System.Action handler) {}\n}\nclass C {\n    void H() {}\n    void F(NotWebApplication app) {\n        app.MapGet(\"/x\", H);\n    }\n}\n";
    let found = http_rows(Language::CSharp, &[("a.cs", source)]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_fully_qualified_spring_annotation_is_a_route() {
    let source = "@org.springframework.web.bind.annotation.RequestMapping(\"/api\")\nclass C {\n    @org.springframework.web.bind.annotation.GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::Java, &[("C.java", source)]),
        ["spring GET /api/x handler"]
    );
}

#[test]
fn another_spring_import_does_not_prove_a_mapping() {
    let source = "import org.springframework.stereotype.Component;\n\n@interface GetMapping {\n    String value();\n}\n\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    let found = http_rows(Language::Java, &[("C.java", source)]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_mapping_of_the_file_hides_a_spring_wildcard_import() {
    let source = "import org.springframework.web.bind.annotation.*;\n\n@interface GetMapping {\n    String value();\n}\n\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    let found = http_rows(Language::Java, &[("C.java", source)]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_conditional_chi_route_does_not_hide_a_mounted_route() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on bool) {\n\tsub := chi.NewRouter()\n\tsub.Get(\"/y\", first)\n\troot := chi.NewRouter()\n\troot.Mount(\"/api\", sub)\n\tif on {\n\t\troot.Get(\"/api/y\", second)\n\t}\n}"
        ),
        ["chi GET /api/y first", "chi GET /api/y second"]
    );
}

#[test]
fn a_mux_route_hides_a_later_route_of_its_branch() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on bool) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tr.HandleFunc(\"/x\", first)\n\t\tr.HandleFunc(\"/x\", second)\n\t}\n}"
        ),
        ["gorilla/mux ANY /x first"]
    );
}

#[test]
fn a_mux_route_of_a_branch_does_not_hide_a_route_of_the_other_branch() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on bool) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tr.HandleFunc(\"/x\", first)\n\t} else {\n\t\tr.HandleFunc(\"/x\", second)\n\t}\n}"
        ),
        ["gorilla/mux ANY /x first", "gorilla/mux ANY /x second"]
    );
}

#[test]
fn an_opaque_mux_route_of_a_branch_does_not_hide_the_other_branch() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on bool, prefix string) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tr.Path(prefix).HandlerFunc(first)\n\t} else {\n\t\tr.HandleFunc(\"/x\", second)\n\t}\n}"
        ),
        ["gorilla/mux ANY /x second"]
    );
}

#[test]
fn an_opaque_mux_route_still_hides_a_later_route_that_can_run_with_it() {
    assert!(go_rows(
        MUX,
        "func routes(prefix string) {\n\tr := mux.NewRouter()\n\tr.Path(prefix).HandlerFunc(first)\n\tr.HandleFunc(\"/x\", second)\n}"
    )
    .is_empty());
}

#[test]
fn net_http_siblings_of_another_method_do_not_cover_a_catch_all() {
    assert_eq!(
        go_rows(
            "",
            "func routes() {\n\tm := http.NewServeMux()\n\tm.HandleFunc(\"GET /x/{rest...}\", first)\n\tm.HandleFunc(\"POST /x/{$}\", second)\n\tm.HandleFunc(\"POST /x/{id}\", second)\n\tm.HandleFunc(\"POST /x/{id}/{tail...}\", second)\n}"
        ),
        [
            "net/http GET /x/{rest...} first",
            "net/http POST /x/{$} second",
            "net/http POST /x/{id} second",
            "net/http POST /x/{id}/{tail...} second",
        ]
    );
}

#[test]
fn net_http_siblings_in_a_branch_do_not_cover_a_catch_all() {
    let found = go_rows(
        "",
        "func routes(on bool) {\n\tm := http.NewServeMux()\n\tm.HandleFunc(\"/x/{rest...}\", first)\n\tif on {\n\t\tm.HandleFunc(\"/x/{$}\", second)\n\t\tm.HandleFunc(\"/x/{id}\", second)\n\t\tm.HandleFunc(\"/x/{id}/{tail...}\", second)\n\t}\n}",
    );
    assert!(
        found.contains(&"net/http ANY /x/{rest...} first".to_string()),
        "{found:?}"
    );
}

#[test]
fn net_http_siblings_without_a_method_cover_a_catch_all_of_a_method() {
    let found = go_rows(
        "",
        "func routes() {\n\tm := http.NewServeMux()\n\tm.HandleFunc(\"GET /x/{rest...}\", first)\n\tm.HandleFunc(\"/x/{$}\", second)\n\tm.HandleFunc(\"/x/{id}\", second)\n\tm.HandleFunc(\"/x/{id}/{tail...}\", second)\n}",
    );
    assert!(
        !found.iter().any(|row| row.contains("{rest...}")),
        "{found:?}"
    );
}

#[test]
fn a_django_import_that_the_file_binds_again_is_no_proof() {
    let assigned = "from django.urls import path\n\ndef fake(pattern, handler):\n    return None\n\ndef view(request):\n    pass\n\npath = fake\nurlpatterns = [path(\"x/\", view)]\n";
    assert!(http_rows(Language::Python, &[("urls.py", assigned)]).is_empty());
    let defined = "from django.urls import path\n\ndef path(pattern, handler):\n    return None\n\ndef view(request):\n    pass\n\nurlpatterns = [path(\"x/\", view)]\n";
    assert!(http_rows(Language::Python, &[("urls.py", defined)]).is_empty());
}

#[test]
fn a_local_variable_of_a_function_does_not_rebind_a_django_import() {
    let source = "from django.urls import path\n\ndef view(request):\n    path = request.path\n    return path\n\nurlpatterns = [path(\"x/\", view)]\n";
    assert_eq!(http_rows(Language::Python, &[("urls.py", source)]).len(), 1);
}

#[test]
fn a_placeholder_at_the_limit_does_not_make_a_longer_list() {
    let mut body = String::from(
        "func routes(all []func(http.Handler) http.Handler) {\n\tr := chi.NewRouter()\n",
    );
    body.push_str(&"\tr.Use(auth)\n".repeat(MAX_GUARDS));
    body.push_str("\tr.Use(all[0])\n\tr.Get(\"/x\", first)\n}");
    let entry = only_go_entry(CHI, &body);
    assert_eq!(entry.guards.len(), MAX_GUARDS);
    assert!(!entry.guards_complete);
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::TooManyGuards)
    );
}

#[test]
fn a_destructuring_assignment_makes_a_constraint_local_unknown() {
    let entry = only_rails_entry(
        "  gate = {}\n  gate, unused = [RouteGate.new, nil]\n  get \"/x\", to: redirect(\"/y\"), constraints: gate",
    );
    assert!(!entry.guards_complete);
    assert_eq!(
        entry.guards_unknown_reason,
        Some(GuardsUnknownReason::UnreadMiddleware)
    );
    let only = only_rails_entry(
        "  gate, unused = [{}, nil]\n  get \"/x\", to: redirect(\"/y\"), constraints: gate",
    );
    assert!(!only.guards_complete);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_django_routes_with_many_imports_cost_linear_time() {
    assert_scales_linearly("django proof", 6_000, |size| {
        let mut source = String::new();
        for import in 0..size {
            writeln!(source, "import m{import}").unwrap();
        }
        source.push_str(
            "from django.urls import path\n\ndef view(request):\n    pass\n\nurlpatterns = [\n",
        );
        for route in 0..size {
            writeln!(source, "    path(\"r{route}/\", view),").unwrap();
        }
        source.push_str("]\n");
        search_time(Language::Python, "urls.py", &source)
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_rust_verbs_with_many_imports_cost_linear_time() {
    assert_scales_linearly("rust proof", 30_000, |size| {
        let mut source = String::from("use actix_web::*;\n");
        for import in 0..size {
            writeln!(source, "use m{import}::item{import};").unwrap();
        }
        for route in 0..size {
            writeln!(source, "#[get(\"/r{route}\")]\nasync fn h{route}() {{}}").unwrap();
        }
        search_time(Language::Rust, "a.rs", &source)
    });
}

// The third review.

#[test]
fn a_next_function_exported_by_name_and_alias_keeps_both_routes() {
    assert_eq!(
        http_rows(
            Language::TypeScript,
            &[(
                "app/x/route.ts",
                "function GET() {}\nexport { GET, GET as POST };\n"
            )]
        ),
        ["next GET /x GET", "next POST /x GET"]
    );
}

#[test]
fn a_global_alias_must_name_the_namespace_of_asp_net() {
    let found = http_rows(
        Language::CSharp,
        &[
            ("GlobalUsings.cs", "global using Routes = Acme.WebApplication;\n"),
            (
                "Api.cs",
                "class C {\n    void Handler() {}\n    void Add(Routes routes) {\n        routes.MapGet(\"/x\", Handler);\n    }\n}\n",
            ),
        ],
    );
    assert!(found.is_empty(), "{found:?}");
    let qualified = http_rows(
        Language::CSharp,
        &[
            (
                "GlobalUsings.cs",
                "global using Routes = global::Microsoft.AspNetCore.Routing.IEndpointRouteBuilder;\n",
            ),
            (
                "Api.cs",
                "class C {\n    void Handler() {}\n    void Add(Routes routes) {\n        routes.MapGet(\"/x\", Handler);\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(qualified, ["aspnet GET /x Handler"]);
}

/// The rows of C# files in a repository with `.csproj` files in the named directories.
fn csharp_project_rows(files: &[(&str, &str)], projects: &[&str]) -> Vec<String> {
    use crate::index::build::CodeIndex;
    use crate::parser::Parser;
    use std::path::{Path, PathBuf};
    let read = |path: &Path| {
        files
            .iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, text)| text.as_bytes().to_vec())
            .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
    };
    let list = |directory: &Path| -> Vec<PathBuf> {
        let name = directory.to_string_lossy();
        let name = name.trim_start_matches("./").trim_end_matches('/');
        let name = if name == "." { "" } else { name };
        if projects.contains(&name) {
            vec![directory.join("App.csproj")]
        } else {
            Vec::new()
        }
    };
    let paths: Vec<PathBuf> = files.iter().map(|(name, _)| PathBuf::from(name)).collect();
    let index = CodeIndex::build(Path::new("."), &paths, &read, &list);
    let parsed: Vec<_> = index
        .files
        .iter()
        .map(|file| {
            let text = files.iter().find(|(name, _)| *name == file.file).unwrap().1;
            Parser::new()
                .parse(text.as_bytes(), Language::CSharp, Path::new(&file.file))
                .unwrap()
        })
        .collect();
    let mut rows: Vec<String> = super::find_entry_points(&parsed, &index.files)
        .iter()
        .filter(|entry| entry.kind == EntryKind::HttpHandler)
        .map(row)
        .collect();
    rows.sort();
    rows
}

const GLOBAL_USING: &str = "global using Microsoft.AspNetCore.Builder;\n";
const FAKE_ROUTES: &str = "class Fake {\n    public void MapGet(string path, System.Action handler) {}\n}\nclass Api {\n    void Handler() {}\n    void Add(Fake app) {\n        app.MapGet(\"/x\", Handler);\n    }\n}\n";

#[test]
fn a_global_using_does_not_reach_another_project() {
    let found = csharp_project_rows(
        &[
            ("A/GlobalUsings.cs", GLOBAL_USING),
            ("B/Api.cs", FAKE_ROUTES),
        ],
        &["A", "B"],
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_global_using_reaches_the_files_of_its_project() {
    let found = csharp_project_rows(
        &[
            ("A/GlobalUsings.cs", GLOBAL_USING),
            ("A/Deep/Api.cs", FAKE_ROUTES),
        ],
        &["A"],
    );
    assert_eq!(found, ["aspnet GET /x Handler"]);
}

#[test]
fn a_nested_project_has_its_own_global_usings() {
    let found = csharp_project_rows(
        &[
            ("A/GlobalUsings.cs", GLOBAL_USING),
            ("A/B/Api.cs", FAKE_ROUTES),
        ],
        &["A", "A/B"],
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_repository_with_no_project_file_is_one_project() {
    let found = csharp_project_rows(
        &[
            ("A/GlobalUsings.cs", GLOBAL_USING),
            ("B/Api.cs", FAKE_ROUTES),
        ],
        &[],
    );
    assert_eq!(found, ["aspnet GET /x Handler"]);
}

#[test]
fn a_csharp_field_through_this_with_spaces_proves_a_receiver() {
    let source = "class Api {\n    Microsoft.AspNetCore.Builder.WebApplication app;\n    void Handler() {}\n    void Add() {\n        this . app.MapGet(\"/x\", Handler);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", source)]),
        ["aspnet GET /x Handler"]
    );
    let group = "class Api {\n    WebApplication app;\n    void Handler() {}\n    void Add() {\n        var api = this . app.MapGroup(\"/g\");\n        api.MapGet(\"/x\", Handler);\n    }\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("Api.cs", group)]),
        ["aspnet GET /x Handler"]
    );
}

#[test]
fn a_class_mapping_of_another_package_gives_no_prefix() {
    let source = "@com.example.RequestMapping(\"/wrong\")\nclass C {\n    @org.springframework.web.bind.annotation.GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::Java, &[("C.java", source)]),
        ["spring GET /x handler"]
    );
    let imported = "import com.example.RequestMapping;\nimport org.springframework.web.bind.annotation.GetMapping;\n\n@RequestMapping(\"/wrong\")\nclass C {\n    @GetMapping(\"/x\")\n    void handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::Java, &[("C.java", imported)]),
        ["spring GET /x handler"]
    );
}

#[test]
fn each_python_binding_form_hides_a_django_import() {
    const HEAD: &str = "from django.urls import path\n\ndef view(request):\n    pass\n\n";
    const ROUTES: &str = "urlpatterns = [path(\"x/\", view)]\n";
    let forms = [
        "with open_gate() as path:\n    pass\n",
        "try:\n    pass\nexcept ValueError as path:\n    pass\n",
        "import other as path\n",
        "from other import thing as path\n",
        "class path:\n    pass\n",
        "def rebind():\n    global path\n    path = None\n",
        "for path in []:\n    pass\n",
        "path, other = (1, 2)\n",
        "(path := 1)\n",
        "match 1:\n    case path:\n        pass\n",
    ];
    for form in forms {
        let source = format!("{HEAD}{form}{ROUTES}");
        let found = http_rows(Language::Python, &[("urls.py", &source)]);
        assert!(found.is_empty(), "{form}: {found:?}");
    }
}

#[test]
fn a_local_binding_of_a_function_or_class_keeps_a_django_import() {
    const HEAD: &str = "from django.urls import path\n\ndef view(request):\n    pass\n\n";
    const ROUTES: &str = "urlpatterns = [path(\"x/\", view)]\n";
    let forms = [
        "def local():\n    with open_gate() as path:\n        pass\n",
        "class Holder:\n    path = 1\n",
        "values = [path for path in []]\n",
    ];
    for form in forms {
        let source = format!("{HEAD}{form}{ROUTES}");
        let found = http_rows(Language::Python, &[("urls.py", &source)]);
        assert_eq!(found.len(), 1, "{form}: {found:?}");
    }
}

const CHI_MOUNT: &str =
    "sub := chi.NewRouter()\n\t\tsub.Get(\"/y\", first)\n\t\troot.Mount(\"/api\", sub)\n";
const CHI_ROUTE: &str = "root.Get(\"/api/y\", second)\n";

fn chi_mount_rows(body: &str) -> Vec<String> {
    go_rows(
        CHI,
        &format!("func routes(on, off bool) {{\n\troot := chi.NewRouter()\n{body}\n}}"),
    )
}

#[test]
fn a_chi_route_in_the_branch_of_a_mount_replaces_it() {
    assert_eq!(
        chi_mount_rows(&format!("\tif on {{\n\t\t{CHI_MOUNT}\t\t{CHI_ROUTE}\t}}")),
        ["chi GET /api/y second"]
    );
}

#[test]
fn a_chi_route_in_an_outer_branch_replaces_a_mount_in_an_inner_branch() {
    assert_eq!(
        chi_mount_rows(&format!(
            "\tif on {{\n\t\tif off {{\n\t\t{CHI_MOUNT}\t\t}}\n\t\t{CHI_ROUTE}\t}}"
        )),
        ["chi GET /api/y second"]
    );
}

#[test]
fn a_chi_route_in_an_inner_branch_does_not_replace_a_mount() {
    assert_eq!(
        chi_mount_rows(&format!(
            "\tif on {{\n\t\t{CHI_MOUNT}\t\tif off {{\n\t\t\t{CHI_ROUTE}\t\t}}\n\t}}"
        )),
        ["chi GET /api/y first", "chi GET /api/y second"]
    );
}

#[test]
fn a_chi_route_in_a_sibling_branch_does_not_replace_a_mount() {
    assert_eq!(
        chi_mount_rows(&format!(
            "\tif on {{\n\t\t{CHI_MOUNT}\t}} else {{\n\t\t{CHI_ROUTE}\t}}"
        )),
        ["chi GET /api/y first", "chi GET /api/y second"]
    );
}

#[test]
fn a_later_chi_route_in_an_outer_branch_replaces_a_route_in_an_inner_branch() {
    assert_eq!(
        go_rows(
            CHI,
            "func routes(on, off bool) {\n\tr := chi.NewRouter()\n\tif on {\n\t\tif off {\n\t\t\tr.Get(\"/x\", first)\n\t\t} else {\n\t\t\tr.Get(\"/x\", third)\n\t\t}\n\t\tr.Get(\"/x\", second)\n\t}\n}"
        ),
        ["chi GET /x second"]
    );
}

#[test]
fn an_earlier_mux_route_of_an_outer_branch_hides_a_nested_route() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on, off bool) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tr.HandleFunc(\"/x\", first)\n\t\tif off {\n\t\t\tr.HandleFunc(\"/x\", second)\n\t\t}\n\t}\n}"
        ),
        ["gorilla/mux ANY /x first"]
    );
}

#[test]
fn an_earlier_mux_route_of_a_nested_branch_does_not_hide_an_outer_route() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on, off bool) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tif off {\n\t\t\tr.HandleFunc(\"/x\", first)\n\t\t}\n\t\tr.HandleFunc(\"/x\", second)\n\t}\n}"
        ),
        ["gorilla/mux ANY /x first", "gorilla/mux ANY /x second"]
    );
}

#[test]
fn a_mux_route_in_a_sibling_of_a_nested_branch_stays() {
    assert_eq!(
        go_rows(
            MUX,
            "func routes(on, off bool) {\n\tr := mux.NewRouter()\n\tif on {\n\t\tif off {\n\t\t\tr.HandleFunc(\"/x\", first)\n\t\t} else {\n\t\t\tr.HandleFunc(\"/x\", second)\n\t\t}\n\t}\n}"
        ),
        ["gorilla/mux ANY /x first", "gorilla/mux ANY /x second"]
    );
}

const NET_SIBLINGS: &str = "m.HandleFunc(\"/x/{$}\", second)\n\tm.HandleFunc(\"/x/{id}\", second)\n\tm.HandleFunc(\"/x/{id}/{tail...}\", second)\n";
const NET_CATCH_ALL: &str = "m.HandleFunc(\"/x/{rest...}\", first)\n";

fn net_rows(body: &str) -> Vec<String> {
    go_rows(
        "",
        &format!("func routes(on, off bool) {{\n\tm := http.NewServeMux()\n{body}\n}}"),
    )
}

fn has_catch_all(rows: &[String]) -> bool {
    rows.iter().any(|row| row.contains("{rest...}"))
}

#[test]
fn net_http_siblings_in_the_branch_of_a_catch_all_cover_it() {
    let rows = net_rows(&format!(
        "\tif on {{\n\t{NET_CATCH_ALL}\t{NET_SIBLINGS}\t}}"
    ));
    assert!(!has_catch_all(&rows), "{rows:?}");
}

#[test]
fn net_http_siblings_in_an_outer_branch_cover_a_nested_catch_all() {
    let rows = net_rows(&format!(
        "\tif on {{\n\t{NET_SIBLINGS}\tif off {{\n\t{NET_CATCH_ALL}\t}}\n\t}}"
    ));
    assert!(!has_catch_all(&rows), "{rows:?}");
}

#[test]
fn net_http_siblings_in_a_nested_branch_do_not_cover_an_outer_catch_all() {
    let rows = net_rows(&format!(
        "\tif on {{\n\t{NET_CATCH_ALL}\tif off {{\n\t{NET_SIBLINGS}\t}}\n\t}}"
    ));
    assert!(has_catch_all(&rows), "{rows:?}");
}

#[test]
fn net_http_siblings_in_a_sibling_branch_do_not_cover_a_catch_all() {
    let rows = net_rows(&format!(
        "\tif on {{\n\t{NET_CATCH_ALL}\t}} else {{\n\t{NET_SIBLINGS}\t}}"
    ));
    assert!(has_catch_all(&rows), "{rows:?}");
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_nested_branches_with_chi_mounts_cost_linear_time() {
    assert_scales_linearly("branch depth", 1_500, |size| {
        let mut body = String::from("func routes(on bool) {\n\troot := chi.NewRouter()\n");
        for level in 0..size {
            writeln!(
                body,
                "if on {{\nsub{level} := chi.NewRouter()\nsub{level}.Get(\"/y\", first)\nroot.Mount(\"/m{level}\", sub{level})\nroot.Get(\"/r{level}\", second)"
            )
            .unwrap();
        }
        body.push_str(&"}\n".repeat(size));
        body.push('}');
        search_time(Language::Go, "main.go", &go(CHI, &body))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_nested_branches_with_mux_routes_cost_linear_time() {
    assert_scales_linearly("mux branch depth", 1_500, |size| {
        let mut body = String::from("func routes(on bool) {\n\tr := mux.NewRouter()\n");
        for level in 0..size {
            writeln!(body, "if on {{\nr.HandleFunc(\"/r{level}\", first)").unwrap();
        }
        body.push_str(&"}\n".repeat(size));
        body.push('}');
        search_time(Language::Go, "main.go", &go(MUX, &body))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_net_http_catch_alls_in_nested_branches_cost_linear_time() {
    assert_scales_linearly("net/http branch depth", 1_500, |size| {
        let mut body = String::from("func routes(on bool) {\n\tm := http.NewServeMux()\n");
        for level in 0..size {
            writeln!(
                body,
                "if on {{\nm.HandleFunc(\"/p{level}/{{rest...}}\", first)\nm.HandleFunc(\"/p{level}/{{$}}\", second)\nm.HandleFunc(\"/p{level}/{{id}}\", second)"
            )
            .unwrap();
        }
        body.push_str(&"}\n".repeat(size));
        body.push('}');
        search_time(Language::Go, "main.go", &go("", &body))
    });
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_csharp_projects_with_global_usings_cost_linear_time() {
    assert_scales_linearly("csharp projects", 1_500, |size| {
        let names: Vec<(String, String)> = (0..size)
            .map(|project| {
                (
                    format!("p{project}/Api.cs"),
                    "global using Routes = Acme.Other;\nclass Api {\n    void H() {}\n    void Add(object app) {\n        app.MapGet(\"/x\", H);\n    }\n}\n"
                        .to_string(),
                )
            })
            .collect();
        let files: Vec<(&str, &str)> = names
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        entries_timed(Language::CSharp, &files, false).1
    });
}

/// The documentation states this: controller attributes need no proof of the framework.
#[test]
fn a_csharp_controller_attribute_is_matched_by_its_name() {
    let source = "class HttpGetAttribute : System.Attribute {\n    public HttpGetAttribute(string path) {}\n}\n\nclass C {\n    [HttpGet(\"/x\")]\n    void Handler() {}\n}\n";
    assert_eq!(
        http_rows(Language::CSharp, &[("a.cs", source)]),
        ["aspnet GET /x Handler"]
    );
}

/// The documentation states this limit of the search through `pub use`.
#[test]
fn a_re_export_chain_past_the_limit_gives_no_route() {
    let chain = |length: usize| -> Vec<(String, String)> {
        let mut files = vec![(
            "src/routes.rs".to_string(),
            "use crate::m0::get;\n\n#[get(\"/x\")]\nasync fn handler() {}\n".to_string(),
        )];
        let mut root = String::from("mod routes;\n");
        for level in 0..length {
            writeln!(root, "mod m{level};").unwrap();
            let text = if level + 1 == length {
                "pub use actix_web::get;\n".to_string()
            } else {
                format!("pub use crate::m{}::get;\n", level + 1)
            };
            files.push((format!("src/m{level}.rs"), text));
        }
        files.push(("src/lib.rs".to_string(), root));
        files
    };
    let rows = |length: usize| {
        let owned = chain(length);
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        http_rows(Language::Rust, &files)
    };
    assert_eq!(rows(31), ["actix GET /x handler"]);
    assert!(rows(32).is_empty());
}

#[test]
fn a_mux_route_stays_hidden_after_a_helper_runs_between_the_routes() {
    assert_eq!(
        go_rows(
            MUX,
            "var r = mux.NewRouter()\n\nfunc helper() {\n\tr.HandleFunc(\"/h\", third)\n}\n\nfunc routes(on bool) {\n\tif on {\n\t\tr.HandleFunc(\"/x\", first)\n\t\thelper()\n\t\tr.HandleFunc(\"/x\", second)\n\t}\n}"
        ),
        ["gorilla/mux ANY /h third", "gorilla/mux ANY /x first"]
    );
}
