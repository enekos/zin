//! Generated models: symmetry reduction must reach the same verdict as full
//! exploration, and every counterexample it reports must replay concretely.

use zin::{ErrorKind, Options, Trace, Value, check_trace, parse, verify, verify_with};

const MODELS: u64 = 2_000;

struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % bound as u64).unwrap()
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

struct Generator {
    rng: Rng,
    fresh: usize,
}

impl Generator {
    /// An integer expression; `vars` are bound workers.
    fn int(&mut self, depth: usize, vars: &[String]) -> String {
        let leaf = depth == 0 || self.rng.below(3) == 0;
        if leaf {
            return match self.rng.below(if vars.is_empty() { 2 } else { 4 }) {
                0 => self.rng.below(3).to_string(),
                1 => "total".into(),
                _ => format!("load of {}", vars[self.rng.below(vars.len())]),
            };
        }
        let (a, b) = (self.int(depth - 1, vars), self.int(depth - 1, vars));
        match self.rng.below(7) {
            0 => format!("({a} + {b})"),
            1 => format!("({a} - {b})"),
            2 => format!("({a} * {b})"),
            // A divisor that is zero for some workers and not others.
            3 => format!("({a} / ({} - {b}))", 1 + self.rng.below(2)),
            4 => format!("({a} / {b})"),
            5 => format!("(table of {a})"),
            _ => {
                let v = self.var();
                let mut inner = vars.to_vec();
                inner.push(v.clone());
                let body = self.boolean(depth - 1, &inner);
                format!("(number of {v} in worker where {body})")
            }
        }
    }

    fn boolean(&mut self, depth: usize, vars: &[String]) -> String {
        let leaf = depth == 0 || self.rng.below(4) == 0;
        if leaf || self.rng.below(3) == 0 {
            let (a, b) = (
                self.int(depth.saturating_sub(1), vars),
                self.int(depth.saturating_sub(1), vars),
            );
            return match self.rng.below(if vars.is_empty() { 3 } else { 4 }) {
                0 => format!("({a} is {b})"),
                1 => format!("({a} < {b})"),
                2 => format!("({a} is at most {b})"),
                _ => format!("(holder is {})", vars[self.rng.below(vars.len())]),
            };
        }
        match self.rng.below(5) {
            0 => format!(
                "({} and {})",
                self.boolean(depth - 1, vars),
                self.boolean(depth - 1, vars)
            ),
            1 => format!(
                "({} or {})",
                self.boolean(depth - 1, vars),
                self.boolean(depth - 1, vars)
            ),
            2 => format!("(not {})", self.boolean(depth - 1, vars)),
            quantifier => {
                let v = self.var();
                let mut inner = vars.to_vec();
                inner.push(v.clone());
                let body = self.boolean(depth - 1, &inner);
                let word = if quantifier == 3 { "some" } else { "every" };
                format!("({word} {v} in worker satisfies {body})")
            }
        }
    }

    fn var(&mut self) -> String {
        self.fresh += 1;
        format!("v{}", self.fresh)
    }

    fn model(&mut self, seed: u64) -> String {
        let mut workers = vec!["ann", "ben", "cid"];
        workers.truncate(2 + self.rng.below(2));
        self.rng.shuffle(&mut workers);
        let mut holders = workers.clone();
        holders.push("nobody");
        self.rng.shuffle(&mut holders);
        let w = ["w".to_string()];
        // Half the models have total actions and a quantified property, so
        // only the property can fault and reduction must not hide that fault.
        let (property, guard, value, total) = if self.rng.below(2) == 0 {
            let v = self.var();
            let vars = std::slice::from_ref(&v);
            let body = if self.rng.below(2) == 0 {
                self.boolean(2, vars)
            } else {
                format!("({} is {})", self.int(2, vars), self.rng.below(3))
            };
            let word = ["some", "every"][self.rng.below(2)];
            (
                format!("{word} {v} in worker satisfies {body}"),
                // Reaching only asymmetric states stops a later symmetric
                // state from exposing the same fault for every worker.
                [
                    "load of w is 0",
                    "every u in worker satisfies load of u is 0",
                ][self.rng.below(2)]
                .into(),
                "1".into(),
                "0".into(),
            )
        } else {
            (
                self.boolean(3, &[]),
                self.boolean(2, &w),
                self.int(2, &w),
                self.int(2, &[]),
            )
        };
        let word = ["always", "never"][self.rng.below(2)];
        format!(
            "system \"Generated {seed}\"\n\
             types\n  interchangeable worker is one of {}\n  small is an integer from 0 to 1\n\
             state\n  load is a map from worker to an integer from 0 to 2\n  \
             table is a map from small to an integer from 0 to 2\n  \
             total is an integer from 0 to 3\n  holder is one of {}\n\
             initially\n  load of every worker is 0\n  table of every small is {}\n  \
             total is 0\n  holder is nobody\n\
             {word} {property}\n\
             action bump(w is a worker)\n  requires {guard}\n  load of w becomes {value}\n\
             action take(w is a worker)\n  requires holder is nobody\n  holder becomes w\n\
             action release\n  requires holder is not nobody\n  holder becomes nobody\n  total becomes {total}\n",
            workers.join(", "),
            holders.join(", "),
            self.rng.below(3),
        )
    }
}

fn json(value: &Value) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Symbol(s) => format!("\"{s}\""),
        Value::Nothing => "null".to_string(),
        Value::List(items) => format!("[{}]", items.iter().map(json).collect::<Vec<_>>().join(",")),
        Value::Tagged { tag, members } => {
            format!("{{\"{tag}\":{}}}", json(&Value::Map(members.clone())))
        }
        Value::Map(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(k, v)| format!("\"{k}\":{}", json(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// `bump(ann)` as a recorded event that expects `state` afterwards.
fn event(call: &str, state: Option<&zin::State>) -> String {
    let (action, args) = call.split_once('(').unwrap_or((call, ")"));
    let args: Vec<String> = args
        .trim_end_matches(')')
        .split(", ")
        .filter(|a| !a.is_empty())
        .map(|a| format!("\"{a}\""))
        .collect();
    let mut line = format!("{{\"action\":\"{action}\",\"args\":[{}]", args.join(","));
    if let Some(state) = state {
        let fields: Vec<String> = state
            .iter()
            .map(|(k, v)| format!("\"{k}\":{}", json(v)))
            .collect();
        line.push_str(&format!(",\"state\":{{{}}}", fields.join(",")));
    }
    line.push_str("}\n");
    line
}

fn replay(trace: &Trace) -> String {
    trace
        .steps
        .iter()
        .map(|step| event(&step.call, Some(&step.state)))
        .collect()
}

#[test]
fn reduction_agrees_with_full_exploration_on_generated_models() {
    let mut generator = Generator {
        rng: Rng(0x9E37_79B9_7F4A_7C15),
        fresh: 0,
    };
    let (mut reduced_runs, mut faults, mut violations, mut valid) = (0, 0, 0, 0);
    for seed in 0..MODELS {
        let source = generator.model(seed);
        let Ok(spec) = parse(&source) else { continue };
        let full = verify(&spec);
        let reduced = verify_with(&spec, Options::default());
        // A model with several defects may report a different one first, since
        // representatives are explored in a different order. Whether the model
        // fails must agree, and the reported defect must be real.
        let class = |r: &zin::Result<zin::Verification>| match r {
            Ok(_) => None,
            Err(e) if matches!(e.kind, ErrorKind::Violation | ErrorKind::Model) => {
                Some(ErrorKind::Violation)
            }
            Err(e) => Some(e.kind),
        };
        assert_eq!(
            class(&reduced),
            class(&full),
            "verdicts differ\n{source}\nreduced: {reduced:?}\nfull: {full:?}"
        );
        if matches!(class(&full), Some(ErrorKind::Spec)) {
            continue;
        }
        valid += 1;
        let model = zin::Model::compile(&spec).unwrap();
        match reduced {
            Ok(verification) => {
                reduced_runs += usize::from(verification.reduced);
                assert!(verification.graph.state_count() <= full.unwrap().graph.state_count());
            }
            Err(error) => {
                let trace = error
                    .trace
                    .as_ref()
                    .unwrap_or_else(|| panic!("{error}\n{source}"));
                let prefix = replay(trace);
                match error.kind {
                    ErrorKind::Violation => {
                        violations += 1;
                        check_trace(&model, &prefix)
                            .unwrap_or_else(|e| panic!("{e}\n{error}\n{source}"));
                    }
                    ErrorKind::Model => {
                        faults += 1;
                        let call =
                            error
                                .message
                                .split_once(": ")
                                .map(|(call, _)| call)
                                .filter(|call| {
                                    ["bump(", "take(", "release"]
                                        .iter()
                                        .any(|a| call.starts_with(a))
                                });
                        let Some(call) = call else {
                            // The property itself faults in the state the trace reaches.
                            check_trace(&model, &prefix)
                                .unwrap_or_else(|e| panic!("{e}\n{error}\n{source}"));
                            continue;
                        };
                        let events = prefix + &event(call, None);
                        let replayed = check_trace(&model, &events).unwrap_err();
                        assert!(
                            replayed.message.contains("cannot take"),
                            "{replayed}\n{error}\n{source}"
                        );
                    }
                    kind => panic!("unexpected {kind:?}: {error}"),
                }
            }
        }
    }
    assert!(
        valid >= MODELS as usize / 2,
        "only {valid} generated models type-check"
    );
    assert!(
        reduced_runs >= 20,
        "only {reduced_runs} models used reduction"
    );
    assert!(
        faults >= 20 && violations >= 20,
        "{faults} faults, {violations} violations"
    );
}
