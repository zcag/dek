use crate::providers::{CheckResult, StateItem};
use owo_colors::OwoColorize;

pub fn print_header(text: &str) {
    println!("{}", text.bold());
}

pub fn print_check_result(item: &StateItem, result: &CheckResult) {
    match result {
        CheckResult::Satisfied => {
            println!(
                "  {} {} {}",
                "✓".green(),
                item.kind.dimmed(),
                item.key.white()
            );
        }
        CheckResult::Missing { detail } => {
            println!(
                "  {} {} {} {}",
                "✗".red(),
                item.kind.dimmed(),
                item.key.white(),
                format!("({})", detail).dimmed()
            );
        }
    }
}

pub fn print_plan_item(item: &StateItem) {
    println!(
        "  {} {} {}",
        "•".blue(),
        item.kind.dimmed(),
        item.key.white()
    );
}

pub fn print_apply_start(item: &StateItem) {
    println!(
        "  {} {} {}",
        "→".yellow(),
        item.kind.dimmed(),
        item.key.white()
    );
}

pub fn print_apply_done(item: &StateItem) {
    println!(
        "  {} {} {}",
        "✓".green(),
        item.kind.dimmed(),
        item.key.white()
    );
}

pub fn print_apply_skip(item: &StateItem) {
    println!(
        "  {} {} {} {}",
        "•".dimmed(),
        item.kind.dimmed(),
        item.key.dimmed(),
        "(ok)".dimmed()
    );
}

pub fn print_apply_fail(item: &StateItem, err: &str) {
    println!(
        "  {} {} {} {}",
        "✗".red(),
        item.kind.dimmed(),
        item.key.white(),
        format!("({})", err).red()
    );
}

pub fn print_summary(total: usize, changed: usize, failed: usize) {
    println!();
    if failed > 0 {
        println!(
            "{} {} total, {} changed, {} failed",
            "✗".red(),
            total,
            changed.to_string().green(),
            failed.to_string().red()
        );
    } else if changed > 0 {
        println!(
            "{} {} total, {} changed",
            "✓".green(),
            total,
            changed.to_string().green()
        );
    } else {
        println!(
            "{} {} total, {} up to date",
            "✓".green(),
            total,
            "all".green()
        );
    }
}

pub fn print_check_summary(total: usize, satisfied: usize, missing: usize) {
    println!();
    if missing > 0 {
        println!(
            "{} {} total, {} ok, {} missing",
            "→".yellow(),
            total,
            satisfied.to_string().green(),
            missing.to_string().yellow()
        );
    } else {
        println!(
            "{} {} total, {} up to date",
            "✓".green(),
            total,
            "all".green()
        );
    }
}

pub fn print_plan_summary(total: usize) {
    println!();
    println!("{} {} items", "•".blue(), total);
}
