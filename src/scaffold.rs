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
    Go,
    Rust,
}

impl Framework {
    pub const NAMES: &'static str = "pytest, unittest, jest, vitest, node, go, rust";

    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "pytest" => Ok(Self::Pytest),
            "unittest" => Ok(Self::Unittest),
            "jest" => Ok(Self::Jest),
            "vitest" => Ok(Self::Vitest),
            "node" => Ok(Self::Node),
            "go" => Ok(Self::Go),
            "rust" => Ok(Self::Rust),
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
        Framework::Go => go(model, spec_path),
        Framework::Rust => rust(model, spec_path),
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
    for action in &model.actions {
        let arguments = params(action);
        let signature = if arguments.is_empty() {
            String::new()
        } else {
            format!("{{ {} }}", arguments.join(", "))
        };
        let _ = writeln!(
            out,
            "      {}({signature}) {{\n        throw new Error(\"perform {} on impl\");\n      }},",
            identifier(&action.name),
            action.name
        );
    }
    let _ = writeln!(
        out,
        "    }},\n    // Errors that mean \"the implementation refused this action\".\n    rejects: (error) => error instanceof RangeError,\n    // Return any subset of the specification's state: {}\n    observe: () => ({{}}),\n  }};\n}}{hooks});",
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
        "use zin::conform::{Adapter, Args, Outcome, assert_conforms};\n\nstruct Harness {\n    // the implementation under test\n}\n\nimpl Adapter for Harness {\n    fn call(&mut self, action: &str, args: &Args) -> Outcome {\n        match action {\n",
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
        "            other => panic!(\"unknown action {{other}}\"),\n        }}\n    }}\n\n    // Optionally expose any subset of the specification's state: {}\n    // fn observe(&self) -> Option<zin::State> {{ None }}\n}}\n\n#[test]\nfn conforms_to_specification() {{\n    assert_conforms(include_str!(\"{spec_path}\"), || Harness {{}});\n}}",
        field_names(model)
    );
    out
}
