use crate::model::{ActionInfo, Model};
use crate::{Error, Result};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    Pytest,
    Unittest,
    Jest,
    Vitest,
    Node,
    Playwright,
    TestingLibrary,
    Go,
    Rust,
    Junit,
    Rspec,
    Phpunit,
    Xunit,
}

impl Framework {
    pub const NAMES: &'static str = "pytest, unittest, jest, vitest, node, playwright, testing-library, go, rust, junit, rspec, phpunit, xunit";

    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "pytest" => Ok(Self::Pytest),
            "unittest" => Ok(Self::Unittest),
            "jest" => Ok(Self::Jest),
            "vitest" => Ok(Self::Vitest),
            "node" => Ok(Self::Node),
            "playwright" => Ok(Self::Playwright),
            "testing-library" => Ok(Self::TestingLibrary),
            "go" => Ok(Self::Go),
            "rust" => Ok(Self::Rust),
            "junit" => Ok(Self::Junit),
            "rspec" => Ok(Self::Rspec),
            "phpunit" => Ok(Self::Phpunit),
            "xunit" => Ok(Self::Xunit),
            other => Err(Error::usage(format!(
                "unknown framework '{other}'; use one of {}",
                Self::NAMES
            ))),
        }
    }
}

fn identifier(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    if out
        .chars()
        .next()
        .is_none_or(|first| first.is_ascii_digit())
    {
        out.insert(0, '_');
    }
    out
}

fn pascal(name: &str) -> String {
    identifier(name)
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect()
}

fn params(action: &ActionInfo) -> Vec<String> {
    action
        .params
        .iter()
        .map(|param| identifier(&param.name))
        .collect()
}

fn python_alias(action: &ActionInfo) -> bool {
    const KEYWORDS: &[&str] = &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ];
    let name = action.name.as_str();
    KEYWORDS.contains(&name)
        || name.starts_with('_')
        || matches!(
            name,
            "setup" | "teardown" | "observe" | "rejects" | "impl" | "zin_actions"
        )
        || action
            .params
            .iter()
            .any(|param| KEYWORDS.contains(&param.name.as_str()) || param.name == "self")
}

fn javascript_binding(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "arguments",
        "eval",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ];
    !RESERVED.contains(&name)
}

fn field_names(model: &Model) -> String {
    model
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A starting-point conformance test for `framework`, with one stub per action.
#[must_use]
pub fn scaffold(model: &Model, spec_path: &str, framework: Framework) -> String {
    match framework {
        Framework::Pytest | Framework::Unittest => python(model, spec_path, framework),
        Framework::Jest | Framework::Vitest | Framework::Node => {
            javascript(model, spec_path, framework)
        }
        Framework::Playwright => playwright(model, spec_path),
        Framework::TestingLibrary => testing_library(model, spec_path),
        Framework::Go => go(model, spec_path),
        Framework::Rust => rust(model, spec_path),
        Framework::Junit | Framework::Rspec | Framework::Phpunit | Framework::Xunit => {
            native_scaffold(model, spec_path, framework)
        }
    }
}

fn python(model: &Model, spec_path: &str, framework: Framework) -> String {
    let mut out = String::new();
    if framework == Framework::Unittest {
        out.push_str("import unittest\n\n");
    }
    let _ = writeln!(out, "import zin\n\n");
    let path = crate::Json::str(spec_path).to_compact();
    let _ = writeln!(out, "@zin.verify({path})");
    let _ = writeln!(
        out,
        "class {}Conformance:",
        identifier(&pascal(&model.name))
    );
    if model.actions.iter().any(python_alias) {
        out.push_str("    # Explicit handlers receive a dictionary with the original parameter names.\n    zin_actions = {\n");
        for (index, action) in model
            .actions
            .iter()
            .enumerate()
            .filter(|(_, action)| python_alias(action))
        {
            let _ = writeln!(out, "        {:?}: \"_zin_action_{index}\",", action.name);
        }
        out.push_str("    }\n");
    }
    out.push_str(
        "    # Exception types that mean \"the implementation refused this action\".\n    rejects = (ValueError,)\n\n    def setup(self):\n        # Runs before every generated test: build a fresh implementation.\n        self.impl = None\n",
    );
    for (index, action) in model.actions.iter().enumerate() {
        if python_alias(action) {
            let _ = writeln!(
                out,
                "\n    def _zin_action_{index}(self, args):\n        raise NotImplementedError(\"perform {} on self.impl\")",
                action.name
            );
            continue;
        }
        let mut arguments = vec!["self".to_string()];
        arguments.extend(params(action));
        let _ = writeln!(
            out,
            "\n    def {}({}):\n        raise NotImplementedError(\"perform {} on self.impl\")",
            identifier(&action.name),
            arguments.join(", "),
            action.name
        );
    }
    let _ = writeln!(
        out,
        "\n    def observe(self):\n        # Return any subset of the specification's state: {}\n        return {{}}",
        field_names(model)
    );
    if framework == Framework::Unittest {
        out.push_str("\n\nif __name__ == \"__main__\":\n    unittest.main()\n");
    }
    out
}

fn relative_url(path: &str) -> String {
    let path = path
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F")
        .replace('\\', "%5C");
    crate::Json::str(format!("./{path}")).to_compact()
}

fn javascript(model: &Model, spec_path: &str, framework: Framework) -> String {
    let mut out = String::new();
    let url = relative_url(spec_path);
    let hooks = match framework {
        Framework::Vitest => {
            out.push_str("import { describe, test } from \"vitest\";\n");
            ", { describe, test }"
        }
        Framework::Node => {
            out.push_str("import { describe, test } from \"node:test\";\n");
            ", { describe, test }"
        }
        _ => "",
    };
    out.push_str("import { verify } from \"zin-verify\";\n\n");
    let _ = writeln!(out, "verify(new URL({url}, import.meta.url), () => {{");
    out.push_str("  // Runs before every generated test: build a fresh implementation.\n  const impl = null;\n  return {\n    actions: {\n");
    javascript_actions(&mut out, model, false, "on impl");
    let _ = writeln!(
        out,
        "    }},\n    // Errors that mean \"the implementation refused this action\".\n    rejects: (error) => error instanceof RangeError,\n    // Return any subset of the specification's state: {}\n    observe: () => ({{}}),\n  }};\n}}{hooks});",
        field_names(model)
    );
    out
}

/// The `actions` entries of a JavaScript adapter: one failing stub per action.
fn javascript_actions(out: &mut String, model: &Model, asynchronous: bool, hint: &str) {
    let keyword = if asynchronous { "async " } else { "" };
    for action in &model.actions {
        let arguments = params(action);
        let signature = if arguments.is_empty() {
            String::new()
        } else if arguments.iter().all(|arg| javascript_binding(arg)) {
            format!("{{ {} }}", arguments.join(", "))
        } else {
            "args".to_string()
        };
        let _ = writeln!(
            out,
            "      {keyword}{}({signature}) {{\n        throw new Error(\"perform {} {hint}\");\n      }},",
            identifier(&action.name),
            action.name
        );
    }
}

fn playwright(model: &Model, spec_path: &str) -> String {
    let url = relative_url(spec_path);
    let mut out = String::from(
        "import { test } from \"@playwright/test\";\nimport { verify, click } from \"zin-verify/playwright\";\n\n",
    );
    let _ = writeln!(
        out,
        "verify(new URL({url}, import.meta.url), async ({{ page }}) => {{"
    );
    out.push_str(
        "  // Runs before every generated test, and again to replay a failure:\n  // start the application from scratch.\n  await page.goto(\"/\");\n  // For actions that are the server's answer, hold the requests (import gate):\n  //   const server = await gate(page, \"**/api/...\");\n  //   save: () => server.request(() => click(...)),\n  //   server_ok: () => server.fulfill({ status: 200 }),\n  return {\n    actions: {\n      // click(locator) and fill(locator, value) refuse the action when the\n      // control is missing, hidden or disabled.\n",
    );
    javascript_actions(
        &mut out,
        model,
        true,
        "on the page: await click(page.getByRole('button', { name: '...' }))",
    );
    let _ = writeln!(
        out,
        "    }},\n    // Read from the page any subset of the specification's state: {}\n    observe: async () => ({{}}),\n    // teardown: () => server.dispose(),\n  }};\n}}, {{ test }});",
        field_names(model)
    );
    out
}

fn testing_library(model: &Model, spec_path: &str) -> String {
    let path = crate::Json::str(spec_path).to_compact();
    let mut out = String::from(
        "// TypeScript with JSX (.test.tsx), for Vitest with the jsdom environment.\nimport { join } from \"node:path\";\nimport { cleanup, fireEvent, render, screen } from \"@testing-library/react\";\nimport { describe, test } from \"vitest\";\nimport { Rejected, verify } from \"zin-verify\";\n\n/** Clicks as a user would: a disabled control refuses the action. */\nfunction press(name: string) {\n  const button = screen.getByRole<HTMLButtonElement>(\"button\", { name });\n  if (button.disabled) throw new Rejected();\n  fireEvent.click(button);\n}\n\n",
    );
    let _ = writeln!(out, "verify(join(import.meta.dirname, {path}), () => {{");
    out.push_str(
        "  // Runs before every generated test: render a fresh component.\n  // render(<Component />);\n  return {\n    actions: {\n",
    );
    javascript_actions(&mut out, model, false, "on the component: press('...')");
    let _ = writeln!(
        out,
        "    }},\n    // Read from the screen any subset of the specification's state: {}\n    observe: () => ({{}}),\n    teardown: cleanup,\n  }};\n}}, {{ describe, test }});",
        field_names(model)
    );
    out
}

fn go(model: &Model, spec_path: &str) -> String {
    let path = crate::Json::str(spec_path).to_compact();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "package {}_test\n\nimport (\n\t\"fmt\"\n\t\"testing\"\n\n\tzin \"github.com/enekos/zin/bindings/go\"\n)\n",
        identifier(&model.name).to_ascii_lowercase()
    );
    out.push_str("type adapter struct {\n\t// the implementation under test\n}\n\n");
    out.push_str("// Call performs one action. Return zin.ErrRejected (or wrap it) when the\n// implementation refuses the action; any other error fails the test.\nfunc (a *adapter) Call(action string, args zin.Args) error {\n\tswitch action {\n");
    for action in &model.actions {
        let _ = writeln!(out, "\tcase \"{}\":", action.name);
        for param in &action.params {
            let getter = match param.domain {
                crate::model::Domain::Integer { .. } => "Int",
                crate::model::Domain::Boolean => "Bool",
                crate::model::Domain::Enum(_) => "String",
            };
            let _ = writeln!(out, "\t\t_ = args.{getter}(\"{}\")", param.name);
        }
        out.push_str("\t\treturn fmt.Errorf(\"not implemented\")\n");
    }
    let _ = writeln!(
        out,
        "\t}}\n\treturn fmt.Errorf(\"unknown action %s\", action)\n}}\n\n// Observe returns any subset of the specification's state: {}\nfunc (a *adapter) Observe() map[string]any {{\n\treturn nil\n}}\n\nfunc TestConformance(t *testing.T) {{\n\tzin.Verify(t, {path}, func() zin.Adapter {{ return &adapter{{}} }})\n}}",
        field_names(model)
    );
    out
}

fn rust(model: &Model, spec_path: &str) -> String {
    let call = if model.sources.is_empty() {
        format!("zin::conform::assert_conforms(include_str!({spec_path:?}), || Harness {{}});")
    } else {
        format!(
            "let path = std::path::Path::new(file!()).with_file_name({spec_path:?});\n    let spec = zin::parse_file(path).unwrap();\n    zin::conform::check_spec(&spec, &Default::default(), || Harness {{}}).unwrap();"
        )
    };
    let mut out = String::from(
        "use zin::conform::{Adapter, Args, Outcome};\n\nstruct Harness {\n    // the implementation under test\n}\n\nimpl Adapter for Harness {\n    fn call(&mut self, action: &str, args: &Args) -> Outcome {\n        match action {\n",
    );
    for action in &model.actions {
        let _ = writeln!(
            out,
            "            \"{}\" => todo!(\"perform {} with {{args:?}}; return Outcome::Rejected when refused\"),",
            action.name, action.name
        );
    }
    let _ = writeln!(
        out,
        "            other => panic!(\"unknown action {{other}}\"),\n        }}\n    }}\n\n    // Optionally expose any subset of the specification's state: {}\n    // fn observe(&self) -> Option<zin::State> {{ None }}\n}}\n\n#[test]\nfn conforms_to_specification() {{\n    {call}\n}}",
        field_names(model)
    );
    out
}

fn native_scaffold(model: &Model, spec_path: &str, framework: Framework) -> String {
    let (template, case) = match framework {
        Framework::Junit => (
            include_str!("templates/junit.java"),
            "                case \"$NAME\": throw new UnsupportedOperationException(\"implement $NAME\");",
        ),
        Framework::Xunit => (
            include_str!("templates/xunit.cs"),
            "                case \"$NAME\": throw new NotImplementedException(\"implement $NAME\");",
        ),
        Framework::Phpunit => (
            include_str!("templates/phpunit.php"),
            "            '$NAME' => throw new LogicException('implement $NAME'),",
        ),
        Framework::Rspec => (
            include_str!("templates/rspec.rb"),
            "    raise NotImplementedError, 'implement $NAME' if action == '$NAME'",
        ),
        _ => unreachable!("native framework"),
    };
    let mut actions = String::new();
    for action in &model.actions {
        let _ = writeln!(actions, "{}", case.replace("$NAME", &action.name));
    }
    let path = match framework {
        Framework::Phpunit | Framework::Rspec => {
            format!("'{}'", spec_path.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        _ => crate::Json::str(spec_path).to_compact(),
    };
    template
        .replace("$ACTIONS", &actions)
        .replace("$MODEL", &pascal(&model.name))
        .replace("$FIELDS", &field_names(model))
        .replace("$SPEC", &path)
}
