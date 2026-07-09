//! VL-CLI-RENDER-003: CLI flag smoke for `--no-color` / `--no-fold`.

use clap::Parser;

// Mirror the Agent flag surface used by `src/main.rs` so we can unit-test
// clap parsing without spinning up the full binary entrypoint.

#[derive(Debug, Parser)]
#[command(name = "velaclaw")]
struct TestCli {
    #[command(subcommand)]
    command: TestCommands,
}

#[derive(Debug, clap::Subcommand)]
enum TestCommands {
    Agent {
        #[arg(short, long)]
        message: Option<String>,
        #[arg(long, default_value_t = false)]
        no_color: bool,
        #[arg(long, default_value_t = false)]
        no_fold: bool,
    },
}

#[test]
fn agent_no_color_flag_parses() {
    let cli = TestCli::try_parse_from(["velaclaw", "agent", "--no-color", "-m", "hi"])
        .expect("parse --no-color");
    match cli.command {
        TestCommands::Agent {
            no_color,
            no_fold,
            message,
        } => {
            assert!(no_color);
            assert!(!no_fold);
            assert_eq!(message.as_deref(), Some("hi"));
        }
    }
}

#[test]
fn agent_no_fold_flag_parses() {
    let cli = TestCli::try_parse_from(["velaclaw", "agent", "--no-fold"]).expect("parse --no-fold");
    match cli.command {
        TestCommands::Agent {
            no_color, no_fold, ..
        } => {
            assert!(!no_color);
            assert!(no_fold);
        }
    }
}

#[test]
fn agent_both_flags_parse() {
    let cli = TestCli::try_parse_from(["velaclaw", "agent", "--no-color", "--no-fold", "-m", "x"])
        .expect("parse both flags");
    match cli.command {
        TestCommands::Agent {
            no_color,
            no_fold,
            message,
        } => {
            assert!(no_color);
            assert!(no_fold);
            assert_eq!(message.as_deref(), Some("x"));
        }
    }
}

#[test]
fn agent_flags_default_off() {
    let cli = TestCli::try_parse_from(["velaclaw", "agent"]).expect("parse defaults");
    match cli.command {
        TestCommands::Agent {
            no_color, no_fold, ..
        } => {
            assert!(!no_color);
            assert!(!no_fold);
        }
    }
}
