use super::slimmer::SlimmerLevel;
use super::*;
use crate::price_fixture;

fn message(role: &str, text: &str) -> RouterMessage {
    chat_backend::message_text(role, text)
}

fn settings_for(config: &TokenSaverConfig) -> TokenSaverSettings {
    TokenSaverSettings::from_config(config)
}

fn system_text(messages: &[RouterMessage]) -> String {
    messages
        .iter()
        .find(|message| message.role == "system")
        .map(|message| message.content.as_text())
        .unwrap_or_default()
}

#[test]
fn settings_treat_every_saver_as_off_by_default() {
    let settings = settings_for(&TokenSaverConfig::default());
    assert!(settings.slimmer.is_some(), "RTK is on by default");
    assert!(settings.headroom.is_none());
    assert!(settings.output.is_none());
    assert!(settings.ponytail.is_none());
    assert!(!settings.is_idle());
}

#[test]
fn settings_are_idle_when_everything_is_disabled() {
    let config = TokenSaverConfig {
        slimmer_enabled: false,
        ..TokenSaverConfig::default()
    };
    assert!(settings_for(&config).is_idle());
}

#[test]
fn caveman_wins_when_both_output_savers_are_enabled() {
    // Validation rejects this combination, so this only pins what an
    // unvalidated config does rather than leaving it to chance.
    let config = TokenSaverConfig {
        terse_enabled: true,
        caveman_enabled: true,
        caveman_level: "ultra".to_string(),
        ..TokenSaverConfig::default()
    };

    let settings = settings_for(&config);
    assert_eq!(
        settings.output,
        Some(OutputSaver::Caveman(CavemanLevel::Ultra))
    );
}

#[test]
fn unknown_levels_fall_back_to_the_documented_default() {
    let config = TokenSaverConfig {
        slimmer_level: "nonsense".to_string(),
        caveman_level: "nonsense".to_string(),
        ponytail_level: "nonsense".to_string(),
        caveman_enabled: true,
        ponytail_enabled: true,
        ..TokenSaverConfig::default()
    };

    let settings = settings_for(&config);
    assert_eq!(settings.slimmer, Some(SlimmerLevel::Minimal));
    assert_eq!(
        settings.output,
        Some(OutputSaver::Caveman(CavemanLevel::Full))
    );
    assert_eq!(settings.ponytail, Some(PonytailLevel::Full));
}

#[test]
fn headroom_urls_are_normalized() {
    let config = TokenSaverConfig {
        headroom_enabled: true,
        headroom_url: "  http://localhost:8787/  ".to_string(),
        headroom_timeout_ms: 0,
        ..TokenSaverConfig::default()
    };

    let settings = settings_for(&config);
    let headroom = settings.headroom.expect("enabled");
    assert_eq!(headroom.url, "http://localhost:8787");
    assert_eq!(headroom.timeout_ms, 1, "a zero timeout would never connect");
}

#[tokio::test]
async fn an_idle_pipeline_returns_the_input_untouched() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("user", "hello")];

    let (out, savings) = apply(&settings, messages.clone(), "gpt-4o").await;
    assert_eq!(out.len(), messages.len());
    assert!(savings.is_empty());
}

#[tokio::test]
async fn a_directive_creates_a_system_message_when_there_is_none() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        terse_enabled: true,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("user", "explain this")];

    let (out, savings) = apply(&settings, messages, "gpt-4o").await;
    assert_eq!(out[0].role, "system");
    assert!(system_text(&out).contains("Respond concisely"));
    assert_eq!(out.len(), 2, "the user message must survive");
    assert_eq!(savings.output, Some(OutputSaver::Terse));
}

#[tokio::test]
async fn a_directive_appends_to_an_existing_system_message() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        terse_enabled: true,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("system", "You are helpful."), message("user", "hi")];

    let (out, _) = apply(&settings, messages, "gpt-4o").await;
    let system = system_text(&out);
    assert!(system.starts_with("You are helpful."));
    assert!(system.contains("Respond concisely"));
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn directives_are_not_injected_twice() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        terse_enabled: true,
        ponytail_enabled: true,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("user", "hi")];

    let (once, _) = apply(&settings, messages, "gpt-4o").await;
    let (twice, _) = apply(&settings, once.clone(), "gpt-4o").await;

    assert_eq!(once, twice, "a retry must not stack a second directive");
}

#[tokio::test]
async fn caveman_uses_a_probe_instead_of_a_visible_marker() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        caveman_enabled: true,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("user", "hi")];

    let (out, savings) = apply(&settings, messages, "gpt-4o").await;
    let system = system_text(&out);
    assert!(system.contains("Keep all technical substance exact"));
    assert!(
        !system.contains("<!--"),
        "caveman must not announce itself to agentic tools"
    );
    assert_eq!(
        savings.output,
        Some(OutputSaver::Caveman(CavemanLevel::Full))
    );
}

#[tokio::test]
async fn ponytail_stacks_after_the_terseness_directive() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        caveman_enabled: true,
        ponytail_enabled: true,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("system", "Base."), message("user", "write a parser")];

    let (out, savings) = apply(&settings, messages, "gpt-4o").await;
    let system = system_text(&out);

    let caveman_at = system.find("Keep all technical substance exact").expect("caveman");
    let ponytail_at = system.find("lazy senior developer").expect("ponytail");
    assert!(caveman_at < ponytail_at, "ponytail goes last");
    assert_eq!(savings.ponytail, Some(PonytailLevel::Full));
}

#[tokio::test]
async fn the_slimmer_compresses_tool_output_but_not_prose() {
    let settings = settings_for(&TokenSaverConfig::default());

    let mut diff = String::from("diff --git a/x b/x\n@@ -1 +1 @@\n");
    for index in 0..200 {
        diff.push_str(&format!("+line {index}\n"));
    }

    let messages = vec![
        message("system", "You are helpful."),
        message("user", "review this"),
        message("tool", &diff),
        message("assistant", "Sure! I would be happy to help you with that."),
    ];

    let (out, savings) = apply(&settings, messages, "gpt-4o").await;

    assert!(savings.slimmer_tokens > 0, "the diff should shrink");
    assert!(out[2].content.as_text().len() < diff.len());
    // Untouched neighbours.
    assert_eq!(out[0].content.as_text(), "You are helpful.");
    assert_eq!(out[3].content.as_text(), "Sure! I would be happy to help you with that.");
}

#[tokio::test]
async fn the_slimmer_leaves_short_and_error_results_alone() {
    let settings = settings_for(&TokenSaverConfig::default());
    let short = "ok";
    let trace = format!(
        "Error: something broke\n{}",
        "+ filler line\n".repeat(400)
    );
    let messages = vec![message("tool", short), message("tool", &trace)];

    let (out, savings) = apply(&settings, messages, "gpt-4o").await;
    assert_eq!(out[0].content.as_text(), short);
    assert_eq!(out[1].content.as_text(), trace, "error traces stay verbatim");
    assert_eq!(savings.slimmer_tokens, 0);
}

#[tokio::test]
async fn an_unreachable_headroom_fails_open() {
    let settings = settings_for(&TokenSaverConfig {
        slimmer_enabled: false,
        headroom_enabled: true,
        // Port 1 on loopback: nothing listens, and the attempt fails fast.
        headroom_url: "http://127.0.0.1:1".to_string(),
        headroom_timeout_ms: 500,
        ..TokenSaverConfig::default()
    });
    let messages = vec![message("user", "hello")];

    let (out, savings) = apply(&settings, messages.clone(), "gpt-4o").await;

    assert_eq!(out, messages, "the request must sail through untouched");
    assert_eq!(savings.headroom_tokens, 0);
    assert_eq!(savings.notes.len(), 1);
    assert!(savings.notes[0].contains("headroom"));
}

#[test]
fn finalize_splits_input_and_output_savings() {
    let savings = Savings {
        slimmer_tokens: 1_000,
        headroom_tokens: 0,
        output: Some(OutputSaver::Caveman(CavemanLevel::Full)),
        ponytail: None,
        notes: Vec::new(),
    };

    let totals = savings.finalize(500, Some(price_fixture(3.0, 15.0)));

    assert_eq!(totals.saved_rtk_tokens, 1_000);
    assert_eq!(totals.saved_caveman_tokens, 300, "60% of 500");
    assert_eq!(totals.total_tokens(), 1_300);
    // 1000 input at $3/M plus 300 output at $15/M.
    assert!((totals.saved_cost_usd - (1_000.0 * 3.0 + 300.0 * 15.0) / 1_000_000.0).abs() < 1e-9);
}

#[test]
fn ponytail_takes_its_share_of_what_caveman_left() {
    // Caveman (60%) runs first, leaving 40 tokens; ponytail (25%) takes 10 of
    // those rather than 25 of the original 100.
    let savings = Savings {
        output: Some(OutputSaver::Caveman(CavemanLevel::Full)),
        ponytail: Some(PonytailLevel::Full),
        ..Savings::default()
    };

    let totals = savings.finalize(100, None);
    assert_eq!(totals.saved_caveman_tokens, 60);
    assert_eq!(totals.saved_ponytail_tokens, 10);
}

#[test]
fn savings_without_a_price_still_count_tokens() {
    let savings = Savings {
        slimmer_tokens: 500,
        ..Savings::default()
    };

    let totals = savings.finalize(0, None);
    assert_eq!(totals.total_tokens(), 500);
    assert_eq!(totals.saved_cost_usd, 0.0);
}

#[test]
fn describe_lists_the_contributing_savers() {
    let totals = SavingsTotals {
        saved_rtk_tokens: 900,
        saved_caveman_tokens: 100,
        saved_cost_usd: 0.0042,
        ..SavingsTotals::default()
    };

    let line = totals.describe().expect("a description");
    assert!(line.contains("900 prompt"), "{line}");
    assert!(line.contains("100 output (est.)"), "{line}");
    assert!(line.contains("rtk, caveman"), "{line}");

    assert!(SavingsTotals::default().describe().is_none());
}

#[test]
fn contributions_are_ordered_by_tokens_saved() {
    let totals = SavingsTotals {
        saved_rtk_tokens: 10,
        saved_ponytail_tokens: 300,
        saved_headroom_tokens: 100,
        ..SavingsTotals::default()
    };

    let contributions = totals.contributions();
    assert_eq!(
        contributions,
        vec![
            (Saver::Ponytail, 300),
            (Saver::Headroom, 100),
            (Saver::Slimmer, 10)
        ]
    );
}

#[test]
fn sides_report_whether_their_numbers_are_estimates() {
    assert!(!SaverSide::Input.is_estimated());
    assert!(SaverSide::Output.is_estimated());
}
