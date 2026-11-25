//! Generated models over sets, optional values and maps of records:
//! symmetry reduction must reach the same verdict as full exploration, and a
//! reduced counterexample must replay on the real state space. Absent
//! optionals keep filler values that relabeling must not disturb.

use zin::{ErrorKind, Options, Trace, Value, check_trace, parse, verify, verify_with};

const MODELS: u64 = 600;

struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % bound as u64).unwrap()
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

struct Generator {
    rng: Rng,
    fresh: usize,
}

impl Generator {
    fn var(&mut self) -> String {
        self.fresh += 1;
        format!("v{}", self.fresh)
    }

    fn worker(&mut self, vars: &[String]) -> String {
        vars[self.rng.below(vars.len())].clone()
    }

    fn int(&mut self, depth: usize, vars: &[String]) -> String {
        let choices = if vars.is_empty() { 3 } else { 5 };
        match self.rng.below(if depth == 0 {
            choices.min(3)
        } else {
            choices + 1
        }) {
            0 => self.rng.below(3).to_string(),
            1 => ["size of crew", "length of log"][self.rng.below(2)].into(),
            2 => "(if spare is nothing then 0 else n of spare)".into(),
            3 if !vars.is_empty() => format!("n of task of {}", self.worker(vars)),
            4 if !vars.is_empty() => format!("(n of task of {} + 1)", self.worker(vars)),
            _ => {
                let v = self.var();
                let mut inner = vars.to_vec();
                inner.push(v.clone());
                let body = self.boolean(depth.saturating_sub(1), &inner);
                format!("(number of {v} in crew where {body})")
            }
        }
    }

    fn atom(&mut self, depth: usize, vars: &[String]) -> String {
        let bound = if vars.is_empty() { 6 } else { 14 };
        match self.rng.below(bound) {
            0 => format!("({} is {})", self.int(depth, vars), self.rng.below(3)),
            1 => "(boss is nothing)".into(),
            2 => "(crew is [])".into(),
            3 => "(spare is nothing)".into(),
            5 => "(log is [])".into(),
            // Unguarded: faults when the spare is nothing.
            4 => format!("(n of spare is {})", self.rng.below(3)),
            11 => format!("(crew has {})", self.worker(vars)),
            12 => format!("(log has {})", self.worker(vars)),
            // Unguarded: faults when the log is empty.
            13 => format!("(first of log is {})", self.worker(vars)),
            6 => format!("(boss is {})", self.worker(vars)),
            7 => format!(
                "(stage of task of {} is {})",
                self.worker(vars),
                self.rng.pick(&["idle", "busy", "done"])
            ),
            8 => format!(
                "(spare is not nothing and who of spare is {})",
                self.worker(vars)
            ),
            9 => format!(
                "(task of {} is {{stage: idle, n: {}}})",
                self.worker(vars),
                self.rng.below(3)
            ),
            _ => format!("({} < {})", self.int(depth, vars), self.int(depth, vars)),
        }
    }

    fn boolean(&mut self, depth: usize, vars: &[String]) -> String {
        if depth == 0 || self.rng.below(3) == 0 {
            return self.atom(depth, vars);
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
                let (word, domain) = if quantifier == 3 {
                    ("some", self.rng.pick(&["worker", "crew", "log"]))
                } else {
                    ("every", self.rng.pick(&["worker", "crew", "log"]))
                };
                format!("({word} {v} in {domain} satisfies {body})")
            }
        }
    }

    fn model(&mut self, seed: u64) -> String {
        let workers = ["ann, ben", "ann, ben, cid"][self.rng.below(2)];
        let w = ["w".to_string()];
        let property = self.boolean(3, &[]);
        let join_guard = self.boolean(1, &w);
        let work_guard = self.boolean(1, &w);
        let work_n = self.int(1, &w);
        let stage = self.rng.pick(&["busy", "done"]);
        let stash_who = self.rng.pick(&["boss", "w", "nothing"]);
        let stash_n = self.int(1, &w);
        let word = self
            .rng
            .pick(&["always", "never", "sometimes", "sometimes"]);
        format!(
            "system \"Compound {seed}\"\n\
             types\n  interchangeable worker is one of {workers}\n  stage is one of idle, busy, done\n\
             state\n  crew is a set of worker\n  boss is a worker or nothing\n  \
             task is a map from worker to a record with (stage is stage, n is an integer from 0 to 2)\n  \
             spare is a record with (who is a worker or nothing, n is an integer from 0 to 2) or nothing\n  \
             log is a list of at most 2 worker\n\
             initially\n  crew is []\n  boss is nothing\n  task of every worker is {{stage: idle, n: 0}}\n  spare is nothing\n  log is []\n\
             {word} {property}\n\
             action join(w is a worker)\n  requires not crew has w and {join_guard}\n  crew becomes crew with w\n\
             action quit(w is a worker)\n  requires crew has w\n  crew becomes crew without w\n  \
             boss becomes if boss is w then nothing else boss\n\
             action promote(w is a worker)\n  requires crew has w and boss is nothing\n  boss becomes w\n\
             action work(w is a worker)\n  requires {work_guard}\n  task of w becomes {{stage: {stage}, n: {work_n}}}\n\
             action stash(w is a worker)\n  requires boss is not nothing and spare is nothing\n  \
             spare becomes {{who: {stash_who}, n: {stash_n}}}\n\
             action drop\n  requires spare is not nothing\n  spare becomes nothing\n\
             action note(w is a worker)\n  requires length of log < 2\n  log becomes log without w with w\n\
             action forget\n  requires log is not []\n  log becomes rest of log\n"
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

fn replay(trace: &Trace) -> String {
    trace
        .steps
        .iter()
        .map(|step| {
            let (action, args) = step.call.split_once('(').unwrap_or((&step.call, ")"));
            let args: Vec<String> = args
                .trim_end_matches(')')
                .split(", ")
                .filter(|a| !a.is_empty())
                .map(|a| format!("\"{a}\""))
                .collect();
            let state: Vec<String> = step
                .state
                .iter()
                .map(|(k, v)| format!("\"{k}\":{}", json(v)))
                .collect();
            format!(
                "{{\"action\":\"{action}\",\"args\":[{}],\"state\":{{{}}}}}\n",
                args.join(","),
                state.join(",")
            )
        })
        .collect()
}

#[test]
fn reduction_agrees_with_full_exploration_on_compound_models() {
    let mut generator = Generator {
        rng: Rng(0x2545_F491_4F6C_DD1D),
        fresh: 0,
    };
    let (mut reduced, mut failing, mut checked) = (0, 0, 0);
    for seed in 0..MODELS {
        let source = generator.model(seed);
        let spec = parse(&source).unwrap_or_else(|e| panic!("{e}\n{source}"));
        let full = verify(&spec);
        let symmetric = verify_with(&spec, Options::default());
        let class = |result: &zin::Result<zin::Verification>| match result {
            Ok(_) => None,
            Err(e) if matches!(e.kind, ErrorKind::Violation | ErrorKind::Model) => {
                Some(ErrorKind::Violation)
            }
            Err(e) => Some(e.kind),
        };
        assert_eq!(
            class(&symmetric),
            class(&full),
            "verdicts differ\n{source}\nreduced: {symmetric:?}\nfull: {full:?}"
        );
        if let Err(error) = &full {
            assert_ne!(error.kind, ErrorKind::Spec, "{error}\n{source}");
        }
        checked += 1;
        match symmetric {
            Ok(verification) => {
                reduced += usize::from(verification.reduced);
                assert!(verification.graph.state_count() <= full.unwrap().graph.state_count());
            }
            Err(error) => {
                failing += 1;
                // An unreachable `sometimes` has no run to show.
                let Some(trace) = error.trace.as_ref() else {
                    assert!(
                        error.message.contains("no reachable state"),
                        "{error}\n{source}"
                    );
                    continue;
                };
                let model = zin::Model::compile(&spec).unwrap();
                let steps = if error.kind == ErrorKind::Model {
                    trace.steps.len().saturating_sub(1)
                } else {
                    trace.steps.len()
                };
                let prefix = Trace {
                    steps: trace.steps[..steps].to_vec(),
                    ..trace.clone()
                };
                check_trace(&model, &replay(&prefix))
                    .unwrap_or_else(|e| panic!("{e}\n{error}\n{source}"));
            }
        }
    }
    assert_eq!(checked, MODELS as usize);
    assert!(
        reduced > MODELS as usize / 5,
        "only {reduced} models were reduced"
    );
    assert!(
        failing > 20 && failing < checked,
        "{failing} of {checked} failed"
    );
}
