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
