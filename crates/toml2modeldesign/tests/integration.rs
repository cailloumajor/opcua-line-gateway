use snapbox::utils::current_dir;
use toml2modeldesign::toml2modeldesign;

#[test]
fn snapshot() {
    let input_dir = current_dir!().join("snapshot").join("in");

    let generated =
        toml2modeldesign(&input_dir, "test_ns").expect("generating model design should not fail");

    snapbox::Assert::new()
        .action_env("SNAPSHOTS")
        .eq(generated, snapbox::file!["./snapshot/expected.xml"]);
}
