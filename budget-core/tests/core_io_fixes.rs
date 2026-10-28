//! Regression tests for the core file/config I/O review round.

use budget_core::config::{format_properties, parse_properties, BudgetConfig};
use budget_core::{bgt, BudgetDb, Cal, Payment};

const BODY: [&str; 5] = ["v,2", ":", "=,", "-10.0, A, 20260101, 1m, 5, 20260101, 48,  ", "## END-OF-DATA; line_no=5 ##"];

fn reload(payments: &[Payment], notes: &str, password: Option<&str>) -> bgt::Loaded {
    let adj = Payment::new(20260101);
    let text = bgt::save_str(payments, &adj, notes, password).unwrap();
    bgt::load_str(&text, 20260101, password).unwrap_or_else(|l| panic!("{}\n{text}", l.log))
}

#[test]
fn cr_lf_and_crlf_line_endings_load_the_same_budget() {
    for sep in ["\n", "\r\n", "\r"] {
        let text = BODY.join(sep);
        let l = bgt::load_str(&text, 20261027, None).unwrap_or_else(|l| panic!("{sep:?}: {}", l.log));
        assert_eq!(l.payments.len(), 1, "{sep:?}");
        assert_eq!(l.payments[0].description, "A");
        assert_eq!(l.version, "2", "{sep:?}");
        // a trailing terminator adds nothing
        assert!(bgt::load_str(&format!("{text}{sep}"), 20261027, None).is_ok(), "{sep:?}");
    }
    // mixed terminators keep the line numbering of the END-OF-DATA check
    let mixed = format!("{}\r{}\n{}\r\n{}\r{}", BODY[0], BODY[1], BODY[2], BODY[3], BODY[4]);
    assert_eq!(bgt::load_str(&mixed, 20261027, None).unwrap_or_else(|l| panic!("{}", l.log)).payments.len(), 1);
    let wrong = mixed.replace("line_no=5", "line_no=4");
    assert!(bgt::load_str(&wrong, 20261027, None).is_err());
    assert!(bgt::is_encrypted_text("#db*encrypted\rv,2"));
}

#[test]
fn cr_only_encrypted_file_opens() {
    let mut p = Payment::new(20260101);
    p.description = "Rent".into();
    let adj = Payment::new(20260101);
    let text = bgt::save_str(&[p], &adj, "", Some("pw")).unwrap().replace('\n', "\r");
    let l = bgt::load_str(&text, 20260101, Some("pw")).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(l.payments[0].description, "Rent");
}

#[test]
fn out_of_range_dates_are_clamped_and_logged_without_overflow() {
    for line in [
        "-10.0, A, -2000000000, 1m, 1, 0, 16,  ",
        "-10.0, A, 2147483647, 1m, 1, 2147483647, 16,  ",
        "-10.0, A, 20260101, 1d, 1, 2147483647, 32,  ",
    ] {
        let text = format!("v,2\n:\n=,\n{line}\n## END-OF-DATA; line_no=5 ##");
        let l = bgt::load_str(&text, 20260101, None).unwrap_or_else(|l| panic!("{line}: {}", l.log));
        assert!(l.log.contains("out of range"), "{line}: {}", l.log);
        let p = &l.payments[0];
        assert!((0..=99991231).contains(&p.start_date), "{line}: {}", p.start_date);
        assert!((0..=99991231).contains(&p.end_date), "{line}: {}", p.end_date);
        let _ = BudgetDb::from_loaded(l, &Cal::from_int(20260101), 12, false);
    }
    // a negative option date would overflow the calendar: rejected like any invalid option
    let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  , -9999999:5\n## END-OF-DATA; line_no=5 ##";
    let l = bgt::load_str(text, 20260101, None).unwrap_err();
    assert!(l.log.contains("invalid option"), "{}", l.log);
    // extreme calendar values compare instead of overflowing
    assert!(Cal::from_int(i32::MIN).lt(&Cal::from_int(20260101)));
    assert!(Cal::from_int(i32::MAX).gt(&Cal::from_int(20260101)));
}

#[test]
fn negative_highlight_colour_saves_a_loadable_file() {
    let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20260101, 48,  , 20261101&-5\n## END-OF-DATA; line_no=5 ##";
    let l = bgt::load_str(text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(l.payments[0].get_hilighting(20261101), 0xFFFFFB);
    let mut p = l.payments[0].clone();
    p.hilighting[0].bg_color = -5; // stray value reached the model some other way
    let adj = Payment::new(20260101);
    let saved = bgt::save_str(&[p], &adj, "", None).unwrap();
    assert!(saved.contains("20261101&fffffb"), "{saved}");
    let back = bgt::load_str(&saved, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(back.payments[0].get_hilighting(20261101), 0xFFFFFB);
}

#[test]
fn control_characters_survive_an_encrypted_save() {
    let mut p = Payment::new(20260101);
    p.description = "A\0B\u{1}C\tD".into();
    p.set_note(20260101, Some("x\0y"));
    for pw in [None, Some("pw")] {
        let l = reload(&[p.clone()], "n\0otes", pw);
        assert_eq!(l.payments[0].description, "A\0B\u{1}C\tD", "{pw:?}");
        assert_eq!(l.payments[0].get_note(20260101).as_deref(), Some("x\0y"));
        assert_eq!(l.notes, "n\0otes");
    }
}

#[test]
fn boundary_whitespace_of_text_fields_survives_reload() {
    let mut p = Payment::new(20260101);
    p.description = "Desc".into();
    p.category = Some(" Auto\t".into());
    p.set_note(20260301, Some("  indented\n  second line  "));
    let l = reload(&[p], "  file notes  ", None);
    assert_eq!(l.payments[0].get_note(20260301).as_deref(), Some("  indented\n  second line  "));
    assert_eq!(l.payments[0].category.as_deref(), Some(" Auto\t"));
    assert_eq!(l.notes, "  file notes  ");
    // interior spacing is written as before
    assert_eq!(bgt::encode_description("a  b"), "a  b");
}

#[test]
fn literal_entity_text_roundtrips() {
    for s in ["&#x0041;BC", "A & B", "&", "&#x002c;", "AT&T", "&#xzzzz;"] {
        assert_eq!(bgt::decode_description(&bgt::encode_description(s)), s, "{s}");
    }
    assert_eq!(bgt::encode_description("AT&T"), "AT&T"); // plain ampersands stay byte-identical
    let mut p = Payment::new(20260101);
    p.description = "&#x0041;BC".into();
    assert_eq!(reload(&[p], "", None).payments[0].description, "&#x0041;BC");
}

#[test]
fn config_non_bmp_characters_roundtrip_as_surrogate_pairs() {
    let c = BudgetConfig { last_file_name: Some("C:\\x\\caf\u{e9} \u{1F600}.bgt".into()), ..Default::default() };
    let text = format_properties(&c.to_properties());
    assert!(text.contains("\\uD83D\\uDE00"), "{text}");
    let back = BudgetConfig::from_properties(&parse_properties(&text));
    assert_eq!(back.last_file_name, c.last_file_name);
    // the form the Java app writes
    assert_eq!(parse_properties("k=a\\uD83D\\uDE00"), vec![("k".to_string(), "a\u{1F600}".to_string())]);
    assert_eq!(parse_properties("k=\\uD83Dx"), vec![("k".to_string(), "?x".to_string())]);
}

#[test]
fn config_trailing_continuation_at_eof_keeps_the_entry() {
    assert_eq!(parse_properties("k=\\"), vec![("k".to_string(), String::new())]);
    assert_eq!(parse_properties("a=b\\\n"), vec![("a".to_string(), "b".to_string())]);
    assert_eq!(parse_properties("x=1\nport=1234\\"), vec![("x".to_string(), "1".to_string()), ("port".to_string(), "1234".to_string())]);
    assert_eq!(parse_properties("x=1\\\\"), vec![("x".to_string(), "1\\".to_string())]);
}

#[test]
fn config_cr_only_lines_and_malformed_unicode_escapes() {
    assert_eq!(
        parse_properties("autosave=false\rport=1\r"),
        vec![("autosave".to_string(), "false".to_string()), ("port".to_string(), "1".to_string())]
    );
    assert_eq!(parse_properties("a=1\r\nb=2\r\n").len(), 2);
    let c = BudgetConfig::from_properties(&parse_properties("currencyPrefix=\\u00\nport=1\n"));
    assert!(!c.currency_prefix.contains('\0'), "{:?}", c.currency_prefix);
    assert_eq!(c.currency_prefix, "$");
    assert_eq!(c.port, 1);
    assert_eq!(parse_properties("k=\\u0041"), vec![("k".to_string(), "A".to_string())]);
}
