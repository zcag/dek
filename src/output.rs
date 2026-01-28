use crate::providers::{CheckResult, StateItem};

pub fn print_check_result(item: &StateItem, result: &CheckResult) {
    let status = match result {
        CheckResult::Satisfied => "✓",
        CheckResult::Missing { .. } => "✗",
    };
    println!("{} {} - {}", status, item, result);
}

pub fn print_apply_start(item: &StateItem) {
    println!("→ Applying {}...", item);
}

pub fn print_apply_done(item: &StateItem) {
    println!("✓ Applied {}", item);
}

pub fn print_apply_skip(item: &StateItem) {
    println!("• Skipped {} (already satisfied)", item);
}

pub fn print_summary(total: usize, changed: usize, failed: usize) {
    println!();
    println!(
        "Summary: {} total, {} changed, {} failed",
        total, changed, failed
    );
}

pub fn print_check_summary(total: usize, satisfied: usize, missing: usize) {
    println!();
    println!(
        "Check: {} total, {} satisfied, {} missing",
        total, satisfied, missing
    );
}
