//! Tests of the resolution of Go calls.

use super::testing::rows;

/// The rows of the calls of `name` in `file`.
fn rows_of(files: &[(&str, &str)], file: &str, name: &str) -> Vec<String> {
    rows(files, file)
        .into_iter()
        .filter(|row| {
            let callee = row.split(' ').nth(1).unwrap_or_default();
            callee.ends_with(&format!(".{name}")) || callee.ends_with(&format!(">{name}"))
        })
        .collect()
}

/// A Go qualifier of a type always names an imported package. A package
/// outside the repository does not have the type of the repository with the
/// same name.
#[test]
fn test_type_of_an_external_package_is_not_a_repository_type() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "gitcmd/command.go",
            "package gitcmd\n\ntype Command struct{}\n\nfunc (c *Command) Run() error { return nil }\n",
        ),
        (
            "main.go",
            "package main\n\nimport (\n\t\"github.com/urfave/cli/v3\"\n\n\t\"example.com/shop/gitcmd\"\n)\n\nfunc run(app *cli.Command, git *gitcmd.Command) {\n\tapp.Run()\n\tgit.Run()\n}\n",
        ),
    ];
    assert_eq!(
        rows_of(&files, "main.go", "Run"),
        [
            "10 run>app.Run Unresolved - []",
            "11 run>git.Run UniqueCandidate ReceiverDeclaredType [gitcmd/command.go#Command.Run]",
        ]
    );
}

/// A method of an embedded field is a method of the struct that embeds it.
#[test]
fn test_method_promoted_through_an_embedded_field() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "web/base.go",
            "package web\n\ntype Base struct{}\n\nfunc (b *Base) JSON() {}\n\nfunc (b *Base) Status() {}\n",
        ),
        (
            "context/context.go",
            "package context\n\nimport \"example.com/shop/web\"\n\ntype Context struct {\n\t*web.Base\n\tname string\n}\n\nfunc (c *Context) Status() {}\n\nfunc handle(ctx *Context) {\n\tctx.JSON()\n\tctx.Status()\n\tctx.Base.JSON()\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "context/context.go"),
        [
            "13 handle>ctx.JSON UniqueCandidate ReceiverDeclaredType [web/base.go#Base.JSON]",
            "14 handle>ctx.Status UniqueCandidate ReceiverDeclaredType [context/context.go#Context.Status]",
            "15 handle>ctx.Base::JSON UniqueCandidate ReceiverDeclaredType [web/base.go#Base.JSON]",
        ]
    );
}

/// Two embedded fields at one depth with the same method make the call
/// ambiguous. Go does not compile such a call.
#[test]
fn test_two_embedded_fields_with_one_method_give_no_edge() {
    let source = "package a\n\ntype Left struct{}\n\nfunc (l Left) Close() {}\n\ntype Right struct{}\n\nfunc (r Right) Close() {}\n\ntype Both struct {\n\tLeft\n\tRight\n}\n\nfunc run(both Both) {\n\tboth.Close()\n}\n";
    assert_eq!(
        rows_of(&[("a.go", source)], "a.go", "Close"),
        ["17 run>both.Close Ambiguous ReceiverDeclaredType [a.go#Left.Close, a.go#Right.Close]"]
    );
}

/// A `_test.go` file is in the package only for the files of its own
/// directory that are tests, and a `_test` package is a different package.
#[test]
fn test_package_of_a_test_file() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "a/a.go",
            "package a\n\nfunc load() {}\n\nfunc run() {\n\tcheck()\n\tload()\n}\n",
        ),
        (
            "a/a_test.go",
            "package a\n\nfunc check() {}\n\nfunc TestRun() {\n\tload()\n}\n",
        ),
        (
            "a/external_test.go",
            "package a_test\n\nfunc load() {}\n\nfunc TestOther() {\n\tcheck()\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "a/a.go"),
        [
            "6 run>check Unresolved - []",
            "7 run>load UniqueCandidate SameFile [a/a.go#load]",
        ]
    );
    assert_eq!(
        rows(&files, "a/a_test.go"),
        ["6 TestRun>load UniqueCandidate SamePackage [a/a.go#load]"]
    );
    assert_eq!(
        rows(&files, "a/external_test.go"),
        ["6 TestOther>check Unresolved - []"]
    );
}

/// A call with explicit type arguments and one argument has the syntax of a
/// conversion to a generic type. It is a call when the name is a function.
#[test]
fn test_call_with_explicit_type_arguments() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "util/util.go",
            "package util\n\nfunc Must[T any](value T) T { return value }\n",
        ),
        (
            "a.go",
            "package main\n\nimport \"example.com/shop/util\"\n\ntype Box[T any] struct{ value T }\n\nfunc errorAs[T error](err error) (T, bool) {\n\tvar zero T\n\treturn zero, false\n}\n\nfunc run(err error) {\n\t_, _ = errorAs[error](err)\n\t_ = util.Must[int](1)\n\t_ = Box[int](Box[int]{})\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "a.go"),
        [
            "13 run>errorAs UniqueCandidate SameFile [a.go#errorAs]",
            "14 run>util.Must UniqueCandidate ImportedModule [util/util.go#Must]",
            "15 run>Box Unresolved - []",
        ]
    );
}

/// A call through an interface gives no edge: the syntax does not tell the
/// dynamic type. The candidates are the method of the interface and the
/// methods of the repository types that implement the interface.
#[test]
fn test_interface_call_offers_the_implementations() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "storage/storage.go",
            "package storage\n\ntype Closer interface {\n\tClose() error\n}\n\ntype Store interface {\n\tCloser\n\tSave(path string) error\n}\n\ntype Disk struct{}\n\nfunc (d *Disk) Save(path string) error { return nil }\n\nfunc (d *Disk) Close() error { return nil }\n\ntype Cloud struct{ *Disk }\n\ntype Partial struct{}\n\nfunc (p Partial) Save(path string) error { return nil }\n\ntype Memory struct{}\n\nfunc (m Memory) Save(path string) error { return nil }\n\nfunc (m Memory) Close() error { return nil }\n",
        ),
        (
            "main.go",
            "package main\n\nimport \"example.com/shop/storage\"\n\nfunc run(store storage.Store) {\n\t_ = store.Save(\"a\")\n\t_ = store.Close()\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "main.go"),
        [
            "6 run>store.Save Ambiguous Implementation [storage/storage.go#Disk.Save, storage/storage.go#Memory.Save, storage/storage.go#Store.Save]",
            "7 run>store.Close Ambiguous Implementation [storage/storage.go#Closer.Close, storage/storage.go#Disk.Close, storage/storage.go#Memory.Close]",
        ]
    );
}

/// A struct that embeds an interface has the methods of the interface. Its
/// own method of a name of the interface is an implementation.
#[test]
fn test_struct_that_embeds_the_interface_is_an_implementation() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "storage/storage.go",
            "package storage\n\ntype store interface {\n\tload() error\n\tdestroy() error\n}\n\nfunc run(s store) {\n\t_ = s.destroy()\n}\n",
        ),
        (
            "storage/storage_test.go",
            "package storage\n\ntype failing struct {\n\tstore\n}\n\nfunc (f *failing) destroy() error { return f.store.destroy() }\n",
        ),
    ];
    assert_eq!(
        rows(&files, "storage/storage.go"),
        ["9 run>s.destroy Ambiguous Implementation [storage/storage.go#store.destroy, storage/storage_test.go#failing.destroy]"]
    );
}

/// Two structs that embed a pointer to each other are a cycle. The search for
/// the methods of an implementation stops at the cycle.
#[test]
fn test_interface_call_with_a_cycle_of_embedded_pointers() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "storage/storage.go",
            "package storage\n\ntype Store interface {\n\tSave() error\n}\n\ntype Left struct{ *Right }\n\ntype Right struct{ *Left }\n\nfunc (r *Right) Save() error { return nil }\n",
        ),
        (
            "main.go",
            "package main\n\nimport \"example.com/shop/storage\"\n\nfunc run(store storage.Store) {\n\t_ = store.Save()\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "main.go"),
        ["6 run>store.Save Ambiguous Implementation [storage/storage.go#Right.Save, storage/storage.go#Store.Save]"]
    );
}

/// A local name that an assignment gives a new value keeps the type of its
/// declaration. A Go assignment cannot change the type of a variable.
#[test]
fn test_reassigned_local_keeps_its_declared_type() {
    let files = [
        ("go.mod", "module example.com/shop\n"),
        (
            "git/repo.go",
            "package git\n\ntype Repository struct{}\n\nfunc (r *Repository) GetCommit() {}\n\nfunc Open() (*Repository, error) { return nil, nil }\n",
        ),
        (
            "temp/temp.go",
            "package temp\n\ntype Upload struct{}\n\nfunc (u *Upload) GetCommit() {}\n",
        ),
        (
            "context/context.go",
            "package context\n\nimport \"example.com/shop/git\"\n\ntype Repository struct {\n\tGitRepo *git.Repository\n}\n\ntype Context struct {\n\tRepo *Repository\n}\n",
        ),
        (
            "main.go",
            "package main\n\nimport (\n\t\"example.com/shop/context\"\n\t\"example.com/shop/git\"\n)\n\nfunc diff(ctx *context.Context, wiki bool) {\n\tgitRepo := ctx.Repo.GitRepo // keep\n\tif wiki {\n\t\tvar err error\n\t\tgitRepo, err = git.Open()\n\t\t_ = err\n\t}\n\tgitRepo.GetCommit()\n}\n",
        ),
    ];
    assert_eq!(
        rows_of(&files, "main.go", "GetCommit"),
        ["15 diff>gitRepo.GetCommit UniqueCandidate ReceiverDeclaredType [git/repo.go#Repository.GetCommit]"]
    );
}

/// The resolution time of `calls` calls through an interface that `types`
/// types implement. Each type embeds the type before it, thus a chain of
/// `types` embedded types promotes the methods of the first type.
fn interface_calls_resolve_at(types: usize, calls: usize, chain: bool) -> std::time::Duration {
    let mut storage = String::from(
        "package storage\n\ntype Store interface {\n\tSave() error\n\tClose() error\n}\n",
    );
    for position in 0..types {
        if chain && position > 0 {
            storage.push_str(&format!(
                "\ntype T{position} struct{{ *T{} }}\n",
                position - 1
            ));
        } else {
            storage.push_str(&format!(
                "\ntype T{position} struct{{}}\n\nfunc (t *T{position}) Save() error {{ return nil }}\n\nfunc (t *T{position}) Close() error {{ return nil }}\n"
            ));
        }
    }
    let mut main = String::from("package main\n\nimport \"example.com/shop/storage\"\n");
    for position in 0..calls {
        main.push_str(&format!(
            "\nfunc run{position}(store storage.Store) {{\n\t_ = store.Save()\n}}\n"
        ));
    }
    let files = [
        ("go.mod", "module example.com/shop\n"),
        ("storage/storage.go", storage.as_str()),
        ("main.go", main.as_str()),
    ];
    let started = crate::index::timing::Stopwatch::start();
    let found = rows(&files, "main.go");
    let elapsed = started.elapsed();
    assert_eq!(found.len(), calls);
    elapsed
}

#[test]
#[ignore = "timing: run in release"]
fn timing_interface_calls_resolve_in_linear_time() {
    use crate::index::timing::assert_scales_linearly;
    assert_scales_linearly("calls", 8_000, |size| {
        interface_calls_resolve_at(10, size, false)
    });
    assert_scales_linearly("types", 8_000, |size| {
        interface_calls_resolve_at(size, 10, false)
    });
    assert_scales_linearly("embedded chain", 16_000, |size| {
        interface_calls_resolve_at(size, 10, true)
    });
}

/// The resolution time of a call of each method of an interface with
/// `methods` methods.
fn calls_of_each_interface_method_resolve_at(methods: usize) -> std::time::Duration {
    let mut storage = String::from("package storage\n\ntype Store interface {\n");
    for method in 0..methods {
        storage.push_str(&format!("\tM{method}() error\n"));
    }
    storage.push_str("}\n\nfunc run(store Store) {\n");
    for method in 0..methods {
        storage.push_str(&format!("\t_ = store.M{method}()\n"));
    }
    storage.push_str("}\n");
    let files = [
        ("go.mod", "module example.com/shop\n"),
        ("storage/storage.go", storage.as_str()),
    ];
    let started = crate::index::timing::Stopwatch::start();
    let found = rows(&files, "storage/storage.go");
    let elapsed = started.elapsed();
    assert_eq!(found.len(), methods);
    elapsed
}

#[test]
#[ignore = "timing: run in release"]
fn timing_calls_of_many_interface_methods_resolve_in_linear_time() {
    crate::index::timing::assert_scales_linearly(
        "methods",
        16_000,
        calls_of_each_interface_method_resolve_at,
    );
}

/// The resolution time of `files` files of one package, each with a call of
/// a builtin and a call of a function of a different file of the package.
fn package_calls_resolve_at(files: usize) -> std::time::Duration {
    let sources: Vec<(String, String)> = (0..files)
        .map(|position| {
            (
                format!("shop/f{position}.go"),
                format!(
                    "package shop\n\nfunc f{position}(values []int) int {{\n\treturn len(values) + f{}(values)\n}}\n",
                    (position + 1) % files
                ),
            )
        })
        .collect();
    let mut inputs: Vec<(&str, &str)> = vec![("go.mod", "module example.com/shop\n")];
    inputs.extend(
        sources
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str())),
    );
    let started = crate::index::timing::Stopwatch::start();
    let found = rows(&inputs, "shop/f0.go");
    let elapsed = started.elapsed();
    assert_eq!(
        found,
        [
            "4 f0>len External - []",
            "4 f0>f1 UniqueCandidate SamePackage [shop/f1.go#f1]"
        ]
    );
    elapsed
}

#[test]
#[ignore = "timing: run in release"]
fn timing_builtin_calls_in_a_large_package_resolve_in_linear_time() {
    crate::index::timing::assert_scales_linearly("files", 8_000, package_calls_resolve_at);
}
