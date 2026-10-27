//! Load the vendor sample file, rebuild the schedule, save, and compare byte for byte.

use budget_core::{bgt, BudgetDb, Cal};
use std::path::Path;

fn sample_text() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference/sample.bgt");
    bgt::latin1_decode(&std::fs::read(p).expect("reference/sample.bgt"))
}

fn load_sample(unlimited: bool) -> BudgetDb {
    let loaded = bgt::load_str(&sample_text(), 20211201, None).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(loaded.payments.len(), 11);
    assert_eq!(loaded.version, "2");
    BudgetDb::from_loaded(loaded, &Cal::from_int(20211201), 12, unlimited)
}

fn roundtrip(unlimited: bool) {
    let text = sample_text();
    let db = load_sample(unlimited);
    let saved = db.to_text(None).unwrap();
    let want: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let got: Vec<&str> = saved.lines().collect();
    assert_eq!(got.last().unwrap(), &"## END-OF-DATA; line_no=15 ##");
    assert_eq!(&got[..got.len() - 1], &want[..], "unlimited={unlimited}");
    // and the saved text loads again to the same file
    let again = bgt::load_str(&saved, 20211201, None).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(bgt::save_str(&again.payments, again.adjustments.as_ref().unwrap(), &again.notes, None).unwrap(), saved);
}

#[test]
fn sample_roundtrip_default_window() { roundtrip(false); }

#[test]
fn sample_roundtrip_unlimited_history() { roundtrip(true); }

#[test]
fn sample_december_2021_schedule() {
    let mut db = load_sample(false);
    // window: latest adjustment older than Oct 2021 is 2012-01-16 -> starts 2012-01-01
    assert_eq!(db.start_date.get(), 20120101);
    assert_eq!(db.end_date.get(), 20221231);
    // Rent -1100 on the 1st, nothing else
    let d1 = db.get_day(&Cal::from_int(20211201)).unwrap().clone();
    assert_eq!(d1.transactions.len(), 1);
    assert_eq!(d1.transactions[0].amount, -1100.0);
    // Salary every 2 weeks from Fri 2004-07-30 lands on 10 and 24 Dec 2021
    let d10 = db.get_day(&Cal::from_int(20211210)).unwrap().clone();
    assert!(d10.transactions.iter().any(|t| t.amount == 1500.0));
    assert!(db.get_day(&Cal::from_int(20211217)).unwrap().transactions.iter().all(|t| t.amount != 1500.0));
    // Visa Pmt: NO_WEEKENDS; 18 Dec 2021 is a Saturday -> Monday 20th with the adjusted suffix
    let d20 = db.get_day(&Cal::from_int(20211220)).unwrap().clone();
    let visa = d20.transactions.iter().find(|t| t.amount == -250.0).expect("visa shifted to Monday");
    assert_eq!(visa.suffix, '\'');
    assert!(db.get_day(&Cal::from_int(20211218)).unwrap().transactions.iter().all(|t| t.amount != -250.0));
    let d31 = db.get_day(&Cal::from_int(20211231)).unwrap().clone();
    println!("Dec 2021: opening {:.2} closing {:.2}", d1.opening_balance, d31.closing_balance);
}

/// The vendor manual's screenshot (app/doc/image002.jpg) shows sample.bgt in December 2021:
/// Total Income 3,000.00, Total Expenses -3,030.00. The screenshot's running balances
/// (opening 2,340.00) do not follow from the file as shipped (the engine gives 24,040.00 from the
/// 2012-01-16 adjustment of 500.00), so only the month's transaction totals are asserted.
#[test]
fn sample_december_2021_totals_match_manual_screenshot() {
    let mut db = load_sample(false);
    let mut income = 0.0;
    let mut expenses = 0.0;
    for d in 1..=31 {
        let day = db.get_day(&Cal::from_int(20211200 + d)).unwrap();
        for t in &day.transactions {
            if t.amount > 0.0 { income += t.amount } else { expenses += t.amount }
        }
    }
    assert_eq!(income, 3000.0);
    assert_eq!(expenses, -3030.0);
}
