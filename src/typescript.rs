//! TypeScript declarations for the finite values crossing an adapter boundary.
use crate::Json;
use crate::model::{Domain, FieldType, Model};
use std::fmt::Write as _;

fn quoted(name: &str) -> String {
    Json::str(name).to_compact()
}

fn scalar(model: &Model, domain: &Domain) -> String {
    match domain {
        Domain::Integer { .. } => "number".into(),
        Domain::Boolean => "boolean".into(),
        Domain::Enum(ids) => ids
            .iter()
            .map(|id| quoted(model.symbol(i64::from(*id))))
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

pub(crate) fn declarations(model: &Model) -> String {
    let mut out =
        String::from("import type { Awaitable } from \"zin-verify\";\n\nexport type State = {\n");
    for field in &model.fields {
        let ty = match &field.kind {
            FieldType::Scalar(domain) => scalar(model, domain),
            FieldType::Map { keys, value, .. } => {
                let names = keys
                    .raws()
                    .into_iter()
                    .map(|key| quoted(&model.value(keys, key).to_string()))
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("Record<{names}, {}>", scalar(model, value))
            }
        };
        let _ = writeln!(out, "  {}: {ty};", quoted(&field.name));
    }
    out.push_str("};\n\nexport type Actions = {\n");
    for action in &model.actions {
        let params = action
            .params
            .iter()
            .map(|p| format!("{}: {}", quoted(&p.name), scalar(model, &p.domain)))
            .collect::<Vec<_>>()
            .join("; ");
        let _ = writeln!(
            out,
            "  {}: (args: {{ {params} }}) => Awaitable<unknown>;",
            quoted(&action.name)
        );
    }
    out.push_str("};\n\nexport type Adapter = { actions: Actions; observe: () => Awaitable<Partial<State>> };\n");
    out
}
