use std::path::{Path, PathBuf};
use zin::{
    Error, Result, ScenarioResult, Verification, parse, render_markdown, sequence_diagram,
    state_diagram, verify,
};

const USAGE: &str = "\
Zin — behavior you can read, run, and prove

Usage:
  zin check <spec.zin>
  zin report <spec.zin> [--out <review.md>]
  zin diagram <spec.zin> [--scenario <name>]

Commands:
  check     Exhaustively explore the finite model and run every scenario
  report    Write a Markdown review with Mermaid state and sequence diagrams
  diagram   Print a Mermaid state diagram, or one named scenario diagram
";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zin: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        print!("{USAGE}");
        return Ok(());
    };
    if command == "--help" || command == "-h" || command == "help" {
        print!("{USAGE}");
        return Ok(());
    }
    let path = arguments
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| Error::new(format!("missing specification path\n\n{USAGE}")))?;
    let (spec, verification) = load_and_verify(Path::new(path))?;
    match command.as_str() {
        "check" => {
            reject_unknown_options(command, &arguments[2..], &[])?;
            println!("✓ {}", spec.name);
            println!(
                "  {} states · {} transitions · {} promises · {} scenarios · {} terminal states",
                verification.states.len(),
                verification.transitions.len(),
                verification.promises,
                verification.scenarios.len(),
                verification.terminal_states()
            );
            let coverage = verification.action_coverage();
            for action in &spec.actions {
                if coverage.get(&action.name).copied().unwrap_or(0) == 0 {
                    println!("  note: {} is never enabled", action.name);
                }
            }
        }
        "report" => {
            reject_unknown_options(command, &arguments[2..], &["--out"])?;
            let markdown = render_markdown(&spec, &verification);
            if let Some(output) = option_value(&arguments, "--out")? {
                let output = PathBuf::from(output);
                std::fs::write(&output, markdown).map_err(|error| {
                    Error::new(format!("cannot write {}: {error}", output.display()))
                })?;
                println!("wrote {}", output.display());
            } else {
                print!("{markdown}");
            }
        }
        "diagram" => {
            reject_unknown_options(command, &arguments[2..], &["--scenario"])?;
            if let Some(name) = option_value(&arguments, "--scenario")? {
                let scenario = verification
                    .scenarios
                    .iter()
                    .find(|scenario| scenario.name == name)
                    .ok_or_else(|| Error::new(format!("no scenario named '{name}'")))?;
                print!("{}", scenario_output(&spec.name, scenario));
            } else {
                print!("{}", state_diagram(&verification));
            }
        }
        _ => {
            return Err(Error::new(format!(
                "unknown command '{command}'\n\n{USAGE}"
            )));
        }
    }
    Ok(())
}

fn load_and_verify(path: &Path) -> Result<(zin::Spec, Verification)> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let spec = parse(&source).map_err(|error| Error::new(format!("{}:{error}", path.display())))?;
    let verification =
        verify(&spec).map_err(|error| Error::new(format!("{}:{error}", path.display())))?;
    Ok((spec, verification))
}

fn scenario_output(system_name: &str, scenario: &ScenarioResult) -> String {
    sequence_diagram(system_name, scenario)
}

fn option_value<'a>(arguments: &'a [String], name: &str) -> Result<Option<&'a str>> {
    let Some(index) = arguments.iter().position(|argument| argument == name) else {
        return Ok(None);
    };
    let value = arguments
        .get(index + 1)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| Error::new(format!("{name} needs a value")))?;
    if arguments
        .iter()
        .skip(index + 2)
        .any(|argument| argument == name)
    {
        return Err(Error::new(format!("{name} may only be supplied once")));
    }
    Ok(Some(value))
}

fn reject_unknown_options(command: &str, arguments: &[String], allowed: &[&str]) -> Result<()> {
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if allowed.contains(&argument.as_str()) {
            index += 2;
        } else {
            return Err(Error::new(format!("unknown {command} option '{argument}'")));
        }
    }
    Ok(())
}
