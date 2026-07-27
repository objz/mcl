use super::build_command;

#[test]
fn parses_instance_list_subcommand() {
    let matches = build_command()
        .try_get_matches_from(["rmcl", "instance", "list"])
        .expect("command should parse");
    let instance = matches
        .subcommand_matches("instance")
        .expect("instance subcommand should be present");
    assert_eq!(
        instance.subcommand_name(),
        Some("list"),
        "instance list should resolve to the list subcommand"
    );
}
