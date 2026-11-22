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
}

impl Framework {
    pub const NAMES: &'static str =
        "pytest, unittest, jest, vitest, node, playwright, testing-library, go, rust, junit";

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
        Framework::Junit => junit(model, spec_path),
    }
}

fn python(model: &Model, spec_path: &str, framework: Framework) -> String {
    let mut out = String::new();
    if framework == Framework::Unittest {
        out.push_str("import unittest\n\n");
    }
    let _ = writeln!(out, "import zin\n\n");
    let _ = writeln!(out, "@zin.verify(\"{spec_path}\")");
    let _ = writeln!(out, "class {}Conformance:", pascal(&model.name));
    out.push_str(
        "    # Exception types that mean \"the implementation refused this action\".\n    rejects = (ValueError,)\n\n    def setup(self):\n        # Runs before every generated test: build a fresh implementation.\n        self.impl = None\n",
    );
    for action in &model.actions {
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

fn javascript(model: &Model, spec_path: &str, framework: Framework) -> String {
    let mut out = String::new();
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
    let _ = writeln!(
        out,
        "verify(new URL(\"./{spec_path}\", import.meta.url), () => {{"
    );
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
        } else {
            format!("{{ {} }}", arguments.join(", "))
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
    let mut out = String::from(
        "import { test } from \"@playwright/test\";\nimport { verify, click } from \"zin-verify/playwright\";\n\n",
    );
    let _ = writeln!(
        out,
        "verify(new URL(\"./{spec_path}\", import.meta.url), async ({{ page }}) => {{"
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
    let mut out = String::from(
        "// TypeScript with JSX (.test.tsx), for Vitest with the jsdom environment.\nimport { join } from \"node:path\";\nimport { cleanup, fireEvent, render, screen } from \"@testing-library/react\";\nimport { describe, test } from \"vitest\";\nimport { Rejected, verify } from \"zin-verify\";\n\n/** Clicks as a user would: a disabled control refuses the action. */\nfunction press(name: string) {\n  const button = screen.getByRole<HTMLButtonElement>(\"button\", { name });\n  if (button.disabled) throw new Rejected();\n  fireEvent.click(button);\n}\n\n",
    );
    let _ = writeln!(
        out,
        "verify(join(import.meta.dirname, \"{spec_path}\"), () => {{"
    );
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
        "\t}}\n\treturn fmt.Errorf(\"unknown action %s\", action)\n}}\n\n// Observe returns any subset of the specification's state: {}\nfunc (a *adapter) Observe() map[string]any {{\n\treturn nil\n}}\n\nfunc TestConformance(t *testing.T) {{\n\tzin.Verify(t, \"{spec_path}\", func() zin.Adapter {{ return &adapter{{}} }})\n}}",
        field_names(model)
    );
    out
}

fn rust(model: &Model, spec_path: &str) -> String {
    let mut out = String::from(
        "use zin::conform::{Adapter, Args, Outcome, assert_conforms_file};\n\nstruct Harness {\n    // the implementation under test\n}\n\nimpl Adapter for Harness {\n    fn call(&mut self, action: &str, args: &Args) -> Outcome {\n        match action {\n",
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
        "            other => panic!(\"unknown action {{other}}\"),\n        }}\n    }}\n\n    // Optionally expose any subset of the specification's state: {}\n    // fn observe(&self) -> Option<zin::State> {{ None }}\n}}\n\n#[test]\nfn conforms_to_specification() {{\n    // The specification and any files it includes, beside this file in tests/.\n    assert_conforms_file(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/tests/{spec_path}\"), || Harness {{}});\n}}",
        field_names(model)
    );
    out
}

fn junit(model: &Model, spec_path: &str) -> String {
    let class = format!("Zin{}Test", pascal(&model.name));
    let mut out = format!(
        "import io.github.enekos.zin.Zin;\nimport org.junit.jupiter.api.DynamicTest;\nimport org.junit.jupiter.api.TestFactory;\n\nimport java.io.IOException;\nimport java.util.Map;\nimport java.util.stream.Stream;\n\n// Put the specification and any committed plan in src/test/resources.\nclass {class} {{\n    static class Harness implements Zin.Adapter {{\n        // the implementation under test\n\n        // Return false when the implementation refuses the action; any exception fails the test.\n        @Override\n        public boolean call(String action, Map<String, Object> args) throws Exception {{\n            switch (action) {{\n"
    );
    for action in &model.actions {
        let _ = writeln!(
            out,
            "                case {} -> throw new UnsupportedOperationException(\"perform {} with \" + args);",
            crate::Json::str(action.name.clone()).to_compact(),
            action.name
        );
    }
    let _ = write!(
        out,
        "                default -> throw new IllegalArgumentException(action);\n            }}\n        }}\n\n        // Return any subset of the specification's state: {}\n        @Override\n        public Map<String, ?> observe() {{\n            return null;\n        }}\n    }}\n\n    @TestFactory\n    Stream<DynamicTest> conformance() throws IOException {{\n        return Zin.load(Zin.resource({class}.class, {})).tests(Harness::new);\n    }}\n}}\n",
        field_names(model),
        crate::Json::str(spec_path).to_compact()
    );
    out
}
