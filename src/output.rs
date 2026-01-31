use crate::providers::{CheckResult, StateItem};
use owo_colors::OwoColorize;
use std::time::Duration;

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{}s", secs)
    } else {
        format!("{}ms", d.as_millis())
    }
}

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

pub fn print_summary(total: usize, changed: usize, failed: usize, elapsed: Duration) {
    println!();
    let timing = format!("({})", format_duration(elapsed));
    if failed > 0 {
        println!(
            "{} {} total, {} changed, {} failed {}",
            "✗".red(),
            total,
            changed.to_string().green(),
            failed.to_string().red(),
            timing.dimmed()
        );
    } else if changed > 0 {
        println!(
            "{} {} total, {} changed {}",
            "✓".green(),
            total,
            changed.to_string().green(),
            timing.dimmed()
        );
    } else {
        println!(
            "{} {} total, {} up to date {}",
            "✓".green(),
            total,
            "all".green(),
            timing.dimmed()
        );
    }
}

pub fn print_check_summary(total: usize, satisfied: usize, missing: usize, elapsed: Duration) {
    println!();
    let timing = format!("({})", format_duration(elapsed));
    if missing > 0 {
        println!(
            "{} {} total, {} ok, {} missing {}",
            "→".yellow(),
            total,
            satisfied.to_string().green(),
            missing.to_string().yellow(),
            timing.dimmed()
        );
    } else {
        println!(
            "{} {} total, {} up to date {}",
            "✓".green(),
            total,
            "all".green(),
            timing.dimmed()
        );
    }
}

pub fn print_plan_summary(total: usize) {
    println!();
    println!("{} {} items", "•".blue(), total);
}

pub fn print_resolving_requirements(count: usize) {
    println!(
        "  {} resolving {} requirement{}...",
        "→".yellow(),
        count,
        if count == 1 { "" } else { "s" }
    );
}
