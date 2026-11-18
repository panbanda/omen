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
