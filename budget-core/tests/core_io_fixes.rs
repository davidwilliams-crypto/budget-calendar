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

#[test]
fn malformed_payment_lines_name_the_field_and_keep_the_id_numbering() {
    let lines = [
        "abc, A, 20260101, 1m, 1, 20260101, 48, img",
        "-1.0, Valid1, 20260101, 1m, 1, 20260101, 48, img",
        "-1.0%x, B, 20260101, 1m, 1, 20260101, 48, img",
        "-1.0, C",
        "-1.0, D, 2026x, 1m, 1, 20260101, 48, img",
        "-1.0, Valid2, 20260101, 1m, 1, 20260101, 48, img",
        "-1.0, E, 20260101, 1q, 1, 20260101, 48, img",
        "-1.0, F, 20260101, ;3, 1, 20260101, 48, img",
        "-1.0, G, 20260101, 1m;x, 1, 20260101, 48, img",
        "-1.0, H, 20260101, xm, 1, 20260101, 48, img",
        "-1.0, I, 20260101, 1m, x, 20260101, 48, img",
        "-1.0, J, 20260101, 1m, 1, x, 48, img",
        "-1.0, K, 20260101, 1m, 1, 20260101, x, img",
        "-1.0, L, 20260101, 1m, 1, 20260101, 48",
        "-1.0, M, 20260101, 1m, 1, 20260101, 48, img, 2026",
        ":, 2026",
        "-1.0!;, Valid3, 20260101, 1m, 1, 20260101, 48, img",
    ];
    let text = format!("v,2\n:\n=,\n{}\n## END-OF-DATA; line_no={} ##", lines.join("\n"), lines.len() + 4);
    let l = bgt::load_str(&text, 20260101, None).unwrap_err();
    let ids: Vec<(&str, i32, bool, char)> = l.payments.iter().map(|p| (p.description.as_str(), p.id, p.disabled, p.flag_value)).collect();
    assert_eq!(ids, [("Valid1", 1, false, '^'), ("Valid2", 4, false, '^'), ("Valid3", 14, true, ';')]);
    let errors: Vec<&str> = l.log.lines().filter(|s| s.starts_with('(')).collect();
    assert_eq!(errors, [
        "(4): Invalid data for 'amount'", "(6): Invalid data for 'loan amount'", "(7): Invalid data for 'start_date'",
        "(8): Invalid data for 'start_date'", "(10): Invalid data for 'frequency'", "(11): Invalid data for 'frequency'",
        "(12): Invalid data for 'frequency'", "(13): Invalid data for 'frequency'", "(14): Invalid data for 'n_pays'",
        "(15): Invalid data for 'end_date'", "(16): Invalid data for 'flags'", "(17): Invalid data for 'image_path'",
        "(18): Invalid data for 'opts'", "(19): Invalid data for 'opts'",
    ]);
    for (line, field, message) in [(7, "start_date", "NoSuchElementException"), (10, "frequency", "Invalid frequency type 'q'"),
        (11, "frequency", "empty frequency"), (17, "image_path", "NoSuchElementException")] {
        assert!(l.log.contains(&format!("({line}): Invalid data for '{field}'\nException: {message}\n")), "{line}: {}", l.log);
    }
}

#[test]
fn utf8_byte_order_mark_is_skipped() {
    // Notepad saves a UTF-8 BOM. A file is read as ISO-8859-1 (bytes EF BB BF), while text decoded
    // as UTF-8 carries U+FEFF; both must load as the same budget, plain or encrypted.
    let body = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20260101, 48,  \n## END-OF-DATA; line_no=5 ##";
    let latin1 = bgt::latin1_decode(&[b"\xEF\xBB\xBF".as_slice(), body.as_bytes()].concat());
    for text in [format!("\u{feff}{body}"), latin1] {
        let l = bgt::load_str(&text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
        assert_eq!(l.payments.len(), 1);
        assert_eq!(l.payments[0].description, "A");
        assert_eq!(l.version, "2");
    }
    let mut p = Payment::new(20260101);
    p.description = "Rent".into();
    let enc = bgt::save_str(&[p], &Payment::new(20260101), "", Some("pw")).unwrap();
    let enc = bgt::latin1_decode(&[b"\xEF\xBB\xBF".as_slice(), enc.as_bytes()].concat());
    assert!(bgt::is_encrypted_text(&enc));
    let l = bgt::load_str(&enc, 20260101, Some("pw")).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(l.payments[0].description, "Rent");
}

#[test]
fn option_dates_must_be_real_calendar_days() {
    // 20260230 used to load silently as 2 March, and a space inside an option date is not a date.
    for opt in ["20260230", "20260230:5", "20260230=note", "20261301", "20260001", "20260431", "20230229", "2026030 :5"] {
        let text = format!("v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  , {opt}\n## END-OF-DATA; line_no=5 ##");
        let l = bgt::load_str(&text, 20260101, None).unwrap_err();
        assert!(l.log.contains("Invalid data for 'opts'"), "{opt}: {}", l.log);
        assert!(l.payments.is_empty(), "{opt}");
    }
    // leap days and dates before year 1000 are real days
    for opt in ["20240229", "20000229:5", "00010101:5"] {
        let text = format!("v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  , {opt}\n## END-OF-DATA; line_no=5 ##");
        bgt::load_str(&text, 20260101, None).unwrap_or_else(|l| panic!("{opt}: {}", l.log));
    }
}

#[test]
fn non_finite_amounts_are_refused_before_saving() {
    // The loader accepts only finite amounts, so a save must never write a file it cannot reopen.
    let base = || {
        let mut p = Payment::new(20260101);
        p.description = "A".into();
        p
    };
    let adj = Payment::new(20260101);
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut amount = base();
        amount.amount = v;
        let mut loan = base();
        loan.loan_amount = v;
        let mut custom = base();
        custom.add_custom_amount(20260201, v);
        for p in [amount, loan, custom] {
            assert!(bgt::save_str(&[p.clone()], &adj, "", None).is_err(), "{:?}", p.opt);
            assert!(bgt::save_str(&[p], &adj, "", Some("pw")).is_err());
        }
    }
    let mut bad_adjustment = Payment::new(20260101);
    bad_adjustment.add_custom_amount(20260301, f64::NAN);
    assert!(bgt::save_str(&[base()], &bad_adjustment, "", None).is_err());
    assert!(bgt::save_str(&[base()], &adj, "", None).is_ok());
}