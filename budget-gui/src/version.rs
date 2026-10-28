//! One source for the version displayed by the app, copied by users and printed by the CLI.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_ID: &str = env!("BUDGET_BUILD_ID");
pub const BUILD_TARGET: &str = env!("BUDGET_BUILD_TARGET");
pub const BUILD_PROFILE: &str = env!("BUDGET_BUILD_PROFILE");

pub fn full_version() -> String {
    format!("Budget Calendar v{VERSION}\nBuild: {BUILD_ID}\nTarget: {BUILD_TARGET}\nProfile: {BUILD_PROFILE}")
}

pub fn window_title(file: Option<&str>, dirty: bool) -> String {
    let mut title = format!("Budget Calendar v{VERSION}");
    if let Some(file) = file {
        title.push_str(" — ");
        title.push_str(file);
    }
    if dirty {
        title.push_str(" *");
    }
    title
}

pub fn cli_output(args: &[String]) -> Option<String> {
    if args.iter().any(|argument| argument == "--version-json") {
        Some(format!("{{\"app\":\"Budget Calendar\",\"version\":{},\"build_id\":{},\"target\":{},\"profile\":{}}}", json_string(VERSION), json_string(BUILD_ID), json_string(BUILD_TARGET), json_string(BUILD_PROFILE)))
    } else if args
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        Some(full_version())
    } else {
        None
    }
}

pub fn history() -> &'static str {
    include_str!("../../CHANGELOG.md")
}

fn json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' => {
                output.push_str(&format!("\\u{:04x}", u32::from(character)))
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn titles_keep_version_file_and_unsaved_marker_visible() {
        assert_eq!(
            window_title(None, false),
            format!("Budget Calendar v{VERSION}")
        );
        assert_eq!(
            window_title(None, true),
            format!("Budget Calendar v{VERSION} *")
        );
        assert_eq!(
            window_title(Some("Household.bgt"), false),
            format!("Budget Calendar v{VERSION} — Household.bgt")
        );
        assert_eq!(
            window_title(Some("Household.bgt"), true),
            format!("Budget Calendar v{VERSION} — Household.bgt *")
        );
    }

    #[test]
    fn plain_version_aliases_return_copyable_complete_build_info() {
        let expected = format!("Budget Calendar v{VERSION}\nBuild: {BUILD_ID}\nTarget: {BUILD_TARGET}\nProfile: {BUILD_PROFILE}");
        assert_eq!(full_version(), expected);
        assert_eq!(cli_output(&args(&["--version"])), Some(expected.clone()));
        assert_eq!(cli_output(&args(&["-V"])), Some(expected));
    }

    #[test]
    fn json_version_has_machine_readable_identity_even_with_budget_arguments() {
        let output = cli_output(&args(&[
            "missing.bgt",
            "--month",
            "202610",
            "--version-json",
        ]))
        .expect("version flag is handled before opening any budget");
        let expected = format!("{{\"app\":\"Budget Calendar\",\"version\":\"{VERSION}\",\"build_id\":\"{BUILD_ID}\",\"target\":\"{BUILD_TARGET}\",\"profile\":\"{BUILD_PROFILE}\"}}");
        assert_eq!(output, expected);
        assert_eq!(
            cli_output(&args(&["--version", "--version-json"])),
            Some(output)
        );
    }

    #[test]
    fn normal_launch_and_unrecognized_flags_are_left_to_the_gui() {
        for input in [
            &[][..],
            &["Household.bgt"][..],
            &["--version-file"][..],
            &["--dark", "--month", "202610"][..],
        ] {
            assert_eq!(cli_output(&args(input)), None);
        }
    }

    #[test]
    fn json_values_escape_quotes_backslashes_and_control_characters() {
        assert_eq!(
            json_string("a\"b\\c\n\r\t\0\u{0008}\u{001f}é"),
            "\"a\\\"b\\\\c\\n\\r\\t\\u0000\\u0008\\u001fé\""
        );
    }
}
