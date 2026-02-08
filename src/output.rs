use crate::providers::{CheckResult, StateItem};
use indicatif::ProgressBar;
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
    println!("{}", c!(text, bold));
}

pub fn print_check_result(item: &StateItem, result: &CheckResult) {
    match result {
        CheckResult::Satisfied => {
            println!(
                "  {} {} {}",
                c!("✓", green),
                c!(item.kind, dimmed),
                c!(item.key, white)
            );
        }
        CheckResult::Missing { detail } => {
            println!(
                "  {} {} {} {}",
                c!("✗", red),
                c!(item.kind, dimmed),
                c!(item.key, white),
                c!(format!("({})", detail), dimmed)
            );
        }
    }
}

pub fn print_plan_item(item: &StateItem) {
    println!(
        "  {} {} {}",
        c!("•", blue),
        c!(item.kind, dimmed),
        c!(item.key, white)
    );
}

pub fn print_apply_done(item: &StateItem) {
    println!(
        "  {} {} {}",
        c!("✓", green),
        c!(item.kind, dimmed),
        c!(item.key, white)
    );
}

pub fn print_apply_skip(item: &StateItem) {
    println!(
        "  {} {} {} {}",
        c!("•", dimmed),
        c!(item.kind, dimmed),
        c!(item.key, dimmed),
        c!("(ok)", dimmed)
    );
}

pub fn print_skip_run_if(item: &StateItem) {
    println!(
        "  {} {} {} {}",
        c!("•", dimmed),
        c!(item.kind, dimmed),
        c!(item.key, dimmed),
        c!("(skipped)", dimmed)
    );
}

pub fn print_apply_fail(item: &StateItem, err: &str) {
    println!(
        "  {} {} {} {}",
        c!("✗", red),
        c!(item.kind, dimmed),
        c!(item.key, white),
        c!(format!("({})", err), red)
    );
}

pub fn print_summary(total: usize, changed: usize, failed: usize, issues: usize, elapsed: Duration) {
    println!();
    let timing = format!("({})", format_duration(elapsed));
    let issues_part = if issues > 0 {
        format!(", {} issues", c!(issues.to_string(), yellow))
    } else {
        String::new()
    };
    if failed > 0 {
        println!(
            "{} {} total, {} changed, {} failed{} {}",
            c!("✗", red),
            total,
            c!(changed.to_string(), green),
            c!(failed.to_string(), red),
            issues_part,
            c!(timing, dimmed)
        );
    } else if changed > 0 || issues > 0 {
        let icon = if issues > 0 { format!("{}", c!("→", yellow)) } else { format!("{}", c!("✓", green)) };
        println!(
            "{} {} total, {} changed{} {}",
            icon,
            total,
            c!(changed.to_string(), green),
            issues_part,
            c!(timing, dimmed)
        );
    } else {
        println!(
            "{} {} total, {} up to date {}",
            c!("✓", green),
            total,
            c!("all", green),
            c!(timing, dimmed)
        );
    }
}

pub fn print_check_summary(total: usize, satisfied: usize, missing: usize, elapsed: Duration) {
    println!();
    let timing = format!("({})", format_duration(elapsed));
    if missing > 0 {
        println!(
            "{} {} total, {} ok, {} missing {}",
            c!("→", yellow),
            total,
            c!(satisfied.to_string(), green),
            c!(missing.to_string(), yellow),
            c!(timing, dimmed)
        );
    } else {
        println!(
            "{} {} total, {} up to date {}",
            c!("✓", green),
            total,
            c!("all", green),
            c!(timing, dimmed)
        );
    }
}

pub fn print_plan_summary(total: usize) {
    println!();
    println!("{} {} items", c!("•", blue), total);
}

pub fn print_resolving_requirements(count: usize) {
    println!(
        "  {} resolving {} requirement{}...",
        c!("→", yellow),
        count,
        if count == 1 { "" } else { "s" }
    );
}

pub fn start_spinner(item: &StateItem) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("  {spinner:.cyan} {prefix} {msg}")
            .unwrap(),
    );
    pb.set_prefix(format!("{} {}", c!(item.kind, dimmed), c!(item.key, white)));
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

pub fn update_spinner(pb: &ProgressBar, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let prefix_len = pb.prefix().len();
    let width = console::Term::stdout().size().1 as usize;
    // 6 = indent(2) + spinner(1) + spaces(3)
    let available = width.saturating_sub(6 + prefix_len + 3);
    let truncated = if line.len() > available {
        &line[..available]
    } else {
        line
    };
    pb.set_message(format!("{} {}", c!("›", dimmed), c!(truncated, dimmed)));
}

pub fn finish_spinner_done(pb: &ProgressBar, item: &StateItem) {
    pb.finish_and_clear();
    print_apply_done(item);
}

pub fn finish_spinner_fail(pb: &ProgressBar, item: &StateItem, err: &str) {
    pb.finish_and_clear();
    print_apply_fail(item, err);
}
